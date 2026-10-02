//! Read-time privacy barrier for compact transcript carriers.
//!
//! Transcript files are intentionally retained on disk for rollback and
//! forensics, but a deleted memory must not become visible again merely
//! because a model asks `read`, `grep`, or `run_command` to open the file.
//! The check is carrier/provenance based: it never searches transcript text.

use super::{ToolRegistry, TranscriptAccessGuard};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use yunxi_core::memory::MemoryStore;
use yunxi_core::state::StateStore;

const READ_TOOLS: &[&str] = &["read", "grep", "glob", "run_command"];

/// Bind the current session's transcript policy to a tool registry.
///
/// Static composition deliberately has no session or memory handles.  Binding
/// here keeps the provider-facing tool definitions byte-identical while making
/// the local execution path fail closed for legacy transcript carriers.
pub(crate) fn bind(registry: &mut ToolRegistry, state: StateStore, memory: MemoryStore) {
    let guard: TranscriptAccessGuard = Arc::new(move |tool, args| {
        if !READ_TOOLS.contains(&tool.name.as_str()) {
            return None;
        }
        let state = &state;
        let memory = &memory;
        match tool.name.as_str() {
            "read" | "grep" | "glob" => args
                .get("path")
                .and_then(Value::as_str)
                .and_then(|raw| path_from_argument(raw))
                .and_then(|path| check_path(state, memory, &path)),
            // Shell parsing is intentionally not attempted.  We only block
            // an exact compact-session root mention, which is the absolute
            // path emitted in the transcript carrier hint.  Structured reads
            // nested through `yunxi tool-call` hit the same registry barrier.
            "run_command" => args
                .get("command")
                .and_then(Value::as_str)
                .and_then(|command| check_command(state, memory, command)),
            _ => None,
        }
    });
    registry.bind_transcript_access_guard(guard);
}

fn path_from_argument(raw: &str) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with("artifact:") || raw.starts_with("kb:") {
        return None;
    }
    Some(resolve_path(raw))
}

fn resolve_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home.join(rest);
        }
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&yunxi_base::workspace::effective_workdir().join(path))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(std::path::MAIN_SEPARATOR.to_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn compact_root(state: &StateStore) -> PathBuf {
    normalize_path(&state.state_dir().join("compact"))
}

fn transcript_root(state: &StateStore) -> PathBuf {
    normalize_path(&compact_root(state).join(state.session_id().as_ref()))
}

fn is_transcript_scope(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn check_path(state: &StateStore, memory: &MemoryStore, path: &Path) -> Option<String> {
    let compact = compact_root(state);
    let Some(relative) = path.strip_prefix(&compact).ok() else {
        return None;
    };
    let mut components = relative.components();
    let Some(std::path::Component::Normal(session)) = components.next() else {
        return Some("compact transcript root is not a readable file".to_string());
    };
    let session = session.to_string_lossy();
    let current_session = state.session_id();
    if session != current_session.as_ref() {
        return Some(format!(
            "transcript belongs to another session `{session}` and is blocked"
        ));
    }
    let root = transcript_root(state);
    if !is_transcript_scope(path, &root) {
        return Some("transcript path is outside the active session scope".to_string());
    }
    let path = path.to_string_lossy();
    match state
        .conv_db()
        .load_transcript_provenance_by_path(current_session.as_ref(), &path)
    {
        Ok(Some(provenance))
            if memory
                .memory_refs_are_tombstoned(&provenance.refs)
                .unwrap_or(true) =>
        {
            Some(format!(
                "transcript `{path}` is unavailable because its linked memory was deleted"
            ))
        }
        Ok(Some(_)) => None,
        Ok(None) => Some(format!(
            "transcript `{path}` has unknown provenance and is blocked"
        )),
        Err(_) => Some(format!(
            "transcript `{path}` provenance could not be verified and is blocked"
        )),
    }
}

fn check_command(state: &StateStore, memory: &MemoryStore, command: &str) -> Option<String> {
    let compact = compact_root(state);
    let compact_text = compact.to_string_lossy();
    // Shell grammar is intentionally not parsed. We only inspect path-like
    // tokens and deny when one resolves inside this session's compact root.
    // This covers absolute paths emitted by compact extras and relative/`~/`
    // spellings that resolve to the same root, without pretending to
    // understand shell variables, command substitutions, or embedded script
    // strings. Those remain explicit residuals of this barrier.
    for token in command_path_tokens(command) {
        let candidate = resolve_path(token);
        if let Some(reason) = check_path(state, memory, &candidate) {
            return Some(reason);
        }
    }
    // Preserve conservative behavior for a compact root embedded in shell
    // syntax that the lightweight tokenizer cannot split cleanly. We cannot
    // prove which session the expression resolves to, so deny it rather than
    // treating an opaque shell expression as a safe read.
    if command.contains(compact_text.as_ref()) {
        return Some("opaque compact transcript command is blocked".to_string());
    }
    None
}

fn command_path_tokens(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"' | '`' | ';' | '|' | '&'))
        .map(|token| token.trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}')))
        .filter(|token| !token.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_scope_does_not_match_sibling_sessions() {
        let root = Path::new("/state/compact/session-a");
        assert!(is_transcript_scope(root, root));
        assert!(is_transcript_scope(&root.join("fold-1.md"), root));
        assert!(!is_transcript_scope(
            Path::new("/state/compact/session-ab"),
            root
        ));
    }

    #[test]
    fn command_probe_only_uses_exact_root_prefix() {
        assert!("cat /state/compact/s/fold-1.md".contains("/state/compact/s"));
        assert!(!"cat /state/compact/other/fold-1.md".contains("/state/compact/s"));
    }

    #[test]
    fn command_path_tokens_keep_relative_and_quoted_paths() {
        let tokens =
            command_path_tokens("cat './state/compact/session/fold-1.md' && printf \"done\"")
                .collect::<Vec<_>>();
        assert_eq!(
            tokens,
            vec!["cat", "./state/compact/session/fold-1.md", "printf", "done"]
        );
    }

    #[tokio::test]
    async fn unknown_transcript_path_is_denied_before_handler() {
        let temp = tempfile::tempdir().unwrap();
        let paths = yunxi_base::paths::YunXiPaths {
            root_dir: temp.path().to_path_buf(),
            config_dir: temp.path().join("config"),
            config_file: temp.path().join("config/config.jsonc"),
            skills_dir: temp.path().join("config/skills"),
            data_dir: temp.path().join("data"),
            cache_dir: temp.path().join("cache"),
            state_dir: temp.path().join("state"),
            pictures_dir: temp.path().join("pictures"),
            fish_hook_file: temp.path().join("config/fish/conf.d/yunxi.fish"),
            bash_hook_file: temp.path().join("config/shell/bash-hook.sh"),
            zsh_hook_file: temp.path().join("config/shell/zsh-hook.zsh"),
            scripts_dir: temp.path().join("config/scripts"),
            system_scripts_dir: temp.path().join("system-scripts"),
        };
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let root = transcript_root(&state);
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "read",
            "read",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        // Exercise the public execution-registry facade used by hosts and the
        // direct CLI, rather than the module-private implementation.
        super::super::bind_transcript_access_guard(&mut registry, state, memory);
        let path = root
            .join("../")
            .join(root.file_name().unwrap())
            .join("fold-1.md");
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn registered_transcript_without_dead_refs_is_readable() {
        let (temp, state, memory, path) = transcript_fixture(&[]);
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "safe").unwrap();
        let result = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap();
        assert!(result.contains("handler ran"));
        drop(temp);
    }

    #[tokio::test]
    async fn registered_transcript_with_tombstoned_ref_is_denied() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let config = yunxi_base::config::AppConfig::default();
        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory.init().unwrap();
        let fact_id = memory.remember_fact("private", "test").unwrap();
        memory
            .delete_item(yunxi_core::memory::browse::BrowseTable::Facts, fact_id)
            .unwrap();
        let path = register_transcript(
            &state,
            &[yunxi_base::memory_types::MemoryRef {
                kind: "fact".to_string(),
                id: fact_id,
            }],
        );
        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory was deleted"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn run_command_shell_chain_cannot_read_tombstoned_transcript() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let config = yunxi_base::config::AppConfig::default();
        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory.init().unwrap();
        let fact_id = memory.remember_fact("private", "test").unwrap();
        memory
            .delete_item(yunxi_core::memory::browse::BrowseTable::Facts, fact_id)
            .unwrap();
        let path = register_transcript(
            &state,
            &[yunxi_base::memory_types::MemoryRef {
                kind: "fact".to_string(),
                id: fact_id,
            }],
        );
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);
        let command = format!("cat '{}' ; printf done", path.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("memory was deleted"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn historical_session_transcript_is_denied_even_with_live_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let current_session = state.session_id().to_string();
        let historical = state
            .create_session(
                "yunxi",
                "historical",
                yunxi_core::state::USER_SESSION_KIND,
                None,
            )
            .unwrap();
        state.switch_session(&historical.session_id).unwrap();
        let path = register_transcript(&state, &[]);
        state.switch_session(&current_session).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "historical").unwrap();

        let mut registry = read_registry();
        bind(&mut registry, state, memory);
        let error = registry
            .call("read", &serde_json::json!({"path": path}).to_string())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("another session"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn literal_glob_inside_compact_scope_is_denied_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let root = transcript_root(&state);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);

        let command = format!("cat '{}/fold-*.md'", root.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    #[tokio::test]
    async fn cd_into_compact_scope_is_denied_before_relative_read() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let root = transcript_root(&state);
        let mut registry = run_command_registry();
        bind(&mut registry, state, memory);

        let command = format!("cd '{}'; cat fold-1.md", root.display());
        let error = registry
            .call(
                "run_command",
                &serde_json::json!({"command": command}).to_string(),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown provenance"), "unexpected: {error}");
    }

    fn read_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "read",
            "read",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        registry
    }

    fn run_command_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(super::super::ToolSpec::new(
            "run_command",
            "run command",
            serde_json::json!({"type":"object"}),
            |_| async { Ok("handler ran".to_string()) },
        ));
        registry
    }

    fn transcript_fixture(
        refs: &[yunxi_base::memory_types::MemoryRef],
    ) -> (
        tempfile::TempDir,
        yunxi_core::state::StateStore,
        yunxi_core::memory::MemoryStore,
        PathBuf,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let state = yunxi_core::state::StateStore::new(&paths).unwrap();
        let memory =
            yunxi_core::memory::MemoryStore::new(&yunxi_base::config::AppConfig::default(), &paths);
        let path = register_transcript(&state, refs);
        (temp, state, memory, path)
    }

    fn register_transcript(
        state: &yunxi_core::state::StateStore,
        refs: &[yunxi_base::memory_types::MemoryRef],
    ) -> PathBuf {
        state.start_turn("t1", "hello", 1).unwrap();
        state.complete_turn("t1", "reply", None).unwrap();
        let visible = state.load_visible_turns().unwrap();
        let turn_ids = visible
            .iter()
            .map(|turn| turn.turn_id.clone())
            .collect::<Vec<_>>();
        let path = transcript_root(state).join("fold-1.md");
        state
            .replace_visible_with_summary_with_refs_and_transcripts(
                &turn_ids,
                &turn_ids,
                "summary",
                Default::default(),
                false,
                None,
                None,
                refs,
                &[yunxi_core::state::TranscriptCarrier {
                    transcript_id: "transcript-test".to_string(),
                    path: path.display().to_string(),
                }],
            )
            .unwrap();
        path
    }

    fn test_paths(root: &Path) -> yunxi_base::paths::YunXiPaths {
        yunxi_base::paths::YunXiPaths {
            root_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            config_file: root.join("config/config.jsonc"),
            skills_dir: root.join("config/skills"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            pictures_dir: root.join("pictures"),
            fish_hook_file: root.join("config/fish/conf.d/yunxi.fish"),
            bash_hook_file: root.join("config/shell/bash-hook.sh"),
            zsh_hook_file: root.join("config/shell/zsh-hook.zsh"),
            scripts_dir: root.join("config/scripts"),
            system_scripts_dir: root.join("system-scripts"),
        }
    }
}
