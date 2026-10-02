//! Rewriting the parts of a restored database that described the old machine.
//!
//! Blobs travel fine — images and attachments live inside the database. What
//! does not travel are absolute paths and process ids: they describe a machine
//! that is not this one, and left alone they either silently do nothing or
//! point work at directories that do not exist here.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, Transaction};
use serde_json::Value;
use std::path::{Path, PathBuf};
use yunxi_base::paths::YunXiPaths;

/// Applies every machine-specific rewrite to the restored state.
/// Returns how many session workspaces had to be cleared.
pub fn apply(paths: &YunXiPaths) -> Result<usize> {
    let databases = conversation_db_paths(paths)?;
    apply_database_paths(&databases)
}

/// Applies fixups to an explicit set of restored databases. Import uses this
/// on the staging tree so a malformed database cannot partially mutate the
/// live installation.
pub(crate) fn apply_database_paths(databases: &[PathBuf]) -> Result<usize> {
    let mut cleared_total = 0usize;
    for database in databases {
        let metadata = match std::fs::symlink_metadata(database) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("checking {}", database.display()))
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            anyhow::bail!(
                "conversation database is not a regular file: {}",
                database.display()
            );
        }
        cleared_total += apply_database(database)?;
    }
    Ok(cleared_total)
}

/// Carry deletion tombstones from the current installation into an imported
/// memory snapshot before that snapshot is installed.
///
/// Archives made before the tombstone migration do not have the table.  A
/// complete YunXi memory database gets the table created on the staged copy;
/// malformed/partial SQLite files are left alone so the existing import
/// validation and rollback behavior remains unchanged.
pub(crate) fn apply_memory_tombstones(pairs: &[(PathBuf, PathBuf)]) -> Result<usize> {
    let mut removed = 0usize;
    for (live, staged) in pairs {
        let live_tombstones = match read_tombstones(live)? {
            Some(rows) => rows,
            None => continue,
        };
        if live_tombstones.is_empty() {
            continue;
        }
        let mut staged_conn = Connection::open(staged)
            .with_context(|| format!("opening staged memory database {}", staged.display()))?;
        if !has_table(&staged_conn, "facts")? || !has_table(&staged_conn, "episodes")? {
            // A hand-authored or malformed database is not a memory snapshot
            // that this migration can safely rewrite.
            continue;
        }
        let tx = staged_conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS memory_tombstones (
                kind TEXT NOT NULL CHECK (kind IN ('fact', 'episode')),
                id INTEGER NOT NULL,
                deleted_at TEXT NOT NULL,
                PRIMARY KEY (kind, id)
            )",
        )?;
        for (kind, id, deleted_at) in live_tombstones {
            tx.execute(
                "INSERT OR IGNORE INTO memory_tombstones (kind, id, deleted_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![kind, id, deleted_at],
            )?;
        }

        if has_table_tx(&tx, "memory_embeddings")? {
            removed += tx.execute(
                "DELETE FROM memory_embeddings
                  WHERE EXISTS (
                      SELECT 1 FROM memory_tombstones t
                       WHERE t.kind = memory_embeddings.kind AND t.id = memory_embeddings.id
                  )",
                [],
            )?;
        }
        if has_table_tx(&tx, "memory_revisions")? {
            removed += tx.execute(
                "DELETE FROM memory_revisions
                  WHERE memory_id IN (
                      SELECT id FROM memory_tombstones WHERE kind='fact'
                  )",
                [],
            )?;
        }
        removed += tx.execute(
            "DELETE FROM facts
              WHERE id IN (SELECT id FROM memory_tombstones WHERE kind='fact')",
            [],
        )?;
        removed += tx.execute(
            "DELETE FROM episodes
              WHERE id IN (SELECT id FROM memory_tombstones WHERE kind='episode')",
            [],
        )?;
        scrub_episode_sources(&tx)?;
        tx.commit()?;
    }
    Ok(removed)
}

fn read_tombstones(path: &Path) -> Result<Option<Vec<(String, i64, String)>>> {
    if !path.is_file() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("opening live memory database {}", path.display()))?;
    if !has_table(&conn, "memory_tombstones")? {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT kind, id, deleted_at FROM memory_tombstones
         WHERE kind IN ('fact', 'episode') ORDER BY kind, id",
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    Ok(Some(rows.collect::<std::result::Result<Vec<_>, _>>()?))
}

fn has_table(conn: &Connection, table: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1
         )",
        [table],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn has_table_tx(tx: &Transaction<'_>, table: &str) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1
         )",
        [table],
        |row| row.get::<_, i64>(0),
    )? != 0)
}

fn has_column_tx(tx: &Transaction<'_>, table: &str, column: &str) -> Result<bool> {
    let mut stmt = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    Ok(rows
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .any(|name| name == column))
}

/// Remove references to deleted episodes from the staged rows.  This keeps
/// imported provenance from pointing at a memory that the current install has
/// explicitly removed, while leaving malformed JSON untouched for later
/// diagnostics instead of failing an otherwise valid archive.
fn scrub_episode_sources(tx: &Transaction<'_>) -> Result<()> {
    for table in ["facts", "episodes", "memory_revisions"] {
        if !has_table_tx(tx, table)? || !has_column_tx(tx, table, "source_episode_ids")? {
            continue;
        }
        let rows = {
            let mut stmt = tx.prepare(&format!(
                "SELECT id, source_episode_ids FROM {table}
                 WHERE source_episode_ids IS NOT NULL AND source_episode_ids != '[]'"
            ))?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (row_id, raw) in rows {
            let Ok(ids) = serde_json::from_str::<Vec<i64>>(&raw) else {
                continue;
            };
            let mut kept = Vec::with_capacity(ids.len());
            for id in ids {
                let tombstoned: bool = tx.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM memory_tombstones
                          WHERE kind='episode' AND id=?1
                     )",
                    [id],
                    |row| row.get::<_, i64>(0),
                )? != 0;
                if !tombstoned {
                    kept.push(id);
                }
            }
            let encoded =
                serde_json::to_string(&Value::Array(kept.into_iter().map(Value::from).collect()))?;
            if encoded != raw {
                tx.execute(
                    &format!("UPDATE {table} SET source_episode_ids=?1 WHERE id=?2"),
                    rusqlite::params![encoded, row_id],
                )?;
            }
        }
    }
    Ok(())
}

fn apply_database(database: &Path) -> Result<usize> {
    let mut conn =
        Connection::open(&database).with_context(|| format!("opening {}", database.display()))?;
    let tx = conn.transaction()?;

    // A bound session workspace is *used*: the turn task runs tools there. One
    // that does not exist on this machine must go, or every turn in that
    // session starts by silently falling back.
    let cleared = clear_missing_workspaces(&tx, "sessions")?;
    // turns.workspace is only a record of where a past turn ran, but a path
    // from another machine is worse than nothing.
    clear_missing_workspaces(&tx, "turns")?;

    // Absolute file paths in the per-turn footprint describe the old machine's
    // filesystem; there is no meaningful translation.
    tx.execute("UPDATE turns SET tool_footprint = NULL", [])?;

    // Process ids belong to processes that died with the old machine. Stale
    // owner pids make turns look like they are owned by a live process.
    tx.execute(
        "UPDATE turns SET owner_pid = 0 WHERE owner_pid IS NOT NULL",
        [],
    )
    .ok();
    tx.execute(
        "UPDATE queued_prompts SET owner_pid = 0 WHERE owner_pid IS NOT NULL",
        [],
    )
    .ok();

    tx.commit()?;
    Ok(cleared)
}

/// Returns every conversation database in the current layout, without ever
/// following a symlinked home directory. The legacy and new-admin paths are
/// deduplicated because they are identical before the home migration.
pub(crate) fn conversation_db_paths(paths: &YunXiPaths) -> Result<Vec<PathBuf>> {
    let mut databases = Vec::new();
    for database in [
        paths.conversation_db_dir().join("conversation.db"),
        paths.state_dir.join("conversation.db"),
    ] {
        if !databases.contains(&database) {
            databases.push(database);
        }
    }

    let homes = paths.homes_dir();
    match std::fs::symlink_metadata(&homes) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            anyhow::bail!(
                "refusing to inspect a symlinked homes directory: {}",
                homes.display()
            );
        }
        Ok(metadata) if metadata.is_dir() => {
            for child in std::fs::read_dir(&homes)? {
                let child = child?;
                let path = child.path();
                let metadata = std::fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() {
                    anyhow::bail!("refusing to inspect a symlinked home: {}", path.display());
                }
                if metadata.is_dir() {
                    let database = path.join("conversation.db");
                    if !databases.contains(&database) {
                        databases.push(database);
                    }
                }
            }
        }
        Ok(_) => anyhow::bail!("homes path is not a directory: {}", homes.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("checking {}", homes.display())),
    }
    Ok(databases)
}

/// Nulls `workspace` on rows whose directory is absent here.
fn clear_missing_workspaces(tx: &rusqlite::Transaction<'_>, table: &str) -> Result<usize> {
    let sql = format!("SELECT DISTINCT workspace FROM {table} WHERE workspace IS NOT NULL");
    let missing: Vec<String> = {
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.filter_map(Result::ok)
            .filter(|workspace| !Path::new(workspace).is_dir())
            .collect()
    };
    let mut cleared = 0usize;
    for workspace in &missing {
        cleared += tx.execute(
            &format!("UPDATE {table} SET workspace = NULL WHERE workspace = ?1"),
            rusqlite::params![workspace],
        )?;
    }
    Ok(cleared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database(path: &Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (session_id TEXT PRIMARY KEY, workspace TEXT);
             CREATE TABLE turns (turn_id TEXT PRIMARY KEY, workspace TEXT,
                                 tool_footprint TEXT, owner_pid INTEGER);
             CREATE TABLE queued_prompts (prompt_id TEXT PRIMARY KEY, owner_pid INTEGER);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn machine_specific_columns_are_cleared_but_local_paths_survive() {
        let temp = tempfile::tempdir().unwrap();
        let here = temp.path().to_string_lossy().to_string();
        let state = temp.path().join("state");
        std::fs::create_dir_all(&state).unwrap();
        let db_path = state.join("conversation.db");
        {
            let conn = database(&db_path);
            conn.execute(
                "INSERT INTO sessions VALUES ('s1', '/nonexistent/from/old/machine')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO sessions VALUES ('s2', ?1)", [&here])
                .unwrap();
            conn.execute(
                "INSERT INTO turns VALUES ('t1', '/nonexistent/from/old/machine',
                     '{\"read\":[\"/home/other/.yunxi\"]}', 4242)",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO queued_prompts VALUES ('q1', 4242)", [])
                .unwrap();
        }

        let mut paths = crate::transfer::tests::test_paths(temp.path());
        paths.state_dir = state.clone();
        let cleared = apply(&paths).unwrap();
        assert_eq!(cleared, 1);

        let conn = Connection::open(&db_path).unwrap();
        let s1: Option<String> = conn
            .query_row(
                "SELECT workspace FROM sessions WHERE session_id='s1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            s1.is_none(),
            "a workspace from the old machine must be cleared"
        );
        let s2: Option<String> = conn
            .query_row(
                "SELECT workspace FROM sessions WHERE session_id='s2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            s2.as_deref(),
            Some(here.as_str()),
            "a directory that exists here is still usable and must be kept"
        );
        let footprint: Option<String> = conn
            .query_row("SELECT tool_footprint FROM turns", [], |row| row.get(0))
            .unwrap();
        assert!(footprint.is_none());
        let owner: i64 = conn
            .query_row("SELECT owner_pid FROM turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(owner, 0, "pids from the old machine must not look alive");
        let queued: i64 = conn
            .query_row("SELECT owner_pid FROM queued_prompts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(queued, 0);
    }

    #[test]
    fn a_missing_database_is_not_an_error() {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::transfer::tests::test_paths(temp.path());
        assert_eq!(apply(&paths).unwrap(), 0);
    }

    #[test]
    fn memory_tombstones_filter_an_old_snapshot_before_install() {
        let temp = tempfile::tempdir().unwrap();
        let live = temp.path().join("live.db");
        let staged = temp.path().join("staged.db");
        let live_conn = Connection::open(&live).unwrap();
        live_conn
            .execute_batch(
                "CREATE TABLE memory_tombstones (
                    kind TEXT NOT NULL,
                    id INTEGER NOT NULL,
                    deleted_at TEXT NOT NULL,
                    PRIMARY KEY(kind, id)
                 );
                 INSERT INTO memory_tombstones VALUES ('fact', 1, '2026-10-02T00:00:00Z');
                 INSERT INTO memory_tombstones VALUES ('episode', 2, '2026-10-02T00:00:00Z');",
            )
            .unwrap();
        drop(live_conn);

        let staged_conn = Connection::open(&staged).unwrap();
        staged_conn
            .execute_batch(
                "CREATE TABLE facts (id INTEGER PRIMARY KEY, content TEXT NOT NULL,
                                     source_episode_ids TEXT NOT NULL DEFAULT '[]');
                 CREATE TABLE episodes (id INTEGER PRIMARY KEY, content TEXT NOT NULL,
                                        source_episode_ids TEXT NOT NULL DEFAULT '[]');
                 CREATE TABLE memory_revisions (
                    id INTEGER PRIMARY KEY, memory_id INTEGER NOT NULL,
                    old_content TEXT NOT NULL, new_content TEXT NOT NULL,
                    source_episode_ids TEXT NOT NULL DEFAULT '[]');
                 CREATE TABLE memory_embeddings (
                    kind TEXT NOT NULL, id INTEGER NOT NULL, model TEXT NOT NULL,
                    content_sha256 TEXT NOT NULL, embedding BLOB NOT NULL,
                    created_at TEXT NOT NULL, PRIMARY KEY(kind, id));
                 INSERT INTO facts VALUES (1, 'deleted fact', '[2]');
                 INSERT INTO facts VALUES (3, 'retained fact', '[2]');
                 INSERT INTO episodes VALUES (2, 'deleted episode', '[]');
                 INSERT INTO episodes VALUES (4, 'retained episode', '[]');
                 INSERT INTO memory_revisions VALUES (9, 1, 'old', 'new', '[2]');
                 INSERT INTO memory_embeddings VALUES ('fact', 1, 'test', 'sha', X'00000000', 'now');
                 INSERT INTO memory_embeddings VALUES ('episode', 2, 'test', 'sha', X'00000000', 'now');",
            )
            .unwrap();
        drop(staged_conn);

        let removed = apply_memory_tombstones(&[(live, staged.clone())]).unwrap();
        assert_eq!(removed, 5);
        let conn = Connection::open(staged).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM facts", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT id FROM facts", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM episodes", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM memory_revisions", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let sources: String = conn
            .query_row(
                "SELECT source_episode_ids FROM facts WHERE id=3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sources, "[]");
    }
}
