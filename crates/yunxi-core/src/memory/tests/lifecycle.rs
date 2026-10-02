//! G3-01 生命周期审计接缝回归。

use super::shared::*;
use crate::memory::*;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeSet;
use yunxi_base::config::AppConfig;

fn event_rows(store: &MemoryStore) -> Vec<(String, String, String, String, String, i64)> {
    let conn = store.data_conn().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT from_state, to_state, owner, owner_scope, content_digest, generation
               FROM memory_lifecycle_events ORDER BY event_id",
        )
        .unwrap();
    stmt.query_map([], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
        ))
    })
    .unwrap()
    .collect::<std::result::Result<Vec<_>, _>>()
    .unwrap()
}

#[test]
fn lifecycle_transition_contract_is_strict_and_stable() {
    for (from, to) in [
        (MemoryLifecycleState::Transient, MemoryLifecycleState::Short),
        (
            MemoryLifecycleState::Transient,
            MemoryLifecycleState::Committed,
        ),
        (MemoryLifecycleState::Short, MemoryLifecycleState::Candidate),
        (MemoryLifecycleState::Short, MemoryLifecycleState::Expired),
        (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Committed,
        ),
        (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Rejected,
        ),
    ] {
        assert!(validate_transition(from, to).is_ok());
    }
    for (from, to) in [
        (MemoryLifecycleState::Short, MemoryLifecycleState::Committed),
        (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Expired,
        ),
        (
            MemoryLifecycleState::Committed,
            MemoryLifecycleState::Rejected,
        ),
        (
            MemoryLifecycleState::Rejected,
            MemoryLifecycleState::Committed,
        ),
    ] {
        assert!(validate_transition(from, to).is_err());
    }
    assert!(validate_owned_transition(
        MemoryLifecycleState::Candidate,
        MemoryLifecycleState::Committed,
        MemoryLifecycleOwner::DecisionPort,
    )
    .is_err());
    assert!(validate_owned_transition(
        MemoryLifecycleState::Transient,
        MemoryLifecycleState::Short,
        MemoryLifecycleOwner::MemoryOrganizer,
    )
    .is_err());
    assert!(validate_owned_transition(
        MemoryLifecycleState::Candidate,
        MemoryLifecycleState::Rejected,
        MemoryLifecycleOwner::MemoryGc,
    )
    .is_ok());
    assert_eq!(MemoryLifecycleState::Candidate.as_str(), "candidate");
    assert_eq!(MemoryLifecycleOwner::MemoryGc.as_str(), "memory_gc");
    assert_eq!(
        MemoryLifecycleState::parse("short"),
        Some(MemoryLifecycleState::Short)
    );
    let json = serde_json::to_value(MemoryLifecycleState::Expired).unwrap();
    assert_eq!(json, Value::String("expired".to_string()));
}

#[test]
fn lifecycle_schema_is_idempotent_and_migrates_an_old_memory_meta() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store.init().unwrap();
    store.init().unwrap();
    let conn = store.data_conn().unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT lifecycle_schema_version FROM memory_meta WHERE id=1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_lifecycle_events'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
    drop(conn);

    let old_temp = tempfile::tempdir().unwrap();
    let old_paths = test_paths(&old_temp);
    let old_store = MemoryStore::new(&config, &old_paths);
    std::fs::create_dir_all(old_store.data_db.parent().unwrap()).unwrap();
    let old_conn = Connection::open(&old_store.data_db).unwrap();
    old_conn
        .execute_batch(
            "CREATE TABLE memory_meta (
                 id INTEGER PRIMARY KEY CHECK(id=1),
                 generation INTEGER NOT NULL DEFAULT 0,
                 database_id TEXT NOT NULL DEFAULT '',
                 access_schema_version INTEGER NOT NULL DEFAULT 2
             );
             INSERT INTO memory_meta (id, generation, database_id, access_schema_version)
             VALUES (1, 4, 'legacy', 2);",
        )
        .unwrap();
    drop(old_conn);
    old_store.init().unwrap();
    let old_conn = old_store.data_conn().unwrap();
    assert_eq!(
        old_conn
            .query_row(
                "SELECT lifecycle_schema_version FROM memory_meta WHERE id=1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}

#[test]
fn completed_turn_records_transient_to_short_without_raw_event_content() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let store = MemoryStore::new(&config, &test_paths(&temp));
    assert!(record_turn(
        &store,
        "用户的瞬时问题 G3_RAW_MARKER",
        "普通回答"
    ));
    let rows = event_rows(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(&rows[0].0, "transient");
    assert_eq!(&rows[0].1, "short");
    assert_eq!(&rows[0].2, "turn_loop");
    assert_eq!(&rows[0].3, "privileged");
    assert_ne!(&rows[0].4, "用户的瞬时问题 G3_RAW_MARKER");
    assert_eq!(rows[0].4.len(), 64);
    let conn = store.data_conn().unwrap();
    assert_eq!(count_rows(&conn, "memory_embeddings").unwrap(), 0);
    let lifecycle_columns = conn
        .prepare("PRAGMA table_info(memory_lifecycle_events)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert!(!lifecycle_columns.iter().any(|column| {
        matches!(
            column.as_str(),
            "content" | "user_message" | "assistant_message"
        )
    }));
    assert!(conn
        .execute(
            "UPDATE memory_lifecycle_events SET reason_code='mutated'",
            [],
        )
        .is_err());
    assert!(conn
        .execute("DELETE FROM memory_lifecycle_events", [])
        .is_err());
}

#[test]
fn organizer_candidate_audit_is_idempotent_and_commit_reject_are_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let config = diary_config(2);
    let store = MemoryStore::new(&config, &test_paths(&temp));
    assert!(record_turn(&store, "候选一", "回答一"));
    assert!(record_turn(&store, "候选二", "回答二"));
    let batch = store.next_organization_batch().unwrap().unwrap();
    assert_eq!(event_rows(&store).len(), 4);
    let again = store.next_organization_batch().unwrap().unwrap();
    assert_eq!(batch.diaries.len(), again.diaries.len());
    assert_eq!(event_rows(&store).len(), 4);

    let committed_id = batch.diaries[0].id;
    store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: Vec::new(),
                long_diaries: vec![LongDiaryDraft {
                    content: "可回溯的长期经历".to_string(),
                    importance: 3,
                    confidence: 0.9,
                    visibility: VISIBILITY_PRIVILEGED.to_string(),
                    subjects: Vec::new(),
                    tags: Vec::new(),
                    diary_ids: vec![committed_id],
                }],
            },
        )
        .unwrap();
    let rows = event_rows(&store);
    assert_eq!(rows.len(), 6);
    assert_eq!(
        (&rows[4].0, &rows[4].1),
        (&"candidate".to_string(), &"committed".to_string())
    );
    assert_eq!(
        (&rows[5].0, &rows[5].1),
        (&"candidate".to_string(), &"rejected".to_string())
    );
    assert!(rows.iter().all(|row| !row.4.contains("候选")));
}

#[test]
fn expired_short_rows_are_audited_before_forget_or_delete() {
    let temp = tempfile::tempdir().unwrap();
    let store = MemoryStore::new(&AppConfig::default(), &test_paths(&temp));
    store.init().unwrap();
    let conn = store.data_conn().unwrap();
    conn.execute(
        "INSERT INTO episodes (content, source, status, created_at, updated_at, retention, expires_at, consolidated_at)
         VALUES ('expired-one', 'episode', 'active', ?1, ?1, 'short_term', ?1, NULL)",
        ["2020-01-01T00:00:00Z"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO episodes (content, source, status, created_at, updated_at, retention, expires_at, consolidated_at)
         VALUES ('expired-two', 'episode', 'active', ?1, ?1, 'short_term', ?1, ?1)",
        ["2020-01-01T00:00:00Z"],
    )
    .unwrap();
    drop(conn);
    assert_eq!(store.cleanup_expired_short_diaries().unwrap(), 1);
    let rows = event_rows(&store);
    assert_eq!(rows.len(), 2);
    assert!(rows
        .iter()
        .all(|row| row.0 == "short" && row.1 == "expired"));
    let conn = store.data_conn().unwrap();
    assert_eq!(count_rows(&conn, "episodes").unwrap(), 1);
    assert_eq!(
        conn.query_row("SELECT status FROM episodes", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "forgotten"
    );
}

#[test]
fn reset_generation_and_owner_scope_keep_audit_rows_distinct() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    assert!(record_turn(&store, "第一代", "回答"));
    let origin = platform_origin("scope-user", "展示名不入审计");
    let (database_id, generation) = store.identity().unwrap();
    assert!(store
        .process_after_turn("第二代", "回答", &origin, &database_id, generation)
        .unwrap());
    let before_reset = event_rows(&store);
    assert!(before_reset
        .iter()
        .any(|row| row.3.starts_with("principal:")));
    store.reset_all().unwrap();
    assert!(record_turn(&store, "新代", "回答"));
    let rows = event_rows(&store);
    let generations = rows.iter().map(|row| row.5).collect::<BTreeSet<_>>();
    assert_eq!(generations, BTreeSet::from([0, 1]));
    assert!(rows.iter().all(|row| !row.3.contains("展示名不入审计")));
}
