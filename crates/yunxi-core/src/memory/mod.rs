use yunxi_base::config::{AppConfig, KnowledgeBasePluginConfig, MemoryConfig};
use yunxi_base::paths::YunXiPaths;
// 只要主体身份这一个纯数据类型，不需要整个平台运行时。
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Arc;
use std::sync::LazyLock;
use yunxi_base::platform_types::PlatformPrincipal;

mod admission;
mod association;
pub mod browse;
mod dedup;
mod evicted;
mod lifecycle;
mod recall;
mod schema;
mod search;
mod semantic;
mod validate;
mod write;
pub(crate) use admission::*;
pub use association::*;
use schema::*;
use search::*;
use validate::*;
mod organizer;

pub use lifecycle::{
    content_digest, normalized_episode_ids, transition_key, validate_owned_transition,
    validate_transition, MemoryLifecycleEvent, MemoryLifecycleOwner, MemoryLifecycleState,
};
pub use organizer::{MemoryOrganizer, MemoryOrganizerHandle};

const SHORT_TERM: &str = "short_term";
const LONG_TERM: &str = "long_term";
const VISIBILITY_PUBLIC: &str = "public";
const VISIBILITY_PRINCIPAL: &str = "principal";
const VISIBILITY_PRIVILEGED: &str = "privileged";
static JIEBA: LazyLock<CompactJieba> = LazyLock::new(|| {
    CompactJieba::new().expect("the build-generated compact Jieba index must be valid")
});

#[derive(Clone)]
pub struct MemoryStore {
    config: MemoryConfig,
    kb_config: KnowledgeBasePluginConfig,
    /// 当前人格的技能目录:只用于状态展示与路径判定,记忆库不再直接动技能
    /// (自动技能的清理在 `skills::purge_generated_skills`,由上层接起来)。
    skills_dir: PathBuf,
    /// Kept whole because the embedding call needs provider lookup and the
    /// knowledge base's timeout setting.
    app_config: AppConfig,
    writes_enabled: bool,
    access: MemoryAccess,
    writer_principal: Option<String>,
    writer_display_name: String,
    /// 写入时盖在行上的会话标记，`reset_session` 按它只清本会话产生的记忆。
    /// 空串 = 这条写入路径不知道自己属于哪个会话（`yunxi memory remember`、
    /// 后台整理），那些行只能被 `reset_all` 清掉。
    session_id: String,
    data_db: PathBuf,
    state_db: PathBuf,
    #[cfg(test)]
    overlap_hook: Option<Arc<dyn Fn(OverlapPoint) + Send + Sync>>,
}

/// Deterministic read-barrier seam used only by the memory overlap tests.
/// Production builds do not carry the field, callback, or synchronization
/// machinery; the hook never changes the runtime read/write contract.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlapPoint {
    BrowseBeforeEpoch,
    KeywordBeforeEpoch,
    SemanticBeforeEpoch,
    StateDeleteBeforeCommit,
}

/// 一次记忆重置删掉了什么，按表分。每个触发面都要把它讲给用户听，所以计数
/// 留在存储层返回，而不是各面各自再查一遍。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub struct MemoryResetSummary {
    pub facts: usize,
    pub episodes: usize,
    pub pending_events: usize,
    pub evicted_turns: usize,
}

/// Remove deleted episode ids from active summaries and revision provenance.
///
/// Episode rows are the source of long-term summaries, but the summaries live
/// in separate rows.  Deleting an episode must not leave a stale source id
/// that can later be presented as a live provenance link.  Lifecycle events
/// are deliberately excluded: they are append-only audit records, not active
/// recall or summary references.
pub(crate) fn scrub_episode_references(
    tx: &rusqlite::Transaction<'_>,
    deleted_ids: &[i64],
) -> Result<()> {
    let deleted: BTreeSet<i64> = deleted_ids.iter().copied().collect();
    if deleted.is_empty() {
        return Ok(());
    }
    for table in ["facts", "episodes", "memory_revisions"] {
        let rows = {
            let mut stmt = tx.prepare(&format!(
                "SELECT id, source_episode_ids FROM {table} WHERE source_episode_ids!='[]'"
            ))?;
            let mapped = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            mapped.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (row_id, raw_ids) in rows {
            let Ok(existing) = serde_json::from_str::<Vec<i64>>(&raw_ids) else {
                continue;
            };
            let original_len = existing.len();
            let filtered: Vec<i64> = existing
                .into_iter()
                .filter(|id| !deleted.contains(id))
                .collect();
            if filtered.len() == original_len {
                continue;
            }
            tx.execute(
                &format!("UPDATE {table} SET source_episode_ids=?1 WHERE id=?2"),
                params![serde_json::to_string(&filtered)?, row_id],
            )?;
        }
    }
    Ok(())
}

/// Keep an id-level deletion marker after a memory row is physically removed.
///
/// Transfer/import uses these markers to stop an older archive from bringing
/// back something the user already deleted on the current installation.  The
/// marker intentionally contains no memory text or provenance: it is only a
/// durable negative fact keyed by the row kind and id.
pub(crate) fn record_memory_tombstones(
    tx: &rusqlite::Transaction<'_>,
    kind: &str,
    ids: &[i64],
    deleted_at: &str,
) -> Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    if !matches!(kind, "fact" | "episode") {
        bail!("invalid memory tombstone kind: {kind}");
    }
    let mut inserted = 0;
    for id in ids.iter().copied().filter(|id| *id > 0) {
        inserted += tx.execute(
            "INSERT OR IGNORE INTO memory_tombstones (kind, id, deleted_at)
             VALUES (?1, ?2, ?3)",
            params![kind, id, deleted_at],
        )?;
    }
    Ok(inserted)
}

/// Consistent data-side read snapshot used by the state-side carrier barrier.
/// The epoch and tombstone rows are read in one SQLite read transaction; no
/// lock is held while the independent state database is queried.
#[derive(Debug, Clone, Default)]
pub(crate) struct TombstoneSnapshot {
    pub(crate) epoch: i64,
    pub(crate) pairs: HashSet<(String, i64)>,
}

pub(crate) fn bump_tombstone_epoch(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "UPDATE memory_meta SET tombstone_epoch=tombstone_epoch+1 WHERE id=1",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
impl MemoryStore {
    pub(crate) fn with_overlap_hook(
        mut self,
        hook: Arc<dyn Fn(OverlapPoint) + Send + Sync>,
    ) -> Self {
        self.overlap_hook = Some(hook);
        self
    }

    pub(crate) fn overlap_hook(&self, point: OverlapPoint) {
        if let Some(hook) = &self.overlap_hook {
            hook(point);
        }
    }
}

impl MemoryResetSummary {
    pub fn total(&self) -> usize {
        self.facts + self.episodes + self.pending_events + self.evicted_turns
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// 给用户看的回执。文案只此一份：REPL、CLI、QQ、WebUI 都拿它，免得同一条
    /// 命令在四个界面上说法不一样。为零的项不列——「事实 0、经历 0」是噪音。
    pub fn describe(&self) -> String {
        if self.is_empty() {
            return yunxi_base::i18n::text(
                "This conversation had not recorded any memory yet.",
                "本次会话还没有记下什么，没有可清的记忆。",
            )
            .to_string();
        }
        let zh = yunxi_base::i18n::is_zh();
        let mut parts = Vec::new();
        for (count, en, zh_label) in [
            (self.facts, "facts", "事实"),
            (self.episodes, "episodes", "经历"),
            (self.pending_events, "pending events", "待整理事件"),
            (self.evicted_turns, "archived turns", "归档回合"),
        ] {
            if count > 0 {
                parts.push(if zh {
                    format!("{zh_label} {count}")
                } else {
                    format!("{count} {en}")
                });
            }
        }
        if zh {
            format!("已清空本次会话记下的记忆：{}。", parts.join("、"))
        } else {
            format!(
                "Erased the memory this conversation produced: {}.",
                parts.join(", ")
            )
        }
    }
}

/// Read authorization for one agent run. Storage remains persona-global; this
/// value only controls which rows may enter the model context or memory tools.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryAccess {
    Privileged,
    Principal(String),
}

impl MemoryAccess {
    pub fn principal(key: impl Into<String>) -> Self {
        Self::Principal(key.into())
    }

    fn principal_key(&self) -> Option<&str> {
        match self {
            Self::Privileged => None,
            Self::Principal(key) => Some(key),
        }
    }
}

#[derive(Clone, Debug)]
struct MemoryOwnership {
    visibility: &'static str,
    owner_principal: String,
    owner_display_name: String,
}

impl MemoryOwnership {
    fn public() -> Self {
        Self {
            visibility: VISIBILITY_PUBLIC,
            owner_principal: String::new(),
            owner_display_name: String::new(),
        }
    }

    fn privileged() -> Self {
        Self {
            visibility: VISIBILITY_PRIVILEGED,
            owner_principal: String::new(),
            owner_display_name: String::new(),
        }
    }

    fn principal(key: impl Into<String>, display_name: impl Into<String>) -> Self {
        let display_name = display_name.into();
        Self {
            visibility: VISIBILITY_PRINCIPAL,
            owner_principal: key.into(),
            owner_display_name: truncate_chars(&compact_line(&display_name), 128),
        }
    }
}

// EvictedTurn 下沉到 yunxi_base::memory_types：state 落库也要它，留在这里会和
// state 形成循环。原样再导出，`memory::EvictedTurn` 的写法不变。
pub use yunxi_base::memory_types::{EvictedTurn, MemoryRef};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MemoryKind {
    Fact,
    Diary,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MemoryOrigin {
    pub kind: String,
    pub platform: String,
    pub account_id: String,
    pub conversation_kind: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub sender_display_name: String,
    pub session_id: String,
    pub message_id: String,
}

impl MemoryOrigin {
    pub fn local(session_id: impl Into<String>) -> Self {
        Self {
            kind: "local".to_string(),
            session_id: session_id.into(),
            ..Self::default()
        }
    }

    fn principal_ownership(&self) -> Option<MemoryOwnership> {
        if self.kind != "platform"
            || self.platform.trim().is_empty()
            || self.account_id.trim().is_empty()
            || self.sender_id.trim().is_empty()
        {
            return None;
        }
        Some(MemoryOwnership::principal(
            PlatformPrincipal {
                platform: self.platform.clone(),
                account_id: self.account_id.clone(),
                user_id: self.sender_id.clone(),
            }
            .stable_key(),
            self.sender_display_name.trim(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub id: i64,
    pub kind: MemoryKind,
    pub content: String,
    pub score: f32,
    pub timestamp: String,
    pub source: String,
    pub retention: Option<String>,
    visibility: String,
    owner_principal: String,
    owner_display_name: String,
    subjects: String,
    source_episode_ids: Vec<i64>,
    origin_session_id: String,
}

#[derive(Debug, Clone, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub(crate) struct MemorySubject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) principal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ShortDiaryRecord {
    pub(crate) id: i64,
    pub(crate) created_at: String,
    pub(crate) user_message: String,
    pub(crate) assistant_message: String,
    pub(crate) force_long_term: bool,
    pub(crate) owner_principal: Option<String>,
    pub(crate) origin: MemoryOrigin,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ExistingMemoryRecord {
    pub(crate) id: i64,
    pub(crate) kind: String,
    pub(crate) content: String,
    pub(crate) truth_status: String,
    pub(crate) visibility: String,
    pub(crate) owner_principal: String,
    pub(crate) owner_display_name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct OrganizationBatch {
    pub(crate) database_id: String,
    pub(crate) generation: i64,
    pub(crate) diaries: Vec<ShortDiaryRecord>,
    pub(crate) existing: Vec<ExistingMemoryRecord>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OrganizedOutput {
    #[serde(default)]
    pub(crate) knowledge: Vec<KnowledgeAction>,
    #[serde(default)]
    pub(crate) long_diaries: Vec<LongDiaryDraft>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct KnowledgeAction {
    pub(crate) operation: String,
    #[serde(default)]
    pub(crate) target_id: Option<i64>,
    pub(crate) memory_type: String,
    pub(crate) content: String,
    pub(crate) truth_status: String,
    pub(crate) importance: i64,
    pub(crate) confidence: f64,
    #[serde(default)]
    pub(crate) visibility: String,
    #[serde(default)]
    pub(crate) subjects: Vec<MemorySubject>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) diary_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LongDiaryDraft {
    pub(crate) content: String,
    pub(crate) importance: i64,
    pub(crate) confidence: f64,
    #[serde(default)]
    pub(crate) visibility: String,
    #[serde(default)]
    pub(crate) subjects: Vec<MemorySubject>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) diary_ids: Vec<i64>,
}

impl MemoryStore {
    pub(crate) fn tombstone_snapshot(&self) -> Result<TombstoneSnapshot> {
        if !self.data_db.is_file() {
            return Ok(TombstoneSnapshot::default());
        }
        let mut conn = self.data_conn_existing()?;
        let tx = conn.transaction()?;
        let has_meta: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_meta')",
            [],
            |row| row.get(0),
        )?;
        if !has_meta {
            tx.commit()?;
            return Ok(TombstoneSnapshot::default());
        }
        // Old databases are readable before their next normal init migration.
        // Detect the missing column explicitly so unrelated SQL errors surface.
        let has_epoch: bool = tx.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('memory_meta')
                 WHERE name='tombstone_epoch'
             )",
            [],
            |row| row.get(0),
        )?;
        let epoch = if has_epoch {
            tx.query_row(
                "SELECT tombstone_epoch FROM memory_meta WHERE id=1",
                [],
                |row| row.get::<_, i64>(0),
            )?
        } else {
            0
        };
        let mut pairs = HashSet::new();
        let has_table: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_tombstones')",
            [],
            |row| row.get(0),
        )?;
        if has_table {
            pairs = {
                let mut stmt = tx.prepare("SELECT kind, id FROM memory_tombstones")?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })?;
                rows.collect::<std::result::Result<HashSet<_>, _>>()?
            };
        }
        tx.commit()?;
        Ok(TombstoneSnapshot { epoch, pairs })
    }

    pub(crate) fn tombstone_epoch(&self) -> Result<i64> {
        Ok(self.tombstone_snapshot()?.epoch)
    }

    /// Return whether any typed reference points at a memory row that has been
    /// tombstoned.  This is deliberately id-only: prompt/state carriers must
    /// never be reinterpreted by matching their copied text.
    pub fn memory_refs_are_tombstoned(&self, refs: &[MemoryRef]) -> Result<bool> {
        let refs = refs
            .iter()
            .filter(|memory_ref| {
                matches!(memory_ref.kind.as_str(), "fact" | "episode") && memory_ref.id > 0
            })
            .collect::<Vec<_>>();
        if refs.is_empty() || !self.data_db.is_file() {
            return Ok(false);
        }
        let conn = self.data_conn_existing()?;
        let has_table: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_tombstones')",
            [],
            |row| row.get(0),
        )?;
        if !has_table {
            return Ok(false);
        }
        for memory_ref in refs {
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM memory_tombstones WHERE kind = ?1 AND id = ?2)",
                params![memory_ref.kind, memory_ref.id],
                |row| row.get(0),
            )?;
            if exists {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn new(config: &AppConfig, paths: &YunXiPaths) -> Self {
        let data_dir = config.active_persona_memory_data_dir(paths).join("memory");
        let state_dir = config.active_persona_memory_state_dir(paths).join("memory");
        Self {
            config: config.memory_config().clone(),
            kb_config: config.plugins.knowledge_base.clone(),
            skills_dir: config.active_persona_skills_dir(paths),
            app_config: config.clone(),
            writes_enabled: true,
            access: MemoryAccess::Privileged,
            writer_principal: None,
            writer_display_name: String::new(),
            session_id: String::new(),
            data_db: data_dir.join("memory.db"),
            state_db: state_dir.join("evicted_context.db"),
            #[cfg(test)]
            overlap_hook: None,
        }
    }

    pub fn set_writes_enabled(&mut self, enabled: bool) {
        self.writes_enabled = enabled;
    }

    pub fn set_request_context(
        &mut self,
        access: MemoryAccess,
        writer_principal: Option<String>,
        writer_display_name: impl Into<String>,
    ) {
        self.access = access;
        self.writer_principal = writer_principal.filter(|value| !value.trim().is_empty());
        self.writer_display_name = writer_display_name.into().trim().to_string();
    }

    pub fn request_context(&self) -> (MemoryAccess, Option<String>, String) {
        (
            self.access.clone(),
            self.writer_principal.clone(),
            self.writer_display_name.clone(),
        )
    }

    pub fn with_request_context(
        mut self,
        access: MemoryAccess,
        writer_principal: Option<String>,
        writer_display_name: impl Into<String>,
    ) -> Self {
        self.set_request_context(access, writer_principal, writer_display_name);
        self
    }

    pub fn set_session_id(&mut self, session_id: impl AsRef<str>) {
        self.session_id = session_id.as_ref().trim().to_string();
    }

    pub fn with_session_id(mut self, session_id: impl AsRef<str>) -> Self {
        self.set_session_id(session_id);
        self
    }

    /// 写入时该盖哪个会话。显式设过就用显式的；没设就问回合的环境会话——
    /// 工具面的 store 在 `tools::build_registry` 里构造，那条路上没有会话
    /// 参数可加，而 `remember_fact` 恰恰跑在回合的 task-local 作用域里。
    fn write_session_id(&self) -> String {
        if !self.session_id.is_empty() {
            return self.session_id.clone();
        }
        yunxi_base::workspace::try_session()
            .map(|session| session.trim().to_string())
            .unwrap_or_default()
    }

    fn automatic_ownership(&self, origin: &MemoryOrigin) -> MemoryOwnership {
        origin
            .principal_ownership()
            .unwrap_or_else(MemoryOwnership::privileged)
    }

    fn writer_ownership(&self) -> MemoryOwnership {
        self.writer_principal
            .as_ref()
            .map(|principal| {
                MemoryOwnership::principal(principal.clone(), self.writer_display_name.clone())
            })
            .unwrap_or_else(MemoryOwnership::privileged)
    }

    pub fn apply_evicted_ownership(&self, turns: &mut [EvictedTurn]) {
        let ownership = self.writer_ownership();
        for turn in turns {
            turn.visibility = ownership.visibility.to_string();
            turn.owner_principal.clone_from(&ownership.owner_principal);
            turn.owner_display_name
                .clone_from(&ownership.owner_display_name);
        }
    }

    fn manual_fact_ownership(&self) -> MemoryOwnership {
        match self.writer_principal.as_ref() {
            Some(principal) => {
                MemoryOwnership::principal(principal.clone(), self.writer_display_name.clone())
            }
            None => MemoryOwnership::privileged(),
        }
    }

    pub fn init(&self) -> Result<()> {
        if let Some(parent) = self.data_db.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = self.state_db.parent() {
            std::fs::create_dir_all(parent)?;
        }
        init_data_db(&self.data_conn()?)?;
        init_state_db(&self.state_conn()?)?;
        self.decay_memories()?;
        Ok(())
    }

    pub fn identity(&self) -> Result<(String, i64)> {
        if !self.data_db.is_file() {
            self.init()?;
        }
        Ok(self.data_conn_existing()?.query_row(
            "SELECT database_id, generation FROM memory_meta WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    }

    fn init_existing(&self) -> Result<()> {
        let conn = self.data_conn_existing()?;
        init_data_db(&conn)?;
        self.decay_memories_with_conn(&conn)
    }

    fn data_conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.data_db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }

    fn data_conn_existing(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.data_db,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }

    fn state_conn(&self) -> Result<Connection> {
        let conn = Connection::open(&self.state_db)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        Ok(conn)
    }
}

#[cfg(test)]
mod tests;
