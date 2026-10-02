//! Safe, non-prompt metadata for the active persona source.
//!
//! `PersonaSource` is an observation seam for later persona/profile work.  It
//! deliberately carries no prompt text, path, or secret.  Resolving it must
//! never change the prompt path, write a file, or make the normal persona
//! loader fail because optional metadata is malformed.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use yunxi_base::config::AppConfig;
use yunxi_base::paths::YunXiPaths;

/// Wire version for [`PersonaSource`].
pub const PERSONA_SOURCE_SCHEMA_VERSION: u16 = 1;

/// The configured source family, without exposing its filesystem location.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonaSourceKind {
    EmbeddedDefault,
    PersonaFile,
    PrivatePersonaFile,
    LegacyInline,
    LegacyFile,
}

/// Resolution outcome.  Fallback values always use the embedded default
/// prompt revision, so callers can safely display this without reading the
/// original prompt or logging an error.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PersonaSourceStatus {
    Active,
    FallbackMissing,
    FallbackUnreadable,
    FallbackMalformedMetadata,
    FallbackUnsupportedVersion,
}

/// Non-sensitive identity of the active persona source.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PersonaSource {
    pub schema_version: u16,
    pub kind: PersonaSourceKind,
    pub scope: String,
    /// BLAKE3 of the selected prompt bytes, represented as lowercase hex.
    /// Fallback results use the embedded default prompt hash.
    pub revision: String,
    pub status: PersonaSourceStatus,
}

impl PersonaSource {
    /// Resolve the current source using the existing persona path truth
    /// sources.  This function is observational: it performs no writes,
    /// logging, model calls, or prompt assembly.
    pub fn resolve(config: &AppConfig, paths: &YunXiPaths) -> Self {
        let scope = config.active_persona_scope();
        let default_revision = embedded_default_revision();

        if let Some(directory) = config.private_persona_dir() {
            let prompt_path = directory.join("persona.md");
            return resolve_file(
                PersonaSourceKind::PrivatePersonaFile,
                scope,
                &prompt_path,
                Some(directory.join("persona.json")),
                &default_revision,
            );
        }

        let active_persona = config.prompt.active_persona.trim();
        if !active_persona.is_empty() {
            let prompt_path = config.persona_path(paths, active_persona);
            return resolve_file(
                PersonaSourceKind::PersonaFile,
                scope,
                &prompt_path,
                Some(prompt_path.with_extension("json")),
                &default_revision,
            );
        }

        if config
            .system_prompt
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty())
        {
            // The legacy inline value is already the existing
            // `active_persona_prompt` truth source.  Hash only its bytes.
            let prompt = config
                .active_persona_prompt(paths)
                .ok()
                .filter(|prompt| !prompt.trim().is_empty());
            return match prompt {
                Some(prompt) => active(PersonaSourceKind::LegacyInline, scope, prompt.as_bytes()),
                None => fallback(
                    PersonaSourceKind::LegacyInline,
                    scope,
                    PersonaSourceStatus::FallbackUnreadable,
                    &default_revision,
                ),
            };
        }

        let prompt_path = config.system_prompt_path(paths);
        if prompt_path.is_file() || custom_legacy_file_is_configured(config, paths) {
            // `custom_system_prompt` remains the source of truth for legacy
            // file loading; this branch only adds a safe identity around it.
            return match config.custom_system_prompt(paths) {
                Ok(prompt) if !prompt.trim().is_empty() => {
                    active(PersonaSourceKind::LegacyFile, scope, prompt.as_bytes())
                }
                Ok(_) => fallback(
                    PersonaSourceKind::LegacyFile,
                    scope,
                    PersonaSourceStatus::FallbackMissing,
                    &default_revision,
                ),
                Err(_) => fallback(
                    PersonaSourceKind::LegacyFile,
                    scope,
                    PersonaSourceStatus::FallbackUnreadable,
                    &default_revision,
                ),
            };
        }

        // A missing default `system-prompt.md` is normal: the base loader
        // then uses the embedded YunXi prompt.  A deliberately configured
        // non-default file is an actual missing legacy source.
        Self {
            schema_version: PERSONA_SOURCE_SCHEMA_VERSION,
            kind: PersonaSourceKind::EmbeddedDefault,
            scope,
            revision: default_revision,
            status: PersonaSourceStatus::Active,
        }
    }

    /// Parse a serialized source and reject unknown schema versions.
    pub fn from_json(input: &[u8]) -> Option<Self> {
        let source: Self = serde_json::from_slice(input).ok()?;
        (source.schema_version == PERSONA_SOURCE_SCHEMA_VERSION).then_some(source)
    }

    pub fn is_active(&self) -> bool {
        self.schema_version == PERSONA_SOURCE_SCHEMA_VERSION
            && self.status == PersonaSourceStatus::Active
    }
}

fn resolve_file(
    kind: PersonaSourceKind,
    scope: String,
    prompt_path: &Path,
    sidecar_path: Option<PathBuf>,
    default_revision: &str,
) -> PersonaSource {
    let prompt = match std::fs::read(prompt_path) {
        Ok(bytes) if !bytes.is_empty() && !bytes_are_empty(&bytes) => {
            if std::str::from_utf8(&bytes).is_err() {
                return fallback(
                    kind,
                    scope,
                    PersonaSourceStatus::FallbackUnreadable,
                    default_revision,
                );
            }
            bytes
        }
        Ok(_) => {
            return fallback(
                kind,
                scope,
                PersonaSourceStatus::FallbackMissing,
                default_revision,
            )
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return fallback(
                kind,
                scope,
                PersonaSourceStatus::FallbackMissing,
                default_revision,
            )
        }
        Err(_) => {
            return fallback(
                kind,
                scope,
                PersonaSourceStatus::FallbackUnreadable,
                default_revision,
            )
        }
    };

    if let Some(sidecar_path) = sidecar_path {
        match validate_sidecar(&sidecar_path) {
            SidecarStatus::Valid | SidecarStatus::Missing => {}
            SidecarStatus::Unreadable => {
                return fallback(
                    kind,
                    scope,
                    PersonaSourceStatus::FallbackUnreadable,
                    default_revision,
                )
            }
            SidecarStatus::Malformed => {
                return fallback(
                    kind,
                    scope,
                    PersonaSourceStatus::FallbackMalformedMetadata,
                    default_revision,
                )
            }
            SidecarStatus::UnsupportedVersion => {
                return fallback(
                    kind,
                    scope,
                    PersonaSourceStatus::FallbackUnsupportedVersion,
                    default_revision,
                )
            }
        }
    }

    active(kind, scope, &prompt)
}

fn active(kind: PersonaSourceKind, scope: String, prompt: &[u8]) -> PersonaSource {
    PersonaSource {
        schema_version: PERSONA_SOURCE_SCHEMA_VERSION,
        kind,
        scope,
        revision: blake3::hash(prompt).to_hex().to_string(),
        status: PersonaSourceStatus::Active,
    }
}

fn fallback(
    kind: PersonaSourceKind,
    scope: String,
    status: PersonaSourceStatus,
    default_revision: &str,
) -> PersonaSource {
    PersonaSource {
        schema_version: PERSONA_SOURCE_SCHEMA_VERSION,
        kind,
        scope,
        revision: default_revision.to_string(),
        status,
    }
}

fn embedded_default_revision() -> String {
    blake3::hash(yunxi_base::prompts::default_system_prompt().as_bytes())
        .to_hex()
        .to_string()
}

fn bytes_are_empty(bytes: &[u8]) -> bool {
    bytes.iter().all(u8::is_ascii_whitespace)
}

fn custom_legacy_file_is_configured(config: &AppConfig, paths: &YunXiPaths) -> bool {
    let Some(value) = config.system_prompt_file.as_deref().map(str::trim) else {
        return false;
    };
    if value.is_empty() || value == "system-prompt.md" || value == "./system-prompt.md" {
        return false;
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return path != paths.config_dir.join("system-prompt.md")
            && paths
                .legacy_config_dir()
                .is_none_or(|legacy| path != legacy.join("system-prompt.md"));
    }
    true
}

enum SidecarStatus {
    Valid,
    Missing,
    Unreadable,
    Malformed,
    UnsupportedVersion,
}

fn validate_sidecar(path: &Path) -> SidecarStatus {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return SidecarStatus::Missing
        }
        Err(_) => return SidecarStatus::Unreadable,
    };
    let value: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => return SidecarStatus::Malformed,
    };
    if !value.is_object() {
        return SidecarStatus::Malformed;
    }
    let Some(version) = value.get("schema_version") else {
        return SidecarStatus::Valid;
    };
    match version.as_u64() {
        Some(version) if version == u64::from(PERSONA_SOURCE_SCHEMA_VERSION) => {
            SidecarStatus::Valid
        }
        Some(_) => SidecarStatus::UnsupportedVersion,
        None => SidecarStatus::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_paths(root: &Path) -> YunXiPaths {
        YunXiPaths {
            root_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            config_file: root.join("config/config.jsonc"),
            skills_dir: root.join("config/skills"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            pictures_dir: root.join("pictures"),
            fish_hook_file: root.join("fish/yunxi.fish"),
            bash_hook_file: root.join("shell/bash-hook.sh"),
            zsh_hook_file: root.join("shell/zsh-hook.zsh"),
            scripts_dir: root.join("config/scripts"),
            system_scripts_dir: PathBuf::new(),
        }
    }

    fn config_and_paths(root: &Path) -> (AppConfig, YunXiPaths) {
        let paths = test_paths(root);
        fs::create_dir_all(&paths.config_dir).unwrap();
        (AppConfig::default(), paths)
    }

    fn assert_no_path(source: &PersonaSource, path: &Path) {
        let encoded = serde_json::to_string(source).unwrap();
        assert!(!encoded.contains(&path.to_string_lossy().to_string()));
        assert!(!encoded.contains("persona.md"));
    }

    #[test]
    fn default_embedded_source_is_active_and_stable() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        let source = PersonaSource::resolve(&config, &paths);
        assert_eq!(source.kind, PersonaSourceKind::EmbeddedDefault);
        assert_eq!(source.status, PersonaSourceStatus::Active);
        assert_eq!(source.scope, "default");
        assert_eq!(source.revision, embedded_default_revision());
        assert!(source.is_active());

        config.system_prompt_file = Some(
            paths
                .config_dir
                .join("system-prompt.md")
                .to_string_lossy()
                .to_string(),
        );
        let absolute_default = PersonaSource::resolve(&config, &paths);
        assert_eq!(absolute_default.kind, PersonaSourceKind::EmbeddedDefault);
        assert_eq!(absolute_default.status, PersonaSourceStatus::Active);
    }

    #[test]
    fn shared_persona_file_uses_existing_scope_and_hashes_only_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        config.prompt.active_persona = "Warm.md".to_string();
        let file = config.persona_path(&paths, "Warm.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "shared persona\n").unwrap();
        let source = PersonaSource::resolve(&config, &paths);
        assert_eq!(source.kind, PersonaSourceKind::PersonaFile);
        assert_eq!(source.status, PersonaSourceStatus::Active);
        assert_eq!(source.scope, config.active_persona_scope());
        assert_eq!(
            source.revision,
            blake3::hash(b"shared persona\n").to_hex().to_string()
        );
        assert_no_path(&source, &file);
    }

    #[test]
    fn private_persona_file_uses_private_scope() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        let dir = temp.path().join("home/alice/personas/close");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("persona.md"), "private persona").unwrap();
        config.prompt.private_persona_dir = Some(dir.to_string_lossy().to_string());
        let source = PersonaSource::resolve(&config, &paths);
        assert_eq!(source.kind, PersonaSourceKind::PrivatePersonaFile);
        assert_eq!(source.status, PersonaSourceStatus::Active);
        assert_eq!(source.scope, "home-alice-close");
        assert_no_path(&source, &dir);
    }

    #[test]
    fn legacy_inline_and_file_have_distinct_kinds() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        config.system_prompt = Some("inline legacy".to_string());
        let inline = PersonaSource::resolve(&config, &paths);
        assert_eq!(inline.kind, PersonaSourceKind::LegacyInline);
        assert_eq!(inline.status, PersonaSourceStatus::Active);

        config.system_prompt = None;
        config.system_prompt_file = Some("legacy.md".to_string());
        let file = config.system_prompt_path(&paths);
        fs::write(&file, "file legacy").unwrap();
        let legacy = PersonaSource::resolve(&config, &paths);
        assert_eq!(legacy.kind, PersonaSourceKind::LegacyFile);
        assert_eq!(legacy.status, PersonaSourceStatus::Active);

        fs::write(&file, " \n").unwrap();
        let empty = PersonaSource::resolve(&config, &paths);
        assert_eq!(empty.status, PersonaSourceStatus::FallbackMissing);
    }

    #[test]
    fn missing_empty_and_directory_prompt_fallback_without_path() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        config.prompt.active_persona = "missing.md".to_string();
        let missing = PersonaSource::resolve(&config, &paths);
        assert_eq!(missing.status, PersonaSourceStatus::FallbackMissing);
        assert_eq!(missing.revision, embedded_default_revision());

        let file = config.persona_path(&paths, "missing.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, " \n\t").unwrap();
        let empty = PersonaSource::resolve(&config, &paths);
        assert_eq!(empty.status, PersonaSourceStatus::FallbackMissing);
        assert_no_path(&empty, &file);

        fs::remove_file(&file).unwrap();
        fs::create_dir(&file).unwrap();
        let unreadable = PersonaSource::resolve(&config, &paths);
        assert_eq!(unreadable.status, PersonaSourceStatus::FallbackUnreadable);

        fs::remove_dir(&file).unwrap();
        fs::write(&file, [0xff, 0xfe]).unwrap();
        let invalid_utf8 = PersonaSource::resolve(&config, &paths);
        assert_eq!(invalid_utf8.status, PersonaSourceStatus::FallbackUnreadable);
    }

    #[test]
    fn sidecar_validation_is_optional_but_safe() {
        let temp = tempfile::tempdir().unwrap();
        let (mut config, paths) = config_and_paths(temp.path());
        config.prompt.active_persona = "sidecar.md".to_string();
        let file = config.persona_path(&paths, "sidecar.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "sidecar persona").unwrap();

        let active_without_metadata = PersonaSource::resolve(&config, &paths);
        assert_eq!(active_without_metadata.status, PersonaSourceStatus::Active);

        fs::write(file.with_extension("json"), "not-json").unwrap();
        assert_eq!(
            PersonaSource::resolve(&config, &paths).status,
            PersonaSourceStatus::FallbackMalformedMetadata
        );
        fs::write(file.with_extension("json"), "[]").unwrap();
        assert_eq!(
            PersonaSource::resolve(&config, &paths).status,
            PersonaSourceStatus::FallbackMalformedMetadata
        );
        fs::write(file.with_extension("json"), r#"{"schema_version": 0}"#).unwrap();
        assert_eq!(
            PersonaSource::resolve(&config, &paths).status,
            PersonaSourceStatus::FallbackUnsupportedVersion
        );
        fs::write(
            file.with_extension("json"),
            r#"{"schema_version": 1, "avatar": "ignored"}"#,
        )
        .unwrap();
        assert_eq!(
            PersonaSource::resolve(&config, &paths).status,
            PersonaSourceStatus::Active
        );
    }

    #[test]
    fn same_prompt_bytes_have_same_revision_across_roots() {
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        let (mut config_one, paths_one) = config_and_paths(one.path());
        let (mut config_two, paths_two) = config_and_paths(two.path());
        config_one.prompt.active_persona = "same.md".to_string();
        config_two.prompt.active_persona = "same.md".to_string();
        let one_file = config_one.persona_path(&paths_one, "same.md");
        let two_file = config_two.persona_path(&paths_two, "same.md");
        fs::create_dir_all(one_file.parent().unwrap()).unwrap();
        fs::create_dir_all(two_file.parent().unwrap()).unwrap();
        fs::write(&one_file, "same bytes").unwrap();
        fs::write(&two_file, "same bytes").unwrap();
        assert_eq!(
            PersonaSource::resolve(&config_one, &paths_one).revision,
            PersonaSource::resolve(&config_two, &paths_two).revision
        );
    }

    #[test]
    fn serde_roundtrip_rejects_unknown_version() {
        let temp = tempfile::tempdir().unwrap();
        let (config, paths) = config_and_paths(temp.path());
        let source = PersonaSource::resolve(&config, &paths);
        let encoded = serde_json::to_vec(&source).unwrap();
        assert_eq!(PersonaSource::from_json(&encoded), Some(source));
        let mut old: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        old["schema_version"] = serde_json::json!(99);
        assert!(PersonaSource::from_json(&serde_json::to_vec(&old).unwrap()).is_none());
    }
}
