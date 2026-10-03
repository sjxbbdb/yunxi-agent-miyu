//! dashboard 记忆浏览:分页、过滤、删除;库不存在时按空处理。

use super::shared::*;
use crate::memory::browse::{BrowsePatch, BrowseQuery, BrowseTable, EvictedQuery};
use crate::memory::*;
use yunxi_base::config::AppConfig;

#[test]
fn browse_pages_filters_and_deletes_without_creating_databases() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);

    // 还没有库:空页,且不能因为浏览就建库。
    let empty = store
        .browse(BrowseTable::Facts, &BrowseQuery::default())
        .unwrap();
    assert_eq!(empty.total, 0);
    assert!(!config
        .active_persona_memory_data_dir(&paths)
        .join("memory.db")
        .exists());

    let a = store
        .remember_fact("用户喜欢 100% 纯黑咖啡", "test")
        .unwrap();
    let _b = store.remember_fact("用户住在东京", "test").unwrap();
    let _c = store.remember_fact("用户养了一只猫", "test").unwrap();

    let all = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                limit: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(all.total, 3);
    assert_eq!(all.items.len(), 2);
    let page2 = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                limit: 2,
                offset: 2,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(page2.items.len(), 1);

    // LIKE 通配符要转义:搜 "100%" 只能命中那一条。
    let hit = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                text: "100%".into(),
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hit.total, 1);
    assert_eq!(hit.items[0]["id"], a);

    assert!(store.delete_item(BrowseTable::Facts, a).unwrap());
    assert!(!store.delete_item(BrowseTable::Facts, a).unwrap());
    let rest = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                limit: 50,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(rest.total, 2);
    assert!(store
        .browse(
            BrowseTable::Episodes,
            &BrowseQuery {
                limit: 50,
                ..Default::default()
            }
        )
        .unwrap()
        .items
        .is_empty());
    assert!(BrowseTable::parse("turns").is_none());
}

#[test]
fn browse_detail_patch_revisions_and_readonly_stats() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);

    // 库不存在:统计全零、详情为空、编辑返回 false,且都不建库。
    let empty = store.stats_readonly().unwrap();
    assert_eq!(empty["exists"], false);
    assert_eq!(empty["facts"], 0);
    assert!(store.browse_item(BrowseTable::Facts, 1).unwrap().is_none());
    assert!(!store
        .update_item(BrowseTable::Facts, 1, &BrowsePatch::default())
        .unwrap());
    assert!(!config
        .active_persona_memory_data_dir(&paths)
        .join("memory.db")
        .exists());

    let id = store.remember_fact("用户在东京工作", "test").unwrap();
    let detail = store.browse_item(BrowseTable::Facts, id).unwrap().unwrap();
    assert_eq!(detail["memory_type"], "fact");
    assert_eq!(detail["truth_status"], "reported");
    assert_eq!(detail["importance"], 3);
    assert_eq!(detail["tags"], serde_json::json!([]));
    assert!(store.browse_revisions(id).unwrap().is_empty());

    // 改内容:写一条修订;改枚举字段与标签;非法值拒绝。
    let patch = BrowsePatch {
        content: Some("用户在大阪工作".into()),
        importance: Some(5),
        memory_type: Some("preference".into()),
        truth_status: Some("accepted".into()),
        tags: Some(vec!["工作".into(), " 城市 ".into(), "".into()]),
        ..Default::default()
    };
    assert!(store.update_item(BrowseTable::Facts, id, &patch).unwrap());
    let after = store.browse_item(BrowseTable::Facts, id).unwrap().unwrap();
    assert_eq!(after["content"], "用户在大阪工作");
    assert_eq!(after["importance"], 5);
    assert_eq!(after["memory_type"], "preference");
    assert_eq!(after["truth_status"], "accepted");
    assert_eq!(after["tags"], serde_json::json!(["工作", "城市"]));
    let revisions = store.browse_revisions(id).unwrap();
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0]["old_content"], "用户在东京工作");
    assert_eq!(revisions[0]["new_content"], "用户在大阪工作");
    // 内容没变就不再写修订。
    assert!(store
        .update_item(
            BrowseTable::Facts,
            id,
            &BrowsePatch {
                content: Some("用户在大阪工作".into()),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(store.browse_revisions(id).unwrap().len(), 1);
    assert!(store
        .update_item(
            BrowseTable::Facts,
            id,
            &BrowsePatch {
                truth_status: Some("maybe".into()),
                ..Default::default()
            }
        )
        .is_err());
    assert!(store
        .update_item(
            BrowseTable::Facts,
            id,
            &BrowsePatch {
                content: Some("   ".into()),
                ..Default::default()
            }
        )
        .is_err());

    // 删除事实也必须原子删除它的修订正文；删除后不能再通过历史抽屉读到
    // 已明确删除的旧内容。使用独立事实，保留上面那条带标签的事实继续覆盖
    // 遗忘/救回和筛选路径。
    let deleted_id = store.remember_fact("待删除的事实", "test").unwrap();
    assert!(store
        .update_item(
            BrowseTable::Facts,
            deleted_id,
            &BrowsePatch {
                content: Some("待删除的事实更新".into()),
                ..Default::default()
            }
        )
        .unwrap());
    assert_eq!(store.browse_revisions(deleted_id).unwrap().len(), 1);
    assert!(store.delete_item(BrowseTable::Facts, deleted_id).unwrap());
    assert!(store
        .browse_item(BrowseTable::Facts, deleted_id)
        .unwrap()
        .is_none());
    assert!(store.browse_revisions(deleted_id).unwrap().is_empty());

    // 遗忘 → 救回:状态回 active 且强度回满。
    assert!(store
        .update_item(
            BrowseTable::Facts,
            id,
            &BrowsePatch {
                status: Some("forgotten".into()),
                ..Default::default()
            }
        )
        .unwrap());
    let stats = store.stats_readonly().unwrap();
    assert_eq!(stats["facts"], 0);
    assert_eq!(stats["facts_forgotten"], 1);
    assert_eq!(stats["revisions"], 1);
    let by_status = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                status: "forgotten".into(),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_status.total, 1);
    let by_tag = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                tag: "工作".into(),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_tag.total, 1);
    let by_type = store
        .browse(
            BrowseTable::Facts,
            &BrowseQuery {
                memory_type: "fact".into(),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_type.total, 0);
    assert!(store
        .update_item(
            BrowseTable::Facts,
            id,
            &BrowsePatch {
                status: Some("active".into()),
                ..Default::default()
            }
        )
        .unwrap());
    let revived = store.browse_item(BrowseTable::Facts, id).unwrap().unwrap();
    assert_eq!(revived["status"], "active");
    assert_eq!(revived["strength"], 1.0);

    // 逐出归档:不存在按空;写入后可分页、按角色过滤、按 id 取全文、删除。
    let none = store.browse_evicted(&EvictedQuery::default()).unwrap();
    assert_eq!(none.total, 0);
    store
        .remember_evicted_turns(&[
            EvictedTurn {
                source_id: "t1".into(),
                timestamp: "2026-09-01T10:00:00+00:00".into(),
                role: "user".into(),
                content: "昨天我们聊了 rust 的所有权".into(),
                visibility: "privileged".into(),
                owner_principal: String::new(),
                owner_display_name: String::new(),
                refs: Vec::new(),
            },
            EvictedTurn {
                source_id: "t2".into(),
                timestamp: "2026-09-01T10:01:00+00:00".into(),
                role: "assistant".into(),
                content: "对,借用检查器那段".into(),
                visibility: "privileged".into(),
                owner_principal: String::new(),
                owner_display_name: String::new(),
                refs: Vec::new(),
            },
        ])
        .unwrap();
    let all = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.total, 2);
    assert_eq!(all.items[0]["role"], "assistant");
    let users = store
        .browse_evicted(&EvictedQuery {
            role: "user".into(),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(users.total, 1);
    let searched = store
        .browse_evicted(&EvictedQuery {
            text: "所有权".into(),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(searched.total, 1);
    let first_id = users.items[0]["id"].as_i64().unwrap();
    let full = store.browse_evicted_item(first_id).unwrap().unwrap();
    assert_eq!(full["content"], "昨天我们聊了 rust 的所有权");
    let stats = store.stats_readonly().unwrap();
    assert_eq!(stats["evicted_turns"], 2);
    assert!(store.delete_evicted_item(first_id).unwrap());
    assert!(!store.delete_evicted_item(first_id).unwrap());
    assert_eq!(store.stats_readonly().unwrap()["evicted_turns"], 1);
}

#[test]
fn browse_evicted_hides_tombstoned_linked_carrier_but_keeps_unlinked_row() {
    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    let fact_id = store.remember_fact("浏览墓碑回归事实", "test").unwrap();
    store
        .remember_evicted_turns(&[
            EvictedTurn {
                source_id: "linked-tool-report".into(),
                timestamp: "2026-09-02T10:00:00+00:00".into(),
                role: "assistant".into(),
                content: "浏览墓碑回归事实".into(),
                refs: vec![MemoryRef {
                    kind: "fact".into(),
                    id: fact_id,
                }],
                ..EvictedTurn::default()
            },
            EvictedTurn {
                source_id: "unlinked-user-row".into(),
                timestamp: "2026-09-02T10:01:00+00:00".into(),
                role: "user".into(),
                content: "浏览墓碑回归事实".into(),
                ..EvictedTurn::default()
            },
        ])
        .unwrap();

    let before = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(before.total, 2);
    let linked_id = before
        .items
        .iter()
        .find(|item| item["role"] == "assistant")
        .and_then(|item| item["id"].as_i64())
        .unwrap();
    let unlinked_id = before
        .items
        .iter()
        .find(|item| item["role"] == "user")
        .and_then(|item| item["id"].as_i64())
        .unwrap();

    store.delete_item(BrowseTable::Facts, fact_id).unwrap();

    let after = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(after.total, 1);
    assert_eq!(after.items[0]["id"], unlinked_id);
    assert!(store.browse_evicted_item(linked_id).unwrap().is_none());
    assert_eq!(
        store.browse_evicted_item(unlinked_id).unwrap().unwrap()["id"],
        unlinked_id
    );

    // The keyword path already applies the same barrier; keep it covered here
    // so direct browse changes cannot accidentally diverge from search.
    let searched = store
        .browse_evicted(&EvictedQuery {
            text: "浏览墓碑回归事实".into(),
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(searched.total, 1);
    assert_eq!(searched.items[0]["id"], unlinked_id);
}

#[test]
fn concurrent_delete_and_browse_converge_on_tombstone_barrier() {
    use std::sync::{Arc, Barrier};

    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    let fact_id = store.remember_fact("并发删除回归事实", "test").unwrap();
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "concurrent-linked-tool-report".into(),
            timestamp: "2026-10-02T10:00:00+00:00".into(),
            role: "assistant".into(),
            content: "并发删除回归事实".into(),
            refs: vec![MemoryRef {
                kind: "fact".into(),
                id: fact_id,
            }],
            ..EvictedTurn::default()
        }])
        .unwrap();

    let start = Arc::new(Barrier::new(2));
    let deleter = store.clone();
    let reader = store.clone();
    let delete_start = Arc::clone(&start);
    let read_start = Arc::clone(&start);
    let delete_thread = std::thread::spawn(move || {
        delete_start.wait();
        deleter.delete_item(BrowseTable::Facts, fact_id).unwrap()
    });
    let read_thread = std::thread::spawn(move || {
        read_start.wait();
        reader
            .browse_evicted(&EvictedQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
    });

    assert!(delete_thread.join().unwrap());
    let _raced_snapshot = read_thread.join().unwrap();

    let after = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(after.total, 0);
    assert!(store
        .memory_refs_are_tombstoned(&[MemoryRef {
            kind: "fact".into(),
            id: fact_id,
        }])
        .unwrap());
}

#[test]
fn evicted_detail_overlap_retries_after_tombstone_commit() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    };

    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    let fact_id = store
        .remember_fact("详情 overlap 回归事实", "test")
        .unwrap();
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "detail-overlap".into(),
            timestamp: "2026-10-03T10:00:00+00:00".into(),
            role: "assistant".into(),
            content: "详情 overlap 回归事实".into(),
            refs: vec![MemoryRef {
                kind: "fact".into(),
                id: fact_id,
            }],
            ..EvictedTurn::default()
        }])
        .unwrap();
    let carrier_id = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap()
        .items[0]["id"]
        .as_i64()
        .unwrap();

    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let once = Arc::new(AtomicBool::new(true));
    let reader = store.clone().with_overlap_hook(Arc::new({
        let reached = Arc::clone(&reached);
        let release = Arc::clone(&release);
        let once = Arc::clone(&once);
        move |point| {
            if point == OverlapPoint::BrowseBeforeEpoch && once.swap(false, Ordering::SeqCst) {
                reached.wait();
                release.wait();
            }
        }
    }));
    let detail = std::thread::spawn(move || reader.browse_evicted_item(carrier_id).unwrap());

    reached.wait();
    assert!(store.delete_item(BrowseTable::Facts, fact_id).unwrap());
    release.wait();

    assert!(detail.join().unwrap().is_none());
    assert!(store.browse_evicted_item(carrier_id).unwrap().is_none());
}

#[tokio::test]
async fn hybrid_keyword_overlap_retries_after_tombstone_commit() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    };

    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    let fact_id = store.remember_fact("确定性 overlap 事实", "test").unwrap();
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "deterministic-overlap".into(),
            timestamp: "2026-10-03T10:00:00+00:00".into(),
            role: "assistant".into(),
            content: "确定性 overlap 事实".into(),
            refs: vec![MemoryRef {
                kind: "fact".into(),
                id: fact_id,
            }],
            ..EvictedTurn::default()
        }])
        .unwrap();
    let carrier_id = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap()
        .items[0]["id"]
        .as_i64()
        .unwrap();

    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let once = Arc::new(AtomicBool::new(true));
    let reader = store.clone().with_overlap_hook(Arc::new({
        let reached = Arc::clone(&reached);
        let release = Arc::clone(&release);
        let once = Arc::clone(&once);
        move |point| {
            if point == OverlapPoint::KeywordBeforeEpoch && once.swap(false, Ordering::SeqCst) {
                reached.wait();
                release.wait();
            }
        }
    }));
    let search = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current()
            .block_on(reader.search_evicted_context_hybrid("确定性 overlap", 10, None, None))
            .unwrap()
    });

    reached.wait();
    assert!(store.delete_item(BrowseTable::Facts, fact_id).unwrap());
    release.wait();
    let result = search.await.unwrap();
    assert!(!result["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["id"].as_i64() == Some(carrier_id)));
    assert_eq!(
        store
            .browse_evicted(&EvictedQuery {
                text: "确定性 overlap".into(),
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .total,
        0
    );
}

#[tokio::test]
async fn state_delete_overlap_converges_on_follow_up_read() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    };

    let temp = tempfile::tempdir().unwrap();
    let config = AppConfig::default();
    let paths = test_paths(&temp);
    let store = MemoryStore::new(&config, &paths);
    store
        .remember_evicted_turns(&[EvictedTurn {
            source_id: "state-delete-overlap".into(),
            timestamp: "2026-10-03T10:01:00+00:00".into(),
            role: "assistant".into(),
            content: "state-side overlap carrier".into(),
            ..EvictedTurn::default()
        }])
        .unwrap();
    let id = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap()
        .items[0]["id"]
        .as_i64()
        .unwrap();

    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let once = Arc::new(AtomicBool::new(true));
    let reader = store.clone().with_overlap_hook(Arc::new({
        let reached = Arc::clone(&reached);
        let release = Arc::clone(&release);
        let once = Arc::clone(&once);
        move |point| {
            if point == OverlapPoint::BrowseBeforeEpoch && once.swap(false, Ordering::SeqCst) {
                reached.wait();
                release.wait();
            }
        }
    }));
    let delete_reached = Arc::new(Barrier::new(2));
    let delete_release = Arc::new(Barrier::new(2));
    let deleter = store.clone().with_overlap_hook(Arc::new({
        let delete_reached = Arc::clone(&delete_reached);
        let delete_release = Arc::clone(&delete_release);
        move |point| {
            if point == OverlapPoint::StateDeleteBeforeCommit {
                delete_reached.wait();
                delete_release.wait();
            }
        }
    }));
    let browse = tokio::task::spawn_blocking(move || {
        reader
            .browse_evicted(&EvictedQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
    });

    reached.wait();
    let delete = tokio::task::spawn_blocking(move || deleter.delete_evicted_item(id).unwrap());
    delete_reached.wait();
    delete_release.wait();
    assert!(delete.await.unwrap());
    release.wait();
    let _raced = browse.await.unwrap();

    let after = store
        .browse_evicted(&EvictedQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(after.total, 0);
    assert!(!store.delete_evicted_item(id).unwrap());
}
