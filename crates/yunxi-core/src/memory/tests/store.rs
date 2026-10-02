//! 存取、检索与重置。

use super::shared::*;
use crate::memory::browse::BrowseTable;
use crate::memory::*;
use yunxi_base::config::AppConfig;

#[test]
fn evicted_search_is_indexed_and_can_be_narrowed_by_time() {
    let temp = tempfile::tempdir().unwrap();
    let store = MemoryStore::new(&AppConfig::default(), &test_paths(&temp));
    store.init().unwrap();
    let rows: Vec<EvictedTurn> = (0..1200)
        .map(|index| EvictedTurn {
            source_id: format!("t{index}:user"),
            timestamp: format!("2026-08-{:02}T10:00:00+00:00", (index % 28) + 1),
            role: "user".to_string(),
            content: format!("第 {index} 轮，聊到了 蓝色小刺猬 这个话题"),
            ..EvictedTurn::default()
        })
        .collect();
    store.remember_evicted_turns(&rows).unwrap();

    // The scan used to stop at the newest 1000 rows, so anything older was
    // stored forever and reachable never.
    let oldest = store
        .search_evicted_context_readonly("第 3 轮", 50, None, None)
        .unwrap();
    assert!(
        oldest["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["snippet"]
                .as_str()
                .unwrap_or_default()
                .contains("第 3 轮")),
        "{oldest}"
    );

    // "What were we talking about that morning" is a question about when.
    let ranged = store
        .search_evicted_context_readonly(
            "蓝色小刺猬",
            50,
            Some("2026-08-05T00:00:00+00:00"),
            Some("2026-08-05T23:59:59+00:00"),
        )
        .unwrap();
    let hits = ranged["results"].as_array().unwrap();
    assert!(!hits.is_empty(), "{ranged}");
    assert!(
        hits.iter().all(|hit| hit["timestamp"]
            .as_str()
            .unwrap_or_default()
            .starts_with("2026-08-05")),
        "{ranged}"
    );
}

#[test]
fn compact_jieba_matches_reference_segmentation() {
    let reference = jieba_rs::Jieba::new();
    for input in [
        "我们中出了一个叛徒",
        "Wayland 输入法需要 XMODIFIERS",
        "Niri窗口规则和中文输入法配置",
        "podman-compose 不能直接重新创建容器",
        "北京烤鸭真好吃，后天天气不好。",
        "Rust 2024 edition与C++20",
    ] {
        assert_eq!(
            JIEBA.cut(input),
            reference.cut(input, false),
            "segmentation differs for {input}"
        );
    }
}

#[test]
fn remembers_and_recalls_fact() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store
        .remember_fact("Niri 输入法需要 XMODIFIERS", "test")
        .unwrap();
    let result = store.recall_memories("Niri XMODIFIERS", 5, false).unwrap();
    assert!(result.to_string().contains("XMODIFIERS"));
}

#[test]
fn user_profile_is_prompt_only_and_never_enters_memory_tables() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    let paths = test_paths(&temp);
    let profile_marker = "PROFILE_ONLY_BOUNDARY_MARKER";

    // The profile is an identity prompt input. It is deliberately placed in
    // the same temporary config root used by the memory fixture so this test
    // exercises the real path resolver rather than a detached string.
    config.prompt.user_identity_file = "profile.md".to_string();
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        config.user_identity_path(&paths),
        format!("# User profile\n\n{profile_marker}\n"),
    )
    .unwrap();
    let prompt = config.system_prompt(&paths).unwrap();
    assert!(prompt.contains(profile_marker));

    // A normal memory write proves the boundary against a populated store:
    // the profile marker may be present in the prompt, but only explicit
    // memory inputs may reach facts/episodes. Vector rows are empty too,
    // because no memory row containing the profile exists to embed.
    let store = MemoryStore::new(&config, &paths);
    store
        .remember_fact("显式记忆：用户喜欢安静的终端", "test")
        .unwrap();
    assert!(record_turn(&store, "普通会话内容", "普通回复内容"));

    let conn = store.data_conn().unwrap();
    for table in ["facts", "episodes"] {
        let sql = format!("SELECT COUNT(*) FROM {table} WHERE content LIKE ?1");
        let count: i64 = conn
            .query_row(&sql, [format!("%{profile_marker}%")], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "profile marker leaked into {table}");
    }
    let vectors: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_embeddings", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(vectors, 0, "profile-only input created a memory vector");

    let recalled = store.recall_memories(profile_marker, 10, false).unwrap();
    assert!(recalled["facts"].as_array().unwrap().is_empty());
    assert!(recalled["episodes"].as_array().unwrap().is_empty());
}

#[test]
fn evicted_context_uses_the_same_principal_filter() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let origin_a = platform_origin("7", "Alice");
    let origin_b = platform_origin("8", "Bob");
    let user_a = scoped_store(&config, &paths, &origin_a, false);
    let user_b = scoped_store(&config, &paths, &origin_b, false);
    user_a
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "a:user".to_string(),
            timestamp: "now".to_string(),
            role: "user".to_string(),
            content: "淘汰记忆 Alice 专属".to_string(),
            ..EvictedTurn::default()
        }])
        .unwrap();
    user_b
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "b:user".to_string(),
            timestamp: "now".to_string(),
            role: "user".to_string(),
            content: "淘汰记忆 Bob 专属".to_string(),
            ..EvictedTurn::default()
        }])
        .unwrap();

    let a = user_a
        .search_evicted_context("淘汰记忆", 10)
        .unwrap()
        .to_string();
    assert!(a.contains("Alice 专属"));
    assert!(!a.contains("Bob 专属"));
    let all = MemoryStore::new(&config, &paths)
        .search_evicted_context("淘汰记忆", 10)
        .unwrap()
        .to_string();
    assert!(all.contains("Alice 专属"));
    assert!(all.contains("Bob 专属"));
}

#[test]
fn reset_all_clears_facts_and_episodes() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store
        .remember_fact("Niri 输入法需要 XMODIFIERS", "test")
        .unwrap();
    store.remember_pending_event("你好", "在呢").unwrap();
    store.flush_pending_events().unwrap();

    let before = store.recall_memories("你好 XMODIFIERS", 5, false).unwrap();
    assert!(!before["facts"].as_array().unwrap().is_empty());
    assert!(!before["episodes"].as_array().unwrap().is_empty());

    store.reset_all().unwrap();

    let after = store.recall_memories("你好 XMODIFIERS", 5, false).unwrap();
    assert!(after["facts"].as_array().unwrap().is_empty());
    assert!(after["episodes"].as_array().unwrap().is_empty());
}

#[test]
fn evicted_context_can_be_cleared() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "turn-1:user".to_string(),
            timestamp: "now".to_string(),
            role: "user".to_string(),
            content: "旧上下文 输入法".to_string(),
            ..EvictedTurn::default()
        }])
        .unwrap();
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "turn-1:user".to_string(),
            timestamp: "now".to_string(),
            role: "user".to_string(),
            content: "旧上下文 输入法".to_string(),
            ..EvictedTurn::default()
        }])
        .unwrap();
    assert_eq!(
        store.search_evicted_context("输入法", 5).unwrap()["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(store
        .search_evicted_context("输入法", 5)
        .unwrap()
        .to_string()
        .contains("旧上下文"));
    store.clear_evicted_context().unwrap();
    assert!(!store
        .search_evicted_context("输入法", 5)
        .unwrap()
        .to_string()
        .contains("旧上下文"));
}

#[test]
fn tombstoned_fact_ref_hides_only_its_archived_tool_report() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    let fact_id = store.remember_fact("同一段记忆正文", "test").unwrap();
    store
        .remember_evicted_turns(&[
            EvictedTurn {
                source_id: "tool:remember_fact".to_string(),
                timestamp: "now".to_string(),
                role: "assistant".to_string(),
                content: "同一段记忆正文".to_string(),
                refs: vec![MemoryRef {
                    kind: "fact".to_string(),
                    id: fact_id,
                }],
                ..EvictedTurn::default()
            },
            EvictedTurn {
                source_id: "user:ordinary".to_string(),
                timestamp: "now".to_string(),
                role: "user".to_string(),
                content: "同一段记忆正文".to_string(),
                ..EvictedTurn::default()
            },
        ])
        .unwrap();
    let state = rusqlite::Connection::open(&store.state_db).unwrap();
    let provenance: (String, String, i64, String) = state
        .query_row(
            "SELECT carrier_kind, memory_kind, memory_id, relation
               FROM memory_provenance",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        provenance,
        (
            "evicted_turn".to_string(),
            "fact".to_string(),
            fact_id,
            "tool_report".to_string(),
        )
    );
    store.delete_item(BrowseTable::Facts, fact_id).unwrap();

    let results = store.search_evicted_context("同一段记忆正文", 10).unwrap()["results"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(results.len(), 1, "deleted fact report must not be recalled");
    assert_eq!(results[0]["role"], "user");
    let semantic_corpus = store.semantic_corpus(None, None).unwrap();
    assert_eq!(semantic_corpus.len(), 1);
    assert_eq!(semantic_corpus[0].1, "同一段记忆正文");
}

#[test]
fn old_evicted_state_database_migrates_provenance_schema() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    std::fs::create_dir_all(store.state_db.parent().unwrap()).unwrap();
    let old = rusqlite::Connection::open(&store.state_db).unwrap();
    old.execute_batch(
        "CREATE TABLE evicted_turns (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_id TEXT,
            timestamp TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL
         );
         CREATE TABLE evicted_embeddings (
            id INTEGER PRIMARY KEY,
            model TEXT NOT NULL,
            embedding_json TEXT NOT NULL,
            created_at TEXT NOT NULL
         );
         CREATE TABLE memory_provenance (
            carrier_kind TEXT NOT NULL,
            carrier_id INTEGER NOT NULL,
            memory_kind TEXT NOT NULL,
            memory_id INTEGER NOT NULL,
            relation TEXT NOT NULL,
            session_id TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
         );
         INSERT INTO memory_provenance
             (carrier_kind, carrier_id, memory_kind, memory_id, relation, created_at)
         VALUES ('evicted_turn', 1, 'fact', 2, 'memory_ref', 'old');
         INSERT INTO memory_provenance
             (carrier_kind, carrier_id, memory_kind, memory_id, relation, created_at)
         VALUES ('evicted_turn', 1, 'fact', 2, 'tool_report', 'current');",
    )
    .unwrap();
    drop(old);

    store.init().unwrap();
    let state = rusqlite::Connection::open(&store.state_db).unwrap();
    let exists: i64 = state
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memory_provenance'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exists, 1);
    let relation: String = state
        .query_row(
            "SELECT relation FROM memory_provenance WHERE carrier_id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(relation, "tool_report");
    let relation_count: i64 = state
        .query_row(
            "SELECT COUNT(*) FROM memory_provenance WHERE carrier_id=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(relation_count, 1);
}

#[test]
fn reset_all_invalidates_an_inflight_organization_batch() {
    let temp = tempfile::tempdir().unwrap();
    let config = diary_config(2);
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    assert!(record_turn(&store, "问题一", "回答一"));
    assert!(record_turn(&store, "问题二", "回答二"));
    let batch = store.next_organization_batch().unwrap().unwrap();
    let stale_database_id = batch.database_id.clone();
    let stale_generation = batch.generation;

    store.reset_all().unwrap();
    assert!(!store
        .process_after_turn(
            "重置前启动的问题",
            "不应写回",
            &test_origin(),
            &stale_database_id,
            stale_generation,
        )
        .unwrap());
    assert!(store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: Vec::new(),
                long_diaries: Vec::new(),
            },
        )
        .is_err());
    let conn = store.data_conn().unwrap();
    assert_eq!(count_rows(&conn, "facts").unwrap(), 0);
    assert_eq!(count_rows(&conn, "episodes").unwrap(), 0);
}

#[test]
fn cleanup_deletes_only_expired_consolidated_short_diaries() {
    let temp = tempfile::tempdir().unwrap();
    let config = diary_config(2);
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store.init().unwrap();
    let conn = store.data_conn().unwrap();
    conn.execute(
        "INSERT INTO episodes (
            content, source, status, created_at, updated_at, retention,
            expires_at, consolidated_at
         ) VALUES ('expired', 'episode', 'active', ?1, ?1, 'short_term', ?1, ?1)",
        ["2020-01-01T00:00:00Z"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO episodes (
            content, source, status, created_at, updated_at, retention,
            expires_at, consolidated_at
         ) VALUES ('pending', 'episode', 'active', ?1, ?1, 'short_term', ?1, NULL)",
        ["2020-01-01T00:00:00Z"],
    )
    .unwrap();
    drop(conn);

    assert_eq!(store.cleanup_expired_short_diaries().unwrap(), 1);
    let conn = store.data_conn().unwrap();
    assert_eq!(count_rows(&conn, "episodes").unwrap(), 1);
    assert_eq!(
        conn.query_row("SELECT content FROM episodes", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "pending"
    );
    assert_eq!(
        conn.query_row("SELECT status FROM episodes", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "forgotten"
    );
}

#[test]
fn organizer_never_recreates_a_moved_persona_database() {
    let temp = tempfile::tempdir().unwrap();
    let config = diary_config(2);
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    assert!(record_turn(&store, "问题一", "回答一"));
    assert!(record_turn(&store, "问题二", "回答二"));
    let batch = store.next_organization_batch().unwrap().unwrap();
    let memory_dir = store.data_db.parent().unwrap().to_path_buf();
    let moved_dir = memory_dir.with_file_name("memory-moved");
    std::fs::rename(&memory_dir, &moved_dir).unwrap();

    assert!(store.next_organization_batch().unwrap().is_none());
    assert!(!memory_dir.exists());
    assert!(store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: Vec::new(),
                long_diaries: Vec::new(),
            },
        )
        .is_err());
    assert!(!memory_dir.exists());

    store.init().unwrap();
    assert!(store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: Vec::new(),
                long_diaries: Vec::new(),
            },
        )
        .is_err());
}
