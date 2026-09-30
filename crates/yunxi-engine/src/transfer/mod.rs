//! Moving a YunXi installation between machines: `yunxi export` / `yunxi import`.

pub mod export;
pub mod fixups;
pub mod import;
pub mod manifest;
pub mod registry;

#[cfg(test)]
pub(crate) mod tests {
    use super::registry::{is_backup_name, unit_for, Tier, IGNORED_SUFFIXES, UNITS};
    use std::collections::BTreeSet;
    use std::path::Path;
    use yunxi_base::paths::YunXiPaths;

    /// A YunXiPaths rooted at `root`, mirroring the real layout.
    pub(crate) fn test_paths(root: &Path) -> YunXiPaths {
        YunXiPaths {
            root_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            config_file: root.join("config/config.jsonc"),
            skills_dir: root.join("data/skills"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            pictures_dir: root.join("data/pictures"),
            fish_hook_file: root.join("fish/yunxi.fish"),
            bash_hook_file: root.join("config/shell/bash-hook.sh"),
            zsh_hook_file: root.join("config/shell/zsh-hook.zsh"),
            scripts_dir: root.join("data/scripts"),
            system_scripts_dir: root.join("system-scripts"),
        }
    }

    /// Everything YunXi writes under `YUNXI_HOME` must be classified.
    ///
    /// This is the guard that keeps export from rotting: add a feature that
    /// writes somewhere new, forget to register it, and this fails with a
    /// pointer to the registry — instead of the user discovering the gap on
    /// the new machine, after the old one is gone.
    /// Builds a populated home: config with a secret, a database holding a
    /// row, a user resource, and things that must not travel.
    fn populated_home(root: &Path) -> YunXiPaths {
        let paths = test_paths(root);
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::create_dir_all(&paths.state_dir).unwrap();
        std::fs::create_dir_all(paths.data_dir.join("prompts")).unwrap();
        std::fs::create_dir_all(root.join("cache/logs")).unwrap();
        std::fs::write(
            &paths.config_file,
            r#"{ "providers": [ { "id": "p", "api_key": "sk-secret" } ] }"#,
        )
        .unwrap();
        std::fs::write(paths.data_dir.join("prompts/system-prompt.md"), "persona").unwrap();
        std::fs::write(root.join("cache/logs/yunxi.log"), "noise").unwrap();
        std::fs::write(paths.state_dir.join("conversation.db.bak"), "old").unwrap();

        let conn = rusqlite::Connection::open(paths.state_dir.join("conversation.db")).unwrap();
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA user_version=17;
             CREATE TABLE sessions (session_id TEXT PRIMARY KEY, workspace TEXT);
             CREATE TABLE turns (turn_id TEXT PRIMARY KEY, workspace TEXT,
                                 tool_footprint TEXT, owner_pid INTEGER);
             CREATE TABLE queued_prompts (prompt_id TEXT PRIMARY KEY, owner_pid INTEGER);
             INSERT INTO sessions VALUES ('s1', '/gone/from/this/machine');",
        )
        .unwrap();
        // Leave the write sitting in the WAL: a plain file copy would miss it.
        std::mem::forget(conn);
        paths
    }

    #[test]
    fn an_export_round_trips_into_an_empty_home() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("yunxi-export.tar.gz");

        let report =
            super::export::export(&paths, &archive, &super::export::ExportOptions::default())
                .unwrap();
        assert!(report.entries > 0);
        assert!(report.secrets_included);

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        std::fs::create_dir_all(&restored.config_dir).unwrap();
        std::fs::remove_dir_all(&restored.config_dir).unwrap();
        let outcome = super::import::import(
            &restored,
            &archive,
            &super::import::ImportOptions::default(),
        )
        .unwrap();
        assert!(outcome.restored > 0);
        assert!(outcome.unknown_units.is_empty());

        // Config and user resources came across, secrets intact.
        let config = std::fs::read_to_string(&restored.config_file).unwrap();
        assert!(config.contains("sk-secret"));
        assert_eq!(
            std::fs::read_to_string(restored.data_dir.join("prompts/system-prompt.md")).unwrap(),
            "persona"
        );

        // The database came through SQLite, so the row that was still in the
        // WAL is present — and its dead workspace was cleared on the way in.
        let conn = rusqlite::Connection::open(restored.state_dir.join("conversation.db")).unwrap();
        let workspace: Option<String> = conn
            .query_row(
                "SELECT workspace FROM sessions WHERE session_id='s1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(workspace.is_none(), "stale workspace should be cleared");
        assert_eq!(outcome.cleared_workspaces, 1);

        // Machine-specific noise stayed behind.
        assert!(!target.path().join("cache/logs/yunxi.log").exists());
        assert!(!restored.state_dir.join("conversation.db.bak").exists());
        // The layout markers are stamped so the tree is not re-migrated.
        assert!(target.path().join(".layout-v1").exists());
    }

    #[test]
    fn importing_over_existing_data_is_refused_without_force() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("yunxi-export.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        // The source home is itself non-empty, so importing onto it must stop.
        let error =
            super::import::import(&paths, &archive, &super::import::ImportOptions::default())
                .unwrap_err()
                .to_string();
        assert!(error.contains("--force"), "got: {error}");

        let outcome = super::import::import(
            &paths,
            &archive,
            &super::import::ImportOptions { force: true },
        )
        .unwrap();
        // Overwriting is only allowed after the current state is safe.
        let backup = outcome.backup.expect("--force must back up first");
        assert!(backup.exists());
    }

    #[test]
    fn force_import_prunes_only_stale_core_files() {
        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("yunxi-export.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let target_paths = populated_home(target.path());
        let stale_core = target_paths.data_dir.join("prompts/obsolete.md");
        std::fs::write(&stale_core, "remove me").unwrap();
        let heavy = target_paths.data_dir.join("kb/semantic_index.db");
        std::fs::create_dir_all(heavy.parent().unwrap()).unwrap();
        let heavy_conn = rusqlite::Connection::open(&heavy).unwrap();
        heavy_conn
            .execute_batch("CREATE TABLE keep_me (value TEXT);")
            .unwrap();
        let unknown = target_paths.data_dir.join("future/keep.db");
        std::fs::create_dir_all(unknown.parent().unwrap()).unwrap();
        std::fs::write(&unknown, "keep unknown").unwrap();

        let outcome = super::import::import(
            &target_paths,
            &archive,
            &super::import::ImportOptions { force: true },
        )
        .unwrap();

        assert!(!stale_core.exists(), "stale Core file must be pruned");
        assert_eq!(outcome.removed_stale, 1);
        let heavy_conn = rusqlite::Connection::open(&heavy).unwrap();
        assert!(
            heavy_conn
                .query_row(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='keep_me'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .is_ok(),
            "Heavy index must be preserved"
        );
        assert_eq!(std::fs::read_to_string(&unknown).unwrap(), "keep unknown");
    }

    #[test]
    fn force_import_backs_up_a_stale_core_only_home_before_pruning() {
        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("yunxi-export.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let target_paths = test_paths(target.path());
        let stale_core = target_paths.data_dir.join("prompts/obsolete.md");
        std::fs::create_dir_all(stale_core.parent().unwrap()).unwrap();
        std::fs::write(&stale_core, "remove me").unwrap();

        let outcome = super::import::import(
            &target_paths,
            &archive,
            &super::import::ImportOptions { force: true },
        )
        .unwrap();

        assert!(outcome.backup.as_ref().is_some_and(|path| path.exists()));
        assert!(!stale_core.exists());
        assert_eq!(outcome.removed_stale, 1);
    }

    #[test]
    fn legacy_manifest_without_coverage_is_merge_only() {
        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("yunxi-export.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();
        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().unwrap().to_string_lossy() == "manifest.json")
            .map(|mut entry| serde_json::from_reader(&mut entry).unwrap())
            .unwrap();
        manifest.included_units = None;
        let legacy = out.path().join("legacy.tar.gz");
        rewrite_manifest(&archive, &legacy, &manifest);

        let target = tempfile::tempdir().unwrap();
        let target_paths = populated_home(target.path());
        let stale_core = target_paths.data_dir.join("prompts/obsolete.md");
        std::fs::write(&stale_core, "keep me").unwrap();
        super::import::import(
            &target_paths,
            &legacy,
            &super::import::ImportOptions { force: true },
        )
        .unwrap();
        assert!(
            stale_core.exists(),
            "legacy archive must not prune Core files"
        );
    }

    #[test]
    fn archive_inside_core_tree_is_not_pruned() {
        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let source_archive = out.path().join("source.tar.gz");
        super::export::export(
            &source_paths,
            &source_archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let target_paths = test_paths(target.path());
        let archive = target_paths.data_dir.join("prompts/incoming.tar.gz");
        std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
        std::fs::copy(&source_archive, &archive).unwrap();

        super::import::import(
            &target_paths,
            &archive,
            &super::import::ImportOptions { force: true },
        )
        .unwrap();
        assert!(archive.exists(), "the input archive must be preserved");
    }

    #[test]
    fn no_secrets_blanks_credentials_without_dropping_the_config() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("redacted.tar.gz");
        let report = super::export::export(
            &paths,
            &archive,
            &super::export::ExportOptions {
                no_secrets: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!report.secrets_included);

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        super::import::import(
            &restored,
            &archive,
            &super::import::ImportOptions::default(),
        )
        .unwrap();
        let config = std::fs::read_to_string(&restored.config_file).unwrap();
        assert!(!config.contains("sk-secret"), "the key must not travel");
        assert!(
            config.contains("providers"),
            "the config itself must survive"
        );
    }

    #[test]
    fn a_newer_archive_is_refused_rather_than_downgraded() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("future.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        // Rewrite the manifest to claim a schema this build cannot open.
        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();
        manifest
            .schema_versions
            .insert("state.conversation".to_string(), 9_999);
        let doctored = out.path().join("doctored.tar.gz");
        rewrite_manifest(&archive, &doctored, &manifest);

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        let error = super::import::import(
            &restored,
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("9999"), "got: {error}");
    }

    #[test]
    fn an_unknown_archive_format_is_refused_before_import() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("format.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();
        manifest.format_version = super::manifest::MANIFEST_FORMAT_VERSION + 1;
        let doctored = out.path().join("future-format.tar.gz");
        rewrite_manifest(&archive, &doctored, &manifest);

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        let error = super::import::import(
            &restored,
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("2 != 1"), "got: {error}");
        assert!(
            !restored.config_file.exists(),
            "validation must precede writes"
        );
    }

    #[test]
    fn manifest_size_and_hash_must_match_archive_bytes() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();

        manifest.entries[0].size += 1;
        let wrong_size = out.path().join("wrong-size.tar.gz");
        rewrite_manifest(&archive, &wrong_size, &manifest);
        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &wrong_size,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("size mismatch"), "got: {error}");

        manifest.entries[0].size -= 1;
        manifest.entries[0].blake3 = "0".repeat(64);
        let wrong_hash = out.path().join("wrong-hash.tar.gz");
        rewrite_manifest(&archive, &wrong_hash, &manifest);
        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &wrong_hash,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("blake3 mismatch"), "got: {error}");
    }

    #[test]
    fn manifest_paths_are_portable_and_entries_must_exist() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let original: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();

        for (index, path) in [
            "../outside",
            "home\\secret",
            "C:\\Windows\\x",
            "/etc/passwd",
        ]
        .into_iter()
        .enumerate()
        {
            let mut manifest = original.clone();
            manifest.entries[0].path = path.trim_start_matches("home/").to_string();
            let doctored = out.path().join(format!("path-{index}.tar.gz"));
            rewrite_manifest(&archive, &doctored, &manifest);
            let target = tempfile::tempdir().unwrap();
            let error = super::import::import(
                &test_paths(target.path()),
                &doctored,
                &super::import::ImportOptions::default(),
            )
            .unwrap_err()
            .to_string();
            assert!(
                error.contains("invalid manifest path") || error.contains("missing"),
                "got: {error}"
            );
        }

        let mut manifest = original;
        manifest.entries[0].path = "missing/entry.txt".to_string();
        let doctored = out.path().join("missing.tar.gz");
        rewrite_manifest(&archive, &doctored, &manifest);
        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("missing from archive"), "got: {error}");
    }

    #[test]
    fn duplicate_or_extra_archive_entries_are_refused() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let first_path = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() != "manifest.json")
                    .unwrap_or(false)
            })
            .unwrap()
            .path()
            .unwrap()
            .to_path_buf();

        let duplicate = out.path().join("duplicate.tar.gz");
        copy_archive_with_entry(&archive, &duplicate, &first_path, b"duplicate");
        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &duplicate,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("duplicate path"), "got: {error}");

        let extra = out.path().join("extra.tar.gz");
        copy_archive_with_entry(&archive, &extra, Path::new("home/extra.txt"), b"extra");
        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &extra,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("undeclared path"), "got: {error}");
    }

    #[test]
    fn known_manifest_paths_cannot_claim_another_unit() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();
        manifest.entries[0].unit = "not-the-registered-unit".to_string();
        let doctored = out.path().join("wrong-unit.tar.gz");
        rewrite_manifest(&archive, &doctored, &manifest);

        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("unit mismatch"), "got: {error}");
    }

    #[test]
    fn never_units_cannot_be_imported_from_a_handcrafted_manifest() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let raw = std::fs::read(&archive).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        let mut manifest: super::manifest::Manifest = tar
            .entries()
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| {
                entry
                    .path()
                    .map(|path| path.to_string_lossy() == "manifest.json")
                    .unwrap_or(false)
            })
            .map(|entry| serde_json::from_reader(entry).unwrap())
            .unwrap();
        manifest.entries[0].path = ".home-layout-v1.journal.json".to_string();
        manifest.entries[0].unit = "layout.home_journal".to_string();
        let doctored = out.path().join("never-unit.tar.gz");
        rewrite_manifest(&archive, &doctored, &manifest);

        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("non-transferable"), "got: {error}");
    }

    #[test]
    fn new_home_layout_conversation_is_occupied_and_fixed_up() {
        let target = tempfile::tempdir().unwrap();
        let paths = test_paths(target.path());
        std::fs::write(target.path().join(".home-layout-v1"), "tester").unwrap();
        let home = target.path().join("home/tester");
        std::fs::create_dir_all(&home).unwrap();
        let db_path = home.join("conversation.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, workspace TEXT);
                 CREATE TABLE turns (turn_id TEXT PRIMARY KEY, workspace TEXT,
                                     tool_footprint TEXT, owner_pid INTEGER);
                 CREATE TABLE queued_prompts (prompt_id TEXT PRIMARY KEY, owner_pid INTEGER);
                 INSERT INTO sessions VALUES ('s1', '/missing/on/target');
                 INSERT INTO turns VALUES ('t1', '/missing/on/target', 'old-footprint', 42);
                 INSERT INTO queued_prompts VALUES ('q1', 42);",
            )
            .unwrap();
        }

        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();
        let error =
            super::import::import(&paths, &archive, &super::import::ImportOptions::default())
                .unwrap_err()
                .to_string();
        assert!(
            error.contains("会话历史") || error.contains("conversation history"),
            "got: {error}"
        );

        assert_eq!(super::fixups::apply(&paths).unwrap(), 1);
        let conn = rusqlite::Connection::open(db_path).unwrap();
        let workspace: Option<String> = conn
            .query_row("SELECT workspace FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert!(workspace.is_none());
        let footprint: Option<String> = conn
            .query_row("SELECT tool_footprint FROM turns", [], |row| row.get(0))
            .unwrap();
        assert!(footprint.is_none());
    }

    #[test]
    fn imported_home_conversation_is_fixed_before_install() {
        let source = tempfile::tempdir().unwrap();
        let source_paths = test_paths(source.path());
        std::fs::create_dir_all(&source_paths.config_dir).unwrap();
        std::fs::write(&source_paths.config_file, "{}").unwrap();
        std::fs::write(source.path().join(".home-layout-v1"), "tester").unwrap();
        let home = source.path().join("home/tester");
        std::fs::create_dir_all(&home).unwrap();
        let db_path = home.join("conversation.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, workspace TEXT);
                 CREATE TABLE turns (turn_id TEXT PRIMARY KEY, workspace TEXT,
                                     tool_footprint TEXT, owner_pid INTEGER);
                 CREATE TABLE queued_prompts (prompt_id TEXT PRIMARY KEY, owner_pid INTEGER);
                 INSERT INTO sessions VALUES ('s1', '/missing/on/old-machine');
                 INSERT INTO turns VALUES ('t1', '/missing/on/old-machine', 'old-footprint', 42);
                 INSERT INTO queued_prompts VALUES ('q1', 42);",
            )
            .unwrap();
        }

        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("home-layout.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        let outcome = super::import::import(
            &restored,
            &archive,
            &super::import::ImportOptions::default(),
        )
        .unwrap();
        assert_eq!(outcome.cleared_workspaces, 1);
        let conn =
            rusqlite::Connection::open(target.path().join("home/tester/conversation.db")).unwrap();
        let workspace: Option<String> = conn
            .query_row("SELECT workspace FROM sessions", [], |row| row.get(0))
            .unwrap();
        assert!(workspace.is_none());
        let footprint: Option<String> = conn
            .query_row("SELECT tool_footprint FROM turns", [], |row| row.get(0))
            .unwrap();
        assert!(footprint.is_none());
        let owner: i64 = conn
            .query_row("SELECT owner_pid FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(owner, 0);
    }

    #[cfg(unix)]
    #[test]
    fn marker_failure_rolls_back_every_installed_file() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), target.path().join(".layout-v1")).unwrap();
        let error = super::import::import(
            &restored,
            &archive,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("symlink"), "got: {error}");
        assert!(
            !restored.config_file.exists(),
            "live files must be rolled back"
        );
        assert!(!outside.path().join("config.jsonc").exists());
        assert!(std::fs::symlink_metadata(target.path().join(".layout-v1")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn stale_core_prune_rolls_back_when_marker_stamping_fails() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let source_paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(
            &source_paths,
            &archive,
            &super::export::ExportOptions::default(),
        )
        .unwrap();

        let target = tempfile::tempdir().unwrap();
        let restored = test_paths(target.path());
        let stale = restored.data_dir.join("prompts/obsolete.md");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "keep after rollback").unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), target.path().join(".layout-v1")).unwrap();

        let error = super::import::import(
            &restored,
            &archive,
            &super::import::ImportOptions { force: true },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("symlink"), "got: {error}");
        assert_eq!(
            std::fs::read_to_string(&stale).unwrap(),
            "keep after rollback"
        );
        assert!(
            !restored.config_file.exists(),
            "installed files must be rolled back"
        );
        assert!(!outside.path().join("config.jsonc").exists());
        assert!(std::fs::symlink_metadata(target.path().join(".layout-v1")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_symlinked_sources() {
        use std::os::unix::fs::symlink;

        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        symlink(
            paths.data_dir.join("prompts/system-prompt.md"),
            paths.data_dir.join("prompts/alias.md"),
        )
        .unwrap();
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("symlink.tar.gz");
        let error =
            super::export::export(&paths, &archive, &super::export::ExportOptions::default())
                .unwrap_err()
                .to_string();
        assert!(error.contains("symlink"), "got: {error}");
    }

    #[cfg(unix)]
    #[test]
    fn import_refuses_non_regular_archive_entries() {
        let source = tempfile::tempdir().unwrap();
        let paths = populated_home(source.path());
        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("source.tar.gz");
        super::export::export(&paths, &archive, &super::export::ExportOptions::default()).unwrap();

        let doctored = out.path().join("symlink.tar.gz");
        let file = std::fs::File::create(&doctored).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let raw = std::fs::read(&archive).unwrap();
        let mut source = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        for entry in source.entries().unwrap().filter_map(Result::ok) {
            let mut header = entry.header().clone();
            header.set_cksum();
            builder.append(&header, entry).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::symlink());
        header.set_link_name("../outside").unwrap();
        header.set_size(0);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, "home/escape", std::io::empty())
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let target = tempfile::tempdir().unwrap();
        let error = super::import::import(
            &test_paths(target.path()),
            &doctored,
            &super::import::ImportOptions::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("regular file"), "got: {error}");
        assert!(!test_paths(target.path()).config_file.exists());
    }

    /// Copies an archive, replacing only its manifest.
    fn rewrite_manifest(from: &Path, to: &Path, manifest: &super::manifest::Manifest) {
        let file = std::fs::File::create(to).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let bytes = serde_json::to_vec_pretty(manifest).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder
            .append_data(&mut header, "manifest.json", bytes.as_slice())
            .unwrap();

        let raw = std::fs::read(from).unwrap();
        let mut source = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        for entry in source.entries().unwrap().filter_map(Result::ok) {
            let path = entry.path().unwrap().to_path_buf();
            if path.to_string_lossy() == "manifest.json" {
                continue;
            }
            let mut header = entry.header().clone();
            header.set_cksum();
            builder.append(&header, entry).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    /// Copies an archive and appends one regular entry without changing its
    /// manifest. Used to prove that the importer checks the exact entry set.
    fn copy_archive_with_entry(from: &Path, to: &Path, path: &Path, bytes: &[u8]) {
        let file = std::fs::File::create(to).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let raw = std::fs::read(from).unwrap();
        let mut source = tar::Archive::new(flate2::read::GzDecoder::new(&raw[..]));
        for entry in source.entries().unwrap().filter_map(Result::ok) {
            let mut header = entry.header().clone();
            header.set_cksum();
            builder.append(&header, entry).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        builder.append_data(&mut header, path, bytes).unwrap();
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn every_data_location_is_classified() {
        // A representative populated home. Kept literal rather than derived
        // from a live run so the expectations are reviewable in the diff.
        let observed = [
            ".layout-v1",
            ".resource-layout-v1",
            "cache/logs/yunxi.2026-08-08.log",
            "cache/models_cache.json",
            "cache/jobs/abc123.log",
            "cache/clipboard_images/1.png",
            "cache/platform_images/onebot/x.jpg",
            "cache/default-kb/update-source",
            "config/config.jsonc",
            "config/webui-theme.css",
            "config/shell/bash-hook.sh",
            "data/prompts/system-prompt.md",
            "data/identities/user-identity.md",
            "data/persona-avatars/default.png",
            "data/skills/my-skill/SKILL.md",
            "data/scripts/index.json",
            "data/pictures/out.png",
            "data/documents/report.md",
            "data/memes/library/a.gif",
            "data/personas/default/memory/memory.db",
            "data/kb/files/games/a.md",
            "data/kb/kb_meta.db",
            "data/kb/semantic_index.db",
            "data/default-kb/state.json",
            "data/platforms/onebot/message_history/history.sqlite3",
            "data/platforms/onebot/real_context/state.json",
            "data/artifacts/sess_1/page.html",
            "state/conversation.db",
            "state/conversation.jsonl",
            "state/usage.db",
            "state/usage.json",
            "state/usage-history.jsonl",
            "state/profile.md",
            "state/alarms.json",
            "state/thinking-variants.json",
            "state/repl-history.jsonl",
            "state/skill-drafts/draft.md",
            "state/personas/default/memory/evicted_context.db",
            "state/prompt-fingerprints/abc.sha256",
            "state/prompt.sha256",
            "state/aur-review-state.json",
            "state/arch_news_last_seen.json",
            "state/daemon-launch.json",
            "state/web-passwords/password-1234-ab",
            "state/yunxi/core.sock",
            "state/conversation.db.bak",
            "data/shared/share_1/page.html",
            "data/personas/default/meme.db",
            "data/personas/default/persona.toml",
            ".home-layout-v1",
            ".home-layout-v1.journal.json",
            "personas/default/memory/memory.db",
            "personas/default/meme.db",
            "personas/default/persona.toml",
            "extensions/skills/my-skill/SKILL.md",
            "extensions/scripts/index.json",
            "home/tester/profile.md",
            "home/tester/identities/team/user.md",
            "home/tester/conversation.db",
            "home/tester/pictures/out.png",
            "home/tester/documents/report.md",
            "home/tester/ledger/ledger.db",
            "home/tester/shares/share_1/page.html",
            "home/tester/artifacts/sess_1/page.html",
            "home/alice/profile.md",
        ];

        let unclassified: Vec<&str> = observed
            .iter()
            .copied()
            .filter(|rel| unit_for(rel).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "unclassified paths under YUNXI_HOME: {unclassified:?}\n\
             Register each in src/transfer/registry.rs `UNITS` — or mark it \
             Tier::Never with the reason it must not travel."
        );
    }

    #[test]
    fn home_and_persona_layout_wildcards_resolve_to_their_units() {
        let cases = [
            ("state/usage.db", "state.usage_db"),
            ("state/usage.json", "state.usage"),
            ("state/usage-history.jsonl", "state.usage_history"),
            // The data/ layout is the original persona resource layout.
            (
                "data/personas/alice/memory/memory.db",
                "data.persona_memory",
            ),
            ("data/personas/alice/meme.db", "data.persona_memes"),
            ("data/personas/alice/persona.toml", "data.persona_manifest"),
            // The personas/ layout is the shared home layout introduced later.
            ("personas/alice/memory/memory.db", "personas.memory"),
            ("personas/alice/meme.db", "personas.memes"),
            ("personas/alice/persona.toml", "personas.manifest"),
            // Owner-scoped home entries use one wildcard segment as well.
            ("home/alice/profile.md", "home.profile"),
            ("home/alice/identities/team/user.md", "home.identities"),
            ("home/alice/conversation.db", "home.conversation"),
            ("home/alice/pictures/render/output.png", "home.pictures"),
            ("home/alice/documents/reports/summary.md", "home.documents"),
            ("home/alice/ledger/ledger.db", "home.ledger"),
            ("home/alice/shares/public/index.html", "home.shares"),
            ("home/alice/artifacts/session/page.html", "home.artifacts"),
        ];

        for (rel, expected_id) in cases {
            assert_eq!(
                unit_for(rel).map(|unit| unit.id),
                Some(expected_id),
                "{rel}"
            );
        }
    }

    #[test]
    fn an_unregistered_location_is_reported() {
        // The guard above is only worth having if it actually catches things.
        assert!(unit_for("data/brand-new-feature/store.db").is_none());
        assert!(unit_for("state/some-future-file.json").is_none());
    }

    #[test]
    fn unit_ids_and_paths_are_unique() {
        let ids: BTreeSet<&str> = UNITS.iter().map(|unit| unit.id).collect();
        assert_eq!(ids.len(), UNITS.len(), "duplicate DataUnit id");
        let rels: BTreeSet<&str> = UNITS.iter().map(|unit| unit.rel).collect();
        assert_eq!(rels.len(), UNITS.len(), "duplicate DataUnit rel");
    }

    #[test]
    fn never_units_state_a_reason() {
        for unit in UNITS.iter().filter(|unit| unit.tier == Tier::Never) {
            assert!(
                unit.why.len() > 20,
                "{}: Tier::Never needs a real reason, not `{}`",
                unit.id,
                unit.why
            );
        }
    }

    #[test]
    fn tier_switches_select_the_expected_units() {
        let ids = |all: bool, index: bool, platforms: bool| -> BTreeSet<&str> {
            UNITS
                .iter()
                .filter(|unit| unit.included(all, index, platforms))
                .map(|unit| unit.id)
                .collect()
        };

        let default = ids(false, false, false);
        assert!(default.contains("state.conversation"));
        assert!(default.contains("kb.files"));
        // The 143MB derived index and the account-bound platform history stay
        // out unless asked for.
        assert!(!default.contains("kb.semantic_index"));
        assert!(!default.contains("platform.message_history"));

        assert!(ids(false, true, false).contains("kb.semantic_index"));
        assert!(ids(false, false, true).contains("platform.message_history"));

        let all = ids(true, true, true);
        assert!(all.contains("kb.semantic_index"));
        assert!(all.contains("platform.message_history"));
        // No switch may ever pull in a Never unit.
        for unit in UNITS.iter().filter(|unit| unit.tier == Tier::Never) {
            assert!(!all.contains(unit.id), "{} must never be exported", unit.id);
        }
    }

    #[test]
    fn tier_matrix_covers_every_registered_unit() {
        let cases = [
            ("default", false, false, false, 44, &[Tier::Core][..]),
            (
                "index",
                false,
                true,
                false,
                45,
                &[Tier::Core, Tier::Heavy][..],
            ),
            (
                "platforms",
                false,
                false,
                true,
                46,
                &[Tier::Core, Tier::Platform][..],
            ),
            (
                "all",
                true,
                false,
                false,
                47,
                &[Tier::Core, Tier::Heavy, Tier::Platform][..],
            ),
        ];

        assert_eq!(UNITS.len(), 61);
        for (label, all, index, platforms, expected_count, included_tiers) in cases {
            let mut selected = 0;
            for unit in UNITS {
                let expected = included_tiers.contains(&unit.tier);
                assert_eq!(
                    unit.included(all, index, platforms),
                    expected,
                    "{label}: unexpected inclusion for {} ({:?})",
                    unit.id,
                    unit.tier
                );
                selected += usize::from(expected);
            }
            assert_eq!(selected, expected_count, "{label}: selected count");
        }

        let never = UNITS
            .iter()
            .filter(|unit| unit.tier == Tier::Never)
            .collect::<Vec<_>>();
        assert_eq!(never.len(), 14);
        for unit in never {
            assert!(
                !unit.included(true, true, true),
                "{} became exportable",
                unit.id
            );
        }
    }

    #[test]
    fn file_and_sqlite_units_do_not_claim_descendants() {
        for rel in [
            "home/alice/profile.md/child",
            "home/alice/conversation.db/child",
            "data/personas/alice/memory/memory.db/child",
            "personas/alice/meme.db/child",
            "state/personas/alice/memory/evicted_context.db/child",
        ] {
            assert!(unit_for(rel).is_none(), "non-directory unit claimed {rel}");
        }
        assert_eq!(
            unit_for("data/kb/files/nested/source.md").map(|unit| unit.id),
            Some("kb.files")
        );
        assert_eq!(
            unit_for("data/platforms/onebot/real_context/nested/state.json").map(|unit| unit.id),
            Some("platform.plugin_data")
        );
    }

    #[test]
    fn machine_specific_paths_resolve_to_never() {
        for rel in [
            "cache/logs/yunxi.log",
            "cache/jobs/abc.log",
            "state/daemon-launch.json",
            "state/web-passwords/password-1-a",
            "state/yunxi/core.sock",
            "config/shell/bash-hook.sh",
            "data/artifacts/sess_1/page.html",
            "state/conversation.db.bak",
        ] {
            let unit = unit_for(rel).unwrap_or_else(|| panic!("{rel} unclassified"));
            assert_eq!(unit.tier, Tier::Never, "{rel} resolved to {}", unit.id);
        }
    }

    #[test]
    fn sqlite_sidecars_and_backups_are_skipped() {
        for name in ["conversation.db-wal", "conversation.db-shm", "core.lock"] {
            assert!(
                IGNORED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)),
                "{name} should be skipped as a sidecar"
            );
        }
        assert!(is_backup_name("config.jsonc.bak-20260802-011956"));
        assert!(is_backup_name("conversation.db.bak"));
        assert!(!is_backup_name("config.jsonc"));
        // Sanity: the helper is about names, not whole paths.
        assert!(Path::new("config.jsonc").file_name().is_some());
    }
}
