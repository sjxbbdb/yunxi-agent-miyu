//! Rewriting the parts of a restored database that described the old machine.
//!
//! Blobs travel fine — images and attachments live inside the database. What
//! does not travel are absolute paths and process ids: they describe a machine
//! that is not this one, and left alone they either silently do nothing or
//! point work at directories that do not exist here.

use anyhow::{Context, Result};
use rusqlite::Connection;
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
}
