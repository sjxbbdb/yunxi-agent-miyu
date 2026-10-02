//! `yunxi import`: restore an exported installation onto this machine.

use super::export::{skip_name, yunxi_home};
use super::manifest::{Manifest, MANIFEST_FORMAT_VERSION, MANIFEST_NAME};
use super::registry::{unit_for, Tier, UNITS};
use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use yunxi_base::i18n::text as t;
use yunxi_base::paths::YunXiPaths;

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd};

/// Hard limits for untrusted import archives. These are deliberately kept out
/// of configuration and the archive format: changing them is a code-level
/// security policy change, not a per-installation tuning knob.
pub(super) const MAX_ARCHIVE_ENTRIES: u64 = 100_000;
pub(super) const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(super) const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Default)]
struct ArchiveBudget {
    entries: u64,
    uncompressed_bytes: u64,
}

impl ArchiveBudget {
    fn observe_entry(&mut self, header_bytes: u64) -> Result<()> {
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("archive entry count overflow"))?;
        if self.entries > MAX_ARCHIVE_ENTRIES {
            bail!(
                "archive contains too many entries: {} (limit {})",
                self.entries,
                MAX_ARCHIVE_ENTRIES
            );
        }

        self.uncompressed_bytes = self
            .uncompressed_bytes
            .checked_add(header_bytes)
            .ok_or_else(|| anyhow::anyhow!("archive uncompressed byte count overflow"))?;
        if self.uncompressed_bytes > MAX_ARCHIVE_UNCOMPRESSED_BYTES {
            bail!(
                "archive declares too many uncompressed bytes: {} (limit {})",
                self.uncompressed_bytes,
                MAX_ARCHIVE_UNCOMPRESSED_BYTES
            );
        }
        Ok(())
    }

    fn check_manifest_size(header_bytes: u64) -> Result<()> {
        if header_bytes > MAX_MANIFEST_BYTES {
            bail!(
                "archive manifest is too large: {} bytes (limit {})",
                header_bytes,
                MAX_MANIFEST_BYTES
            );
        }
        Ok(())
    }
}

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
    /// Core files removed from the live tree by a force import with explicit
    /// manifest coverage. Legacy/partial manifests report zero.
    pub removed_stale: usize,
}

pub fn import(paths: &YunXiPaths, archive: &Path, options: &ImportOptions) -> Result<ImportReport> {
    let root = yunxi_home(paths)?;
    // Validate the complete archive before checking occupancy, creating a
    // backup, or extracting anything. The manifest and tar stream must agree
    // on exactly the same set of regular files.
    let manifest = read_manifest(archive)?;
    check_versions(&manifest)?;
    let occupied_reason = occupied(paths);

    if !options.force {
        if let Some(reason) = occupied_reason.as_ref() {
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

    let stale_core = if options.force {
        stale_core_files(&root, &manifest, archive)?
    } else {
        Vec::new()
    };

    let backup = if options.force && (occupied_reason.is_some() || !stale_core.is_empty()) {
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
    validate_staged(&staged, &manifest)?;

    // Rewrite machine-specific fields while the restored databases are still
    // in staging, so a SQLite failure cannot leave the live tree half changed.
    let staged_databases = manifest
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                unit_for(&entry.path).map(|unit| unit.id),
                Some("state.conversation" | "home.conversation")
            )
        })
        .map(|entry| staged.join(&entry.path))
        .collect::<Vec<_>>();
    let cleared_workspaces = super::fixups::apply_database_paths(&staged_databases)?;
    // A force import may be restoring an older snapshot than the current
    // installation.  Carry current memory deletion tombstones into each
    // staged persona DB before the atomic install so deleted ids cannot be
    // resurrected by that archive.
    let staged_memory_databases = manifest
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                unit_for(&entry.path).map(|unit| unit.id),
                Some("data.persona_memory" | "personas.memory")
            )
        })
        .map(|entry| (root.join(&entry.path), staged.join(&entry.path)))
        .collect::<Vec<_>>();
    super::fixups::apply_memory_tombstones(&staged_memory_databases)?;

    let mut unknown_units = BTreeSet::new();
    for entry in &manifest.entries {
        if unit_for(&entry.path).is_none() {
            // Written by a newer build that knows about data this one does
            // not. Restoring it verbatim is strictly better than dropping it.
            unknown_units.insert(entry.unit.clone());
        }
    }

    let rollback = tempfile::tempdir_in(root.parent().unwrap_or(&root))
        .context("creating an import rollback directory")?;
    let mut undo = Vec::new();
    let mut removed_stale = 0;
    let restored = match prune_stale_core(&stale_core, &root, rollback.path(), &mut undo)
        .and_then(|removed| {
            removed_stale = removed;
            install(&staged, &root, rollback.path(), &mut undo)
        })
        .and_then(|restored| {
            stamp_layout_markers(&root, rollback.path(), &mut undo)?;
            Ok(restored)
        }) {
        Ok(restored) => restored,
        Err(error) => {
            let rollback_error = rollback_install(&undo, &root);
            return match rollback_error {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error.context(format!(
                    "import failed and rollback also failed: {rollback_error}"
                ))),
            };
        }
    };

    Ok(ImportReport {
        restored,
        unknown_units,
        backup,
        secrets_included: manifest.secrets_included,
        index_included: manifest.scope.index,
        cleared_workspaces,
        removed_stale,
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
    let databases = match super::fixups::conversation_db_paths(paths) {
        Ok(databases) => databases,
        Err(error) => return Some(format!("cannot inspect conversation databases: {error}")),
    };
    if let Some(conversations) = databases.into_iter().find(|path| path.exists()) {
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
    let mut budget = ArchiveBudget::default();
    let mut manifest = None;
    let mut archive_entries = BTreeMap::new();
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry?;
        budget.observe_entry(entry.size())?;
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
            ArchiveBudget::check_manifest_size(entry.size())?;
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
            if unit.tier == Tier::Never {
                bail!("manifest contains a non-transferable path: {}", item.path);
            }
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
    if let Some(included_units) = &manifest.included_units {
        for id in included_units {
            let Some(unit) = UNITS.iter().find(|unit| unit.id == id) else {
                // Unknown units are forward-compatible and are restored
                // verbatim; they must not participate in stale pruning.
                continue;
            };
            if unit.tier == Tier::Never {
                bail!("manifest coverage includes a non-transferable unit: {id}");
            }
            if unit.tier == Tier::Heavy && !manifest.scope.index {
                bail!("manifest coverage includes Heavy unit {id} without index scope");
            }
            if unit.tier == Tier::Platform && !manifest.scope.platforms {
                bail!("manifest coverage includes Platform unit {id} without platform scope");
            }
        }
        for entry in &manifest.entries {
            if let Some(unit) = unit_for(&entry.path) {
                if !included_units.contains(unit.id) {
                    bail!(
                        "manifest entry {} is outside declared unit coverage {}",
                        entry.path,
                        unit.id
                    );
                }
            }
        }
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
    std::fs::create_dir_all(into.join("home"))?;
    let file = File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut tar = tar::Archive::new(GzDecoder::new(file));
    let mut budget = ArchiveBudget::default();
    for entry in tar.entries().context("reading the archive")? {
        let mut entry = entry?;
        budget.observe_entry(entry.size())?;
        let path = entry
            .path()?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("archive entry path is not valid UTF-8"))?;
        if !entry.header().entry_type().is_file() {
            bail!("archive entry is not a regular file: {path}");
        }
        if path == MANIFEST_NAME {
            ArchiveBudget::check_manifest_size(entry.size())?;
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

/// Lists live regular files owned by Core units but absent from the incoming
/// manifest. Only `--force` calls this helper; Heavy, Platform, Never, and
/// unknown paths are deliberately left alone so a partial-scope import cannot
/// erase data it did not declare.
fn stale_core_files(root: &Path, manifest: &Manifest, archive: &Path) -> Result<Vec<PathBuf>> {
    let Some(included_units) = manifest.included_units.as_ref() else {
        // Legacy and hand-authored manifests do not prove which units were
        // selected. Treat them as merge-only to avoid deleting live data.
        return Ok(Vec::new());
    };
    let expected = manifest
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let protected_archive = std::fs::canonicalize(archive).ok();
    let mut stale = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for child in std::fs::read_dir(&directory)? {
            let child = child?;
            let path = child.path();
            let file_name = child.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if skip_name(name) {
                continue;
            }
            let metadata = std::fs::symlink_metadata(&path)?;
            if protected_archive.as_deref().is_some_and(|protected| {
                std::fs::canonicalize(&path).ok().as_deref() == Some(protected)
            }) {
                continue;
            }
            if metadata.file_type().is_symlink() {
                // Never follow or delete a symlink while pruning. Export
                // already rejects these; preserving one is the safe fallback
                // for an existing home that was edited outside YunXi.
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .context("stale path outside the YunXi home")?
                .to_str()
                .map(str::to_owned);
            let Some(rel) = rel else {
                continue;
            };
            let rel = rel.replace(std::path::MAIN_SEPARATOR, "/");
            let owner = unit_for(&rel);
            if metadata.is_dir() {
                if owner.is_some_and(|unit| unit.tier != Tier::Core) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                // Unknown sockets/FIFOs/etc. are outside the transfer
                // registry. Leave them untouched rather than turning a
                // force import into an unrelated cleanup operation.
                continue;
            }
            if expected.contains(rel.as_str()) {
                continue;
            }
            if owner.is_some_and(|unit| unit.tier == Tier::Core && included_units.contains(unit.id))
            {
                stale.push(path);
            }
        }
    }
    stale.sort();
    Ok(stale)
}

#[cfg(not(unix))]
fn prune_stale_core(
    stale: &[PathBuf],
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<usize> {
    for destination in stale {
        move_existing(destination, root, rollback, undo)?;
    }
    Ok(stale.len())
}

/// Re-hashes the extracted tree so replacing the archive between the initial
/// manifest pass and extraction cannot install bytes that were never checked.
fn validate_staged(staged: &Path, manifest: &Manifest) -> Result<()> {
    let expected: BTreeMap<&str, (&str, u64)> = manifest
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), (entry.blake3.as_str(), entry.size)))
        .collect();
    let mut seen = BTreeSet::new();
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
                .context("staged path outside the staging root")?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("staged path is not valid UTF-8"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let Some((expected_hash, expected_size)) = expected.get(rel.as_str()) else {
                bail!("staged archive contains an undeclared path: {rel}");
            };
            let (size, hash) = hash_file(&path)?;
            if size != *expected_size || hash != *expected_hash {
                bail!("staged content does not match manifest: {rel}");
            }
            seen.insert(rel);
        }
    }
    if seen.len() != expected.len() {
        let missing = expected
            .keys()
            .find(|path| !seen.contains(**path))
            .copied()
            .unwrap_or("<unknown>");
        bail!("staged archive is missing a manifest entry: {missing}");
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 32 * 1024];
    let mut size = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size += read as u64;
    }
    Ok((size, hasher.finalize().to_hex().to_string()))
}

#[derive(Debug)]
struct UndoEntry {
    destination: PathBuf,
    backup: Option<PathBuf>,
}

/// Moves the staged tree into place, recording enough information to restore
/// every touched file if marker stamping or a later import step fails.
#[cfg(not(unix))]
fn install(
    staged: &Path,
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<usize> {
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
            move_existing(&destination, root, rollback, undo)?;
            // Rename first (same filesystem, atomic); fall back to a copy
            // only when the staging dir landed on another filesystem. Other
            // errors must remain errors instead of widening the operation to
            // a path-based copy after a permission or destination race.
            if let Err(error) = std::fs::rename(&path, &destination) {
                if !is_cross_device_error(&error) {
                    return Err(error).with_context(|| format!("installing {}", rel.display()));
                }
                copy_into_place(&path, &destination)
                    .with_context(|| format!("installing {} across filesystems", rel.display()))?;
            }
            installed += 1;
        }
    }
    Ok(installed)
}

#[cfg(not(unix))]
fn move_existing(
    destination: &Path,
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            undo.push(UndoEntry {
                destination: destination.to_path_buf(),
                backup: None,
            });
            return Ok(());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("checking {}", destination.display()))
        }
    };
    if metadata.file_type().is_symlink() {
        bail!(
            "refusing to replace a destination symlink: {}",
            destination.display()
        );
    }
    if !metadata.is_file() {
        bail!(
            "destination is not a regular file: {}",
            destination.display()
        );
    }
    let rel = destination
        .strip_prefix(root)
        .context("destination escaped the install root")?;
    let backup = rollback.join(rel);
    if let Some(parent) = backup.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(destination, &backup).with_context(|| {
        format!(
            "moving {} to the import rollback directory",
            destination.display()
        )
    })?;
    undo.push(UndoEntry {
        destination: destination.to_path_buf(),
        backup: Some(backup),
    });
    Ok(())
}

#[cfg(not(unix))]
fn rollback_install(undo: &[UndoEntry], _root: &Path) -> Result<()> {
    for entry in undo.iter().rev() {
        match std::fs::symlink_metadata(&entry.destination) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                bail!(
                    "rollback destination is not a regular file: {}",
                    entry.destination.display()
                );
            }
            Ok(_) => std::fs::remove_file(&entry.destination)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("checking {}", entry.destination.display()))
            }
        }
        if let Some(backup) = &entry.backup {
            std::fs::rename(backup, &entry.destination).with_context(|| {
                format!(
                    "restoring {} from the import rollback directory",
                    entry.destination.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
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

#[cfg(not(unix))]
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

#[cfg(not(unix))]
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

#[cfg(unix)]
/// A destination basename held relative to an already-opened, no-follow
/// parent directory. Keeping the descriptor across the check and mutation is
/// what closes the parent-directory replacement window in force imports.
struct DestinationParent {
    directory: File,
    name: CString,
    display: PathBuf,
}

#[cfg(unix)]
fn open_directory(path: &Path) -> Result<File> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| anyhow::anyhow!("path contains NUL: {}", path.display()))?;
    let fd = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("opening directory {}", path.to_string_lossy()));
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn open_child_directory(parent: &File, name: &std::ffi::OsStr, create: bool) -> Result<File> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| anyhow::anyhow!("path component contains NUL"))?;
    let open = || unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    let mut fd = open();
    if fd < 0 && create && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        let rc = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if rc < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(error).with_context(|| "creating destination directory");
            }
        }
        fd = open();
    }
    if fd < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| "opening destination directory without following symlinks");
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(unix)]
fn destination_parent(root: &Path, destination: &Path) -> Result<DestinationParent> {
    let root_dir = open_directory(root)?;
    let parent = destination
        .parent()
        .context("destination has no parent directory")?;
    let relative = parent
        .strip_prefix(root)
        .context("destination parent escaped the install root")?;
    let mut directory = root_dir;
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            bail!("destination parent contains a non-normal path component");
        };
        directory = open_child_directory(&directory, name, true)?;
    }
    let name = destination
        .file_name()
        .context("destination has no file name")?;
    Ok(DestinationParent {
        directory,
        name: CString::new(name.as_bytes())
            .map_err(|_| anyhow::anyhow!("destination name contains NUL"))?,
        display: destination.to_path_buf(),
    })
}

#[cfg(unix)]
fn stat_destination(parent: &DestinationParent) -> Result<Option<libc::stat>> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    let rc = unsafe {
        libc::fstatat(
            parent.directory.as_raw_fd(),
            parent.name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        return Ok(Some(stat));
    }
    let error = std::io::Error::last_os_error();
    if error.kind() == std::io::ErrorKind::NotFound {
        Ok(None)
    } else {
        Err(error).with_context(|| format!("checking {}", parent.display.display()))
    }
}

#[cfg(unix)]
fn secure_move_existing(
    parent: &DestinationParent,
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<()> {
    let Some(stat) = stat_destination(parent)? else {
        undo.push(UndoEntry {
            destination: parent.display.clone(),
            backup: None,
        });
        return Ok(());
    };
    let mode = stat.st_mode as libc::mode_t & libc::S_IFMT as libc::mode_t;
    if mode == libc::S_IFLNK as libc::mode_t {
        bail!(
            "refusing to replace a destination symlink: {}",
            parent.display.display()
        );
    }
    if mode != libc::S_IFREG as libc::mode_t {
        bail!(
            "destination is not a regular file: {}",
            parent.display.display()
        );
    }
    let rel = parent
        .display
        .strip_prefix(root)
        .context("destination escaped the install root")?;
    let backup = rollback.join(rel);
    if let Some(backup_parent) = backup.parent() {
        std::fs::create_dir_all(backup_parent)?;
    }
    let backup_parent = open_directory(backup.parent().context("backup has no parent")?)?;
    let backup_name = CString::new(
        backup
            .file_name()
            .context("backup has no file name")?
            .as_bytes(),
    )
    .map_err(|_| anyhow::anyhow!("backup name contains NUL"))?;
    let rc = unsafe {
        libc::renameat(
            parent.directory.as_raw_fd(),
            parent.name.as_ptr(),
            backup_parent.as_raw_fd(),
            backup_name.as_ptr(),
        )
    };
    if rc < 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "moving {} to the import rollback directory",
                parent.display.display()
            )
        });
    }
    undo.push(UndoEntry {
        destination: parent.display.clone(),
        backup: Some(backup),
    });
    Ok(())
}

#[cfg(unix)]
fn install(
    staged: &Path,
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<usize> {
    let _root_dir = open_directory(root)?;
    let staged_metadata = std::fs::symlink_metadata(staged)?;
    if staged_metadata.file_type().is_symlink() || !staged_metadata.is_dir() {
        bail!("expected a real staging directory: {}", staged.display());
    }
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
                .context("staged path outside the install root")?;
            let destination = root.join(rel);
            let parent = destination_parent(root, &destination)?;
            secure_move_existing(&parent, root, rollback, undo)?;
            let source = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| anyhow::anyhow!("staged path contains NUL"))?;
            let rc = unsafe {
                libc::renameat(
                    libc::AT_FDCWD,
                    source.as_ptr(),
                    parent.directory.as_raw_fd(),
                    parent.name.as_ptr(),
                )
            };
            if rc < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::EXDEV) {
                    bail!("cross-device import install is not supported by secure directory operations");
                }
                return Err(error).with_context(|| format!("installing {}", rel.display()));
            }
            installed += 1;
        }
    }
    Ok(installed)
}

#[cfg(unix)]
fn prune_stale_core(
    stale: &[PathBuf],
    root: &Path,
    rollback: &Path,
    undo: &mut Vec<UndoEntry>,
) -> Result<usize> {
    for destination in stale {
        let parent = destination_parent(root, destination)?;
        secure_move_existing(&parent, root, rollback, undo)?;
    }
    Ok(stale.len())
}

#[cfg(unix)]
fn secure_write_marker(parent: &DestinationParent) -> Result<()> {
    let fd = unsafe {
        libc::openat(
            parent.directory.as_raw_fd(),
            parent.name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("writing {}", parent.display.display()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(b"1")?;
    file.sync_all().ok();
    Ok(())
}

#[cfg(unix)]
fn stamp_layout_markers(root: &Path, rollback: &Path, undo: &mut Vec<UndoEntry>) -> Result<()> {
    for marker in [".layout-v1", ".resource-layout-v1"] {
        let path = root.join(marker);
        let parent = destination_parent(root, &path)?;
        secure_move_existing(&parent, root, rollback, undo)?;
        secure_write_marker(&parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn rollback_install(undo: &[UndoEntry], root: &Path) -> Result<()> {
    for entry in undo.iter().rev() {
        let parent = destination_parent(root, &entry.destination)?;
        if let Some(stat) = stat_destination(&parent)? {
            let mode = stat.st_mode as libc::mode_t & libc::S_IFMT as libc::mode_t;
            if mode == libc::S_IFLNK as libc::mode_t || mode != libc::S_IFREG as libc::mode_t {
                bail!(
                    "rollback destination is not a regular file: {}",
                    entry.destination.display()
                );
            }
            let rc =
                unsafe { libc::unlinkat(parent.directory.as_raw_fd(), parent.name.as_ptr(), 0) };
            if rc < 0 {
                return Err(std::io::Error::last_os_error())
                    .with_context(|| format!("removing {}", entry.destination.display()));
            }
        }
        if let Some(backup) = &entry.backup {
            let source = CString::new(backup.as_os_str().as_bytes())
                .map_err(|_| anyhow::anyhow!("rollback path contains NUL"))?;
            let rc = unsafe {
                libc::renameat(
                    libc::AT_FDCWD,
                    source.as_ptr(),
                    parent.directory.as_raw_fd(),
                    parent.name.as_ptr(),
                )
            };
            if rc < 0 {
                return Err(std::io::Error::last_os_error()).with_context(|| {
                    format!(
                        "restoring {} from the import rollback directory",
                        entry.destination.display()
                    )
                });
            }
        }
    }
    Ok(())
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        // Win32 ERROR_NOT_SAME_DEVICE. Keep this local rather than importing
        // a Windows-only crate into the portable transfer module.
        error.raw_os_error() == Some(17)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = error;
        false
    }
}

/// Marks the restored tree as already using the current layout, so
/// `YunXiPaths::new` does not try to migrate it from a legacy one.
#[cfg(not(unix))]
fn stamp_layout_markers(root: &Path, rollback: &Path, undo: &mut Vec<UndoEntry>) -> Result<()> {
    for marker in [".layout-v1", ".resource-layout-v1"] {
        let path = root.join(marker);
        move_existing(&path, root, rollback, undo)?;
        std::fs::write(&path, "1").with_context(|| format!("writing {}", path.display()))?;
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

#[cfg(test)]
mod tests {
    use super::{
        is_cross_device_error, ArchiveBudget, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_UNCOMPRESSED_BYTES,
        MAX_MANIFEST_BYTES,
    };

    #[test]
    fn archive_budget_rejects_entry_and_expansion_limits() {
        let mut budget = ArchiveBudget::default();
        budget.entries = MAX_ARCHIVE_ENTRIES;
        assert!(budget.observe_entry(0).is_err());

        let mut budget = ArchiveBudget::default();
        budget.uncompressed_bytes = MAX_ARCHIVE_UNCOMPRESSED_BYTES;
        assert!(budget.observe_entry(1).is_err());
    }

    #[test]
    fn archive_budget_rejects_manifest_before_parsing() {
        assert!(ArchiveBudget::check_manifest_size(MAX_MANIFEST_BYTES).is_ok());
        assert!(ArchiveBudget::check_manifest_size(MAX_MANIFEST_BYTES + 1).is_err());
    }

    #[test]
    fn only_cross_device_errors_allow_copy_fallback() {
        #[cfg(unix)]
        assert!(is_cross_device_error(&std::io::Error::from_raw_os_error(
            libc::EXDEV,
        )));
        #[cfg(windows)]
        assert!(is_cross_device_error(&std::io::Error::from_raw_os_error(
            17
        )));
        assert!(!is_cross_device_error(&std::io::Error::from_raw_os_error(
            13
        )));
        assert!(!is_cross_device_error(&std::io::Error::from_raw_os_error(
            2
        )));
    }
}
