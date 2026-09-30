//! `yunxi import`: restore an exported installation onto this machine.

use super::export::yunxi_home;
use super::manifest::{Manifest, MANIFEST_FORMAT_VERSION, MANIFEST_NAME};
use super::registry::unit_for;
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use yunxi_base::i18n::text as t;
use yunxi_base::paths::YunXiPaths;

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    pub force: bool,
}

#[derive(Debug)]
pub struct ImportReport {
    pub restored: usize,
    pub unknown_units: BTreeSet<String>,
    pub backup: Option<PathBuf>,
    pub secrets_included: bool,
    pub index_included: bool,
    /// Session workspaces that pointed at directories this machine does not
    /// have; cleared so a turn does not try to run in them.
    pub cleared_workspaces: usize,
}

pub fn import(paths: &YunXiPaths, archive: &Path, options: &ImportOptions) -> Result<ImportReport> {
    let root = yunxi_home(paths)?;
    // Validate the complete archive before checking occupancy, creating a
    // backup, or extracting anything. The manifest and tar stream must agree
    // on exactly the same set of regular files.
    let manifest = read_manifest(archive)?;
    check_versions(&manifest)?;

    if !options.force {
        if let Some(reason) = occupied(paths) {
            bail!(
                "{}\n{}",
                reason,
                t(
                    "pass --force to overwrite (the current data is exported to a backup archive first)",
                    "如需覆盖请传 --force（覆盖前会先把现有数据导出成备份包）"
                )
            );
        }
    }

    let backup = if options.force && occupied(paths).is_some() {
        Some(backup_current(paths, archive)?)
    } else {
        None
    };

    // Unpack beside the target first: a half-extracted archive must never be
    // able to leave YUNXI_HOME in a mixed state.
    let staging = tempfile::tempdir_in(root.parent().unwrap_or(&root))
        .context("creating a staging directory")?;
    let staged = staging.path().join("home");
    extract(archive, staging.path())?;

    let mut unknown_units = BTreeSet::new();
    for entry in &manifest.entries {
        if unit_for(&entry.path).is_none() {
            // Written by a newer build that knows about data this one does
            // not. Restoring it verbatim is strictly better than dropping it.
            unknown_units.insert(entry.unit.clone());
        }
    }

    let restored = install(&staged, &root)?;
    stamp_layout_markers(&root)?;
    let cleared_workspaces = super::fixups::apply(paths)?;

    Ok(ImportReport {
        restored,
        unknown_units,
        backup,
        secrets_included: manifest.secrets_included,
        index_included: manifest.scope.index,
        cleared_workspaces,
    })
}

/// Why importing here would destroy something, or `None` when the target is
/// effectively empty.
fn occupied(paths: &YunXiPaths) -> Option<String> {
    if paths.config_file.exists() {
        return Some(format!(
            "{}: {}",
            t("a configuration already exists", "目标已有配置"),
            paths.config_file.display()
        ));
    }
    let conversations = paths.state_dir.join("conversation.db");
    if conversations.exists() {
        return Some(format!(
            "{}: {}",
            t("conversation history already exists", "目标已有会话历史"),
            conversations.display()
        ));
    }
    None
}

fn read_manifest(archive: &Path) -> Result<Manifest> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    let mut manifest = None;
    let mut archive_entries = BTreeMap::new();
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("archive entry path is not valid UTF-8"))?;
        if path == MANIFEST_NAME {
            if manifest.is_some() {
                bail!("archive contains duplicate {MANIFEST_NAME} entries");
            }
            require_regular_file(&entry, &path)?;
            let bytes = read_entry_bytes(&mut entry, &path)?;
            manifest =
                Some(serde_json::from_slice(&bytes).context("parsing the archive manifest")?);
            continue;
        }

        let rel = path
            .strip_prefix("home/")
            .filter(|rel| !rel.is_empty())
            .ok_or_else(|| anyhow::anyhow!("archive contains undeclared path: {path}"))?;
        validate_manifest_path(rel)?;
        require_regular_file(&entry, &path)?;
        if archive_entries.contains_key(rel) {
            bail!("archive contains duplicate path: {path}");
        }
        let (size, blake3) = hash_entry(&mut entry, &path)?;
        archive_entries.insert(rel.to_owned(), (size, blake3));
    }

    let manifest = manifest.ok_or_else(|| {
        anyhow::anyhow!(
            "{}",
            t(
                "this file has no YunXi manifest; it is not a yunxi export archive",
                "包里没有 YunXi 清单，这不是 yunxi export 生成的归档"
            )
        )
    })?;
    validate_manifest_entries(&manifest, &archive_entries)?;
    Ok(manifest)
}

fn validate_manifest_entries(
    manifest: &Manifest,
    archive_entries: &BTreeMap<String, (u64, String)>,
) -> Result<()> {
    let mut manifest_entries = BTreeSet::new();
    for item in &manifest.entries {
        validate_manifest_path(&item.path)?;
        if !manifest_entries.insert(item.path.as_str()) {
            bail!("manifest contains duplicate path: {}", item.path);
        }
        if let Some(unit) = unit_for(&item.path) {
            if item.unit != unit.id {
                bail!(
                    "manifest unit mismatch for {}: declared {}, expected {}",
                    item.path,
                    item.unit,
                    unit.id
                );
            }
        }
        let Some((actual_size, actual_blake3)) = archive_entries.get(&item.path) else {
            bail!("manifest entry is missing from archive: {}", item.path);
        };
        if item.size != *actual_size {
            bail!(
                "manifest size mismatch for {}: declared {}, archive {}",
                item.path,
                item.size,
                actual_size
            );
        }
        if item.blake3 != *actual_blake3 {
            bail!("manifest blake3 mismatch for {}", item.path);
        }
    }
    if manifest_entries.len() != archive_entries.len() {
        let extra = archive_entries
            .keys()
            .find(|path| !manifest_entries.contains(path.as_str()))
            .expect("length differs, so one archive path must be extra");
        bail!("archive contains undeclared path: home/{extra}");
    }
    Ok(())
}

fn validate_manifest_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || path.contains(':')
        || path.starts_with('/')
        || path.ends_with('/')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("invalid manifest path: {path:?}");
    }
    Ok(())
}

fn require_regular_file(entry: &tar::Entry<'_, GzDecoder<File>>, path: &str) -> Result<()> {
    if !entry.header().entry_type().is_file() {
        bail!("archive entry is not a regular file: {path}");
    }
    Ok(())
}

fn read_entry_bytes(entry: &mut tar::Entry<'_, GzDecoder<File>>, path: &str) -> Result<Vec<u8>> {
    let expected = entry.size();
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading archive entry {path}"))?;
    if bytes.len() as u64 != expected {
        bail!(
            "archive entry size mismatch for {path}: header {expected}, read {}",
            bytes.len()
        );
    }
    Ok(bytes)
}

fn hash_entry(entry: &mut tar::Entry<'_, GzDecoder<File>>, path: &str) -> Result<(u64, String)> {
    let expected = entry.size();
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 32 * 1024];
    let mut size = 0u64;
    loop {
        let read = entry
            .read(&mut buffer)
            .with_context(|| format!("reading archive entry {path}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    if size != expected {
        bail!("archive entry size mismatch for {path}: header {expected}, read {size}");
    }
    Ok((size, hasher.finalize().to_hex().to_string()))
}

/// Refuses archives this build cannot safely open, mirroring the checks
/// `AppConfig::migrate` and the schema migrations already make.
fn check_versions(manifest: &Manifest) -> Result<()> {
    if manifest.format_version != MANIFEST_FORMAT_VERSION {
        bail!(
            "{} ({} != {})",
            t(
                "the archive manifest format is not supported by this build",
                "当前 YunXi 不支持这个归档清单格式"
            ),
            manifest.format_version,
            MANIFEST_FORMAT_VERSION
        );
    }
    if manifest.config_version > yunxi_base::config::CURRENT_CONFIG_VERSION {
        bail!(
            "{} ({} > {})",
            t(
                "the archive's configuration is newer than this build supports; upgrade YunXi first",
                "包里的配置版本高于当前 YunXi 支持的版本；请先升级 YunXi"
            ),
            manifest.config_version,
            yunxi_base::config::CURRENT_CONFIG_VERSION
        );
    }
    let newer = manifest.schemas_newer_than(yunxi_core::state::latest_schema_version());
    if !newer.is_empty() {
        let detail = newer
            .iter()
            .map(|(unit, version)| format!("{unit}={version}"))
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "{} ({detail} > {})",
            t(
                "the archive's database schema is newer than this build supports; upgrade YunXi first",
                "包里的数据库 schema 高于当前 YunXi 支持的版本；请先升级 YunXi"
            ),
            yunxi_core::state::latest_schema_version()
        );
    }
    Ok(())
}

fn extract(archive: &Path, into: &Path) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("archive entry path is not valid UTF-8"))?;
        if !entry.header().entry_type().is_file() {
            bail!("archive entry is not a regular file: {path}");
        }
        if path != MANIFEST_NAME {
            let rel = path
                .strip_prefix("home/")
                .filter(|rel| !rel.is_empty())
                .ok_or_else(|| anyhow::anyhow!("archive contains undeclared path: {path}"))?;
            validate_manifest_path(rel)?;
        }
        let unpacked = entry
            .unpack_in(into)
            .with_context(|| format!("extracting {path}"))?;
        if !unpacked {
            bail!("archive entry was rejected by tar path safety checks: {path}");
        }
    }
    Ok(())
}

/// Moves the staged tree into place, replacing whatever it covers.
fn install(staged: &Path, root: &Path) -> Result<usize> {
    ensure_real_dir(staged, false)?;
    ensure_real_dir(root, false)?;
    let mut installed = 0usize;
    let mut stack = vec![staged.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for child in std::fs::read_dir(&dir)? {
            let child = child?;
            let path = child.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                bail!("staged archive contains a symlink: {}", path.display());
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                bail!(
                    "staged archive contains a non-regular file: {}",
                    path.display()
                );
            }
            let rel = path
                .strip_prefix(staged)
                .context("staged path outside the staging root")?;
            let destination = root.join(rel);
            if let Some(parent) = destination.parent() {
                ensure_destination_parent(root, parent)?;
            }
            if let Ok(destination_metadata) = std::fs::symlink_metadata(&destination) {
                if destination_metadata.file_type().is_symlink() {
                    bail!(
                        "refusing to replace a destination symlink: {}",
                        destination.display()
                    );
                }
                if !destination_metadata.is_file() {
                    bail!(
                        "destination is not a regular file: {}",
                        destination.display()
                    );
                }
            }
            // Rename first (same filesystem, atomic); fall back to a copy when
            // the staging dir landed elsewhere.
            if std::fs::rename(&path, &destination).is_err() {
                copy_into_place(&path, &destination)
                    .with_context(|| format!("installing {}", rel.display()))?;
            }
            installed += 1;
        }
    }
    Ok(installed)
}

fn ensure_real_dir(path: &Path, create: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                bail!("refusing to use a directory symlink: {}", path.display());
            }
            if !metadata.is_dir() {
                bail!("expected a directory: {}", path.display());
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
            std::fs::create_dir(path)?;
        }
        Err(error) => return Err(error).with_context(|| format!("checking {}", path.display())),
    }
    Ok(())
}

fn ensure_destination_parent(root: &Path, parent: &Path) -> Result<()> {
    let relative = parent
        .strip_prefix(root)
        .context("destination parent escaped the install root")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            bail!("destination parent contains a non-normal path component");
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(_) => ensure_real_dir(&current, false)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ensure_real_dir(&current, true)?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", current.display()));
            }
        }
    }
    Ok(())
}

fn copy_into_place(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("destination has no parent directory")?;
    let temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;
    std::fs::copy(source, temporary.path())
        .with_context(|| format!("copying {}", source.display()))?;
    let temporary = temporary.into_temp_path();
    std::fs::rename(&temporary, destination)
        .with_context(|| format!("renaming temporary file to {}", destination.display()))?;
    Ok(())
}

/// Marks the restored tree as already using the current layout, so
/// `YunXiPaths::new` does not try to migrate it from a legacy one.
fn stamp_layout_markers(root: &Path) -> Result<()> {
    for marker in [".layout-v1", ".resource-layout-v1"] {
        let path = root.join(marker);
        if !path.exists() {
            std::fs::write(&path, "1").with_context(|| format!("writing {}", path.display()))?;
        }
    }
    Ok(())
}

fn backup_current(paths: &YunXiPaths, archive: &Path) -> Result<PathBuf> {
    let directory = archive.parent().unwrap_or(Path::new("."));
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let destination = directory.join(format!("yunxi-backup-{stamp}.tar.gz"));
    let options = super::export::ExportOptions {
        all: true,
        ..Default::default()
    };
    super::export::export(paths, &destination, &options)
        .context("backing up the current installation before overwriting it")?;
    Ok(destination)
}
