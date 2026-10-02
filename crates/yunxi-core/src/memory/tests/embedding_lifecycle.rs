use super::shared::test_paths;
use crate::memory::browse::{BrowsePatch, BrowseTable};
use crate::memory::*;
use sha2::{Digest, Sha256};
use yunxi_base::config::AppConfig;

fn store(temp: &tempfile::TempDir) -> MemoryStore {
    let store = MemoryStore::new(&AppConfig::default(), &test_paths(temp));
    store.init().unwrap();
    store
}

fn digest(content: &str) -> String {
    hex::encode(Sha256::digest(content.as_bytes()))
}

fn embedding(store: &MemoryStore, kind: &str, id: i64, content_sha256: &str) {
    let conn = store.data_conn().unwrap();
    conn.execute(
        "INSERT INTO memory_embeddings
             (kind, id, model, content_sha256, embedding, created_at)
         VALUES (?1, ?2, 'test-model', ?3, ?4, ?5)",
        rusqlite::params![
            kind,
            id,
            content_sha256,
            vec![0_u8, 1, 2],
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .unwrap();
}

fn episode(store: &MemoryStore, content: &str, retention: &str, status: &str) -> i64 {
    let conn = store.data_conn().unwrap();
    let timestamp = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO episodes (
             content, source, status, recall_count, created_at, updated_at,
             retention, consolidated_at, origin_session_id
         ) VALUES (?1, 'test', ?2, 0, ?3, ?3, ?4, ?3, 'embedding-test')",
        rusqlite::params![content, status, timestamp, retention],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn vector_count(store: &MemoryStore, kind: &str, id: i64) -> i64 {
    store
        .data_conn()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM memory_embeddings WHERE kind=?1 AND id=?2",
            rusqlite::params![kind, id],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn reset_all_removes_fact_and_episode_embeddings() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let fact_id = store.remember_fact("持久事实", "test").unwrap();
    let episode_id = episode(&store, "长期经历", "long_term", "active");
    embedding(&store, "fact", fact_id, &digest("持久事实"));
    embedding(&store, "episode", episode_id, &digest("长期经历"));
    embedding(&store, "fact", 999_999, "orphan");
    store.reset_all().unwrap();
    let count: i64 = store
        .data_conn()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn browse_delete_and_update_keep_embeddings_in_same_transaction() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let fact_id = store.remember_fact("浏览事实", "test").unwrap();
    embedding(&store, "fact", fact_id, &digest("浏览事实"));
    assert!(store.delete_item(BrowseTable::Facts, fact_id).unwrap());
    assert_eq!(vector_count(&store, "fact", fact_id), 0);

    // A no-op delete must not become an implicit orphan cleanup operation;
    // the explicit prune pass owns that responsibility.
    embedding(&store, "fact", 999_998, "orphan");
    assert!(!store.delete_item(BrowseTable::Facts, 999_998).unwrap());
    assert_eq!(vector_count(&store, "fact", 999_998), 1);

    let fact_id = store.remember_fact("待修改事实", "test").unwrap();
    embedding(&store, "fact", fact_id, &digest("待修改事实"));
    assert!(store
        .update_item(
            BrowseTable::Facts,
            fact_id,
            &BrowsePatch {
                content: Some("修改后的事实".to_string()),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(vector_count(&store, "fact", fact_id), 0);

    let episode_id = episode(&store, "待遗忘经历", "long_term", "active");
    embedding(&store, "episode", episode_id, &digest("待遗忘经历"));
    assert!(store
        .update_item(
            BrowseTable::Episodes,
            episode_id,
            &BrowsePatch {
                status: Some("forgotten".to_string()),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(vector_count(&store, "episode", episode_id), 0);
}

#[test]
fn deleting_an_episode_scrubs_summary_provenance_references() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let source_id = episode(&store, "被删除的来源经历", "short_term", "active");
    let fact_id = store.remember_fact("保留的长期事实", "test").unwrap();
    let summary_id = episode(&store, "保留的长期摘要", "long_term", "active");
    let conn = store.data_conn().unwrap();
    conn.execute(
        "UPDATE facts SET source_episode_ids=?1 WHERE id=?2",
        rusqlite::params![format!("[{source_id}]"), fact_id],
    )
    .unwrap();
    conn.execute(
        "UPDATE episodes SET source_episode_ids=?1 WHERE id=?2",
        rusqlite::params![format!("[{source_id}]"), summary_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO memory_revisions
             (memory_id, old_content, new_content, source_episode_ids, created_at)
         VALUES (?1, '旧事实', '新事实', ?2, ?3)",
        rusqlite::params![
            fact_id,
            format!("[{source_id}]"),
            chrono::Utc::now().to_rfc3339()
        ],
    )
    .unwrap();
    drop(conn);

    assert!(store.delete_item(BrowseTable::Episodes, source_id).unwrap());
    let conn = store.data_conn().unwrap();
    for table in ["facts", "episodes", "memory_revisions"] {
        let refs: String = conn
            .query_row(
                &format!("SELECT source_episode_ids FROM {table} WHERE id=?1"),
                [if table == "facts" {
                    fact_id
                } else if table == "episodes" {
                    summary_id
                } else {
                    1
                }],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(refs, "[]", "stale source reference remained in {table}");
    }
}

#[test]
fn failed_browse_update_rolls_back_without_dropping_vector() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let fact_id = store.remember_fact("事务安全事实", "test").unwrap();
    embedding(&store, "fact", fact_id, &digest("事务安全事实"));
    assert!(store
        .update_item(
            BrowseTable::Facts,
            fact_id,
            &BrowsePatch {
                status: Some("not-a-status".to_string()),
                ..Default::default()
            }
        )
        .is_err());
    assert_eq!(vector_count(&store, "fact", fact_id), 1);
}

#[test]
fn expired_short_diary_drops_its_embedding() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let conn = store.data_conn().unwrap();
    let past = "2020-01-01T00:00:00Z";
    conn.execute(
        "INSERT INTO episodes (
             content, source, status, recall_count, created_at, updated_at,
             retention, expires_at, consolidated_at, promotion_pending, origin_session_id
         ) VALUES ('过期短记', 'test', 'active', 0, ?1, ?1, 'short_term', ?1, NULL, 0, 'embedding-test')",
        [past],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    drop(conn);
    embedding(&store, "episode", id, &digest("过期短记"));
    store.cleanup_expired_short_diaries().unwrap();
    assert_eq!(vector_count(&store, "episode", id), 0);
}

#[test]
fn decay_forgetting_revokes_the_vector_immediately() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.memory.forgetting_min_strength = 2.0;
    config.memory.forgetting_half_life_days = 1.0;
    let store = MemoryStore::new(&config, &test_paths(&temp));
    let id = store.remember_fact("会被衰减遗忘的事实", "test").unwrap();
    let conn = store.data_conn().unwrap();
    conn.execute(
        "UPDATE facts SET strength=1.0, updated_at='2020-01-01T00:00:00Z' WHERE id=?1",
        [id],
    )
    .unwrap();
    embedding(&store, "fact", id, &digest("会被衰减遗忘的事实"));
    drop(conn);

    store.init().unwrap();
    let conn = store.data_conn().unwrap();
    let status: String = conn
        .query_row("SELECT status FROM facts WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(status, "forgotten");
    assert_eq!(vector_count(&store, "fact", id), 0);
}

#[test]
fn organizer_fact_update_drops_old_embedding_for_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let fact_id = store.remember_fact("旧项目约束", "test").unwrap();
    embedding(&store, "fact", fact_id, &digest("旧项目约束"));
    let source = ShortDiaryRecord {
        id: 9001,
        created_at: chrono::Utc::now().to_rfc3339(),
        user_message: "项目约束必须保持稳定".to_string(),
        assistant_message: "已记录项目约束".to_string(),
        force_long_term: false,
        owner_principal: None,
        origin: MemoryOrigin::local("embedding-test"),
    };
    let (database_id, generation) = store.identity().unwrap();
    let batch = OrganizationBatch {
        database_id,
        generation,
        diaries: vec![source.clone()],
        existing: vec![ExistingMemoryRecord {
            id: fact_id,
            kind: "knowledge".to_string(),
            content: "旧项目约束".to_string(),
            truth_status: "reported".to_string(),
            visibility: VISIBILITY_PRIVILEGED.to_string(),
            owner_principal: String::new(),
            owner_display_name: String::new(),
        }],
    };
    store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: vec![KnowledgeAction {
                    operation: "update".to_string(),
                    target_id: Some(fact_id),
                    memory_type: "fact".to_string(),
                    content: "新项目约束".to_string(),
                    truth_status: "accepted".to_string(),
                    importance: 3,
                    confidence: 0.9,
                    visibility: VISIBILITY_PRIVILEGED.to_string(),
                    subjects: Vec::new(),
                    tags: Vec::new(),
                    diary_ids: vec![source.id],
                }],
                long_diaries: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(vector_count(&store, "fact", fact_id), 0);
}

#[test]
fn prune_stale_embeddings_is_delete_only_and_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let store = store(&temp);
    let valid_fact = store.remember_fact("有效事实", "test").unwrap();
    let stale_hash_fact = store.remember_fact("已变事实", "test").unwrap();
    let rejected_fact = store.remember_fact("被拒事实", "test").unwrap();
    let valid_episode = episode(&store, "有效长期经历", "long_term", "active");
    let short_episode = episode(&store, "短期经历", "short_term", "active");
    let forgotten_episode = episode(&store, "遗忘长期经历", "long_term", "forgotten");
    embedding(&store, "fact", valid_fact, &digest("有效事实"));
    embedding(&store, "fact", stale_hash_fact, "wrong-hash");
    embedding(&store, "fact", rejected_fact, &digest("被拒事实"));
    embedding(&store, "episode", valid_episode, &digest("有效长期经历"));
    embedding(&store, "episode", short_episode, &digest("短期经历"));
    embedding(
        &store,
        "episode",
        forgotten_episode,
        &digest("遗忘长期经历"),
    );
    embedding(&store, "fact", 999_999, "orphan");
    embedding(&store, "unknown", 888_888, "orphan");
    store
        .data_conn()
        .unwrap()
        .execute(
            "UPDATE facts SET truth_status='rejected' WHERE id=?1",
            [rejected_fact],
        )
        .unwrap();

    assert_eq!(store.prune_stale_embeddings().unwrap(), 6);
    assert_eq!(store.prune_stale_embeddings().unwrap(), 0);
    assert_eq!(vector_count(&store, "fact", valid_fact), 1);
    assert_eq!(vector_count(&store, "episode", valid_episode), 1);
    assert_eq!(vector_count(&store, "fact", stale_hash_fact), 0);
    assert_eq!(vector_count(&store, "fact", rejected_fact), 0);
    assert_eq!(vector_count(&store, "episode", short_episode), 0);
    assert_eq!(vector_count(&store, "episode", forgotten_episode), 0);
    assert_eq!(vector_count(&store, "fact", 999_999), 0);
    assert_eq!(vector_count(&store, "unknown", 888_888), 0);
}
