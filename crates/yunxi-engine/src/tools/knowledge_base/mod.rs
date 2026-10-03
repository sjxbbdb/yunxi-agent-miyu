mod dashboard;
mod files;
mod index;
mod search;
mod store;
#[cfg(test)]
use index::keyword_search_blocking;
pub(in crate::tools) use store::reject_non_kb_upload;

use search::*;
use store::*;

use super::{ToolCallContext, ToolRegistry, ToolSpec};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use yunxi_base::config::{AppConfig, KnowledgeBasePluginConfig};
use yunxi_base::paths::YunXiPaths;

// 08-21 Edit/Read 统一(用户裁定):upload/edit/remove/read 四个 CRUD 工具退场,
// 写走统一 `edit` 的 kb: 命名空间(apply_patch.rs 路由回本模块的 import_file/
// remove,索引钩子不绕过),读走统一 `read` 的 kb: 前缀。只留语义检索。
pub fn register(registry: &mut ToolRegistry, config: AppConfig, paths: YunXiPaths) {
    register_readonly(registry, config.clone(), paths.clone());
    // 08-21 二次裁定:知识库写入独立成 `kb` 工具(补丁语义,域名即广告)。
    if config.plugins.knowledge_base.upload_tool_enabled {
        crate::tools::apply_patch::register_kb(registry, config, paths);
    }
}

pub fn register_readonly(registry: &mut ToolRegistry, config: AppConfig, paths: YunXiPaths) {
    registry.register(ToolSpec::new_with_context(
        "search_knowledge_base",
        // 内容检索与文件名检索合并(08-17):同一个知识库的两种检索口径,
        // 拆成两个工具只是让 tools 数组多背一份外壳。by 缺省 content。
        "Search the local knowledge base. by=content (default) searches file contents and returns paths plus original snippets; by=name finds files by file name, directory, extension, or path fragment and returns relative paths. Use read_knowledge_base_file if snippets are insufficient. Mention paths only when useful or when the user asks.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search keywords, user question, or (with by=name) a file name / directory / extension / path fragment." },
                "by": { "type": "string", "enum": ["content", "name"], "description": "content searches text, name searches paths. Defaults to content." },
                "max_results": { "type": "integer", "description": "Optional result limit." }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        {
            let config = config.clone();
            let paths = paths.clone();
            move |args, _progress, context| {
                let config = config.clone();
                let paths = paths.clone();
                async move {
                    match args.get("by").and_then(Value::as_str).unwrap_or("content") {
                        "content" => {
                            tool_search_readonly(args, config, paths, context).await
                        }
                        "name" => {
                            tool_find_readonly(args, config, paths, context).await
                        }
                        other => bail!("unknown by: {other}; expected content or name"),
                    }
                }
            }
        },
    ));
}

pub struct KnowledgeBase {
    config: AppConfig,
    root: PathBuf,
    files_dir: PathBuf,
    meta_db: PathBuf,
    semantic_db: PathBuf,
    capability: KnowledgeCapability,
}

/// A local capability for one Knowledge Base view.  This type intentionally
/// stays private to the KB module: later host/registry plumbing may mint one,
/// but a model argument must never be able to construct or widen it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KnowledgeCapability {
    read_namespaces: HashSet<String>,
    write_namespaces: HashSet<String>,
    delete_namespaces: HashSet<String>,
    allow_bundled_replace: bool,
}

impl KnowledgeCapability {
    pub(crate) fn denied() -> Self {
        Self {
            read_namespaces: HashSet::new(),
            write_namespaces: HashSet::new(),
            delete_namespaces: HashSet::new(),
            allow_bundled_replace: false,
        }
    }

    pub(crate) fn owner_default() -> Self {
        Self {
            read_namespaces: [USER_KB_NAMESPACE, DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            write_namespaces: [USER_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            delete_namespaces: [USER_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allow_bundled_replace: false,
        }
    }

    pub(crate) fn bundled_maintenance() -> Self {
        Self {
            read_namespaces: [DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            write_namespaces: [DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            delete_namespaces: [DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allow_bundled_replace: true,
        }
    }

    #[cfg(test)]
    fn bundled_admin() -> Self {
        Self {
            write_namespaces: [USER_KB_NAMESPACE, DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            delete_namespaces: [USER_KB_NAMESPACE, DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allow_bundled_replace: true,
            ..Self::owner_default()
        }
    }

    pub(in crate::tools::knowledge_base) fn can_read(&self, namespace: &str) -> bool {
        self.read_namespaces.contains(namespace)
    }

    pub(in crate::tools::knowledge_base) fn can_write(&self, namespace: &str) -> bool {
        self.write_namespaces.contains(namespace)
    }

    pub(in crate::tools::knowledge_base) fn can_delete(&self, namespace: &str) -> bool {
        self.delete_namespaces.contains(namespace)
    }

    pub(in crate::tools::knowledge_base) fn can_replace_bundled(&self) -> bool {
        self.allow_bundled_replace
    }
}

impl KnowledgeBase {
    pub fn new(config: AppConfig, paths: YunXiPaths) -> Result<Self> {
        Self::with_capability(config, paths, KnowledgeCapability::owner_default())
    }

    pub(in crate::tools::knowledge_base) fn with_capability(
        config: AppConfig,
        paths: YunXiPaths,
        capability: KnowledgeCapability,
    ) -> Result<Self> {
        let root = kb_root_for(&config, &paths);
        let files_dir = root.join("files");
        let meta_db = root.join("kb_meta.db");
        let semantic_db = root.join("semantic_index.db");
        Ok(Self {
            config,
            root,
            files_dir,
            meta_db,
            semantic_db,
            capability,
        })
    }

    pub(crate) fn with_tool_context(
        config: AppConfig,
        paths: YunXiPaths,
        context: &ToolCallContext,
    ) -> Result<Self> {
        let capability = context
            .knowledge_capability()
            .map(|capability| (*capability).clone())
            .unwrap_or_else(KnowledgeCapability::denied);
        Self::with_capability(config, paths, capability)
    }

    pub(crate) fn bundled_maintenance(config: AppConfig, paths: YunXiPaths) -> Result<Self> {
        Self::with_capability(config, paths, KnowledgeCapability::bundled_maintenance())
    }

    pub fn init(&self) -> Result<()> {
        std::fs::create_dir_all(&self.files_dir)?;
        let conn = self.meta_conn()?;
        init_meta_db(&conn)?;
        let semantic = self.semantic_conn()?;
        init_semantic_db(&semantic)?;
        Ok(())
    }

    fn readonly_available(&self) -> bool {
        self.root.is_dir() && self.files_dir.is_dir() && self.meta_db.is_file()
    }

    pub async fn search(&self, query: &str, max_results: Option<usize>) -> Result<Value> {
        self.init()?;
        self.search_existing(query, max_results, true).await
    }

    pub async fn search_readonly(&self, query: &str, max_results: Option<usize>) -> Result<Value> {
        if !self.readonly_available() {
            return Ok(
                json!({"ok": true, "query": query, "total_matches": 0, "semantic_used": false, "results": []}),
            );
        }
        self.search_existing(query, max_results, self.semantic_db.is_file())
            .await
    }

    async fn search_existing(
        &self,
        query: &str,
        max_results: Option<usize>,
        allow_semantic: bool,
    ) -> Result<Value> {
        let limit = max_results
            .unwrap_or(self.config.plugins.knowledge_base.max_search_results)
            .clamp(1, 50);
        let mut results = self.keyword_search(query, limit).await?;
        let strongest = results.first().map(|item| item.score).unwrap_or(0.0);
        let mut semantic_used = false;
        if allow_semantic
            && self.config.plugins.knowledge_base.embedding_enabled
            && strongest
                < self
                    .config
                    .plugins
                    .knowledge_base
                    .keyword_strong_score_threshold
        {
            if let Ok(semantic) = self.semantic_search(query).await {
                semantic_used = !semantic.is_empty();
                merge_results(&mut results, semantic, limit);
            }
        }
        Ok(json!({
            "ok": true,
            "query": query,
            "total_matches": results.len(),
            "semantic_used": semantic_used,
            "results": results.iter().map(SearchResult::to_json).collect::<Vec<_>>(),
        }))
    }

    pub fn stats(&self) -> Result<Value> {
        self.init()?;
        let files = self.list()?;
        let semantic = self.semantic_conn()?;
        let mut stmt = semantic.prepare("SELECT namespace FROM semantic_chunks")?;
        let mut chunks = 0i64;
        for namespace in stmt.query_map([], |row| row.get::<_, String>(0))? {
            if self.capability.can_read(&namespace?) {
                chunks += 1;
            }
        }
        let embedder = self.embedder();
        Ok(json!({
            "ok": true,
            "root": self.root.display().to_string(),
            "files_dir": self.files_dir.display().to_string(),
            "files": files.len(),
            "total_size_kb": (files.iter().map(|file| file.size_bytes).sum::<i64>() as f64 / 1024.0 * 10.0).round() / 10.0,
            "semantic_chunks": chunks,
            "embedding_enabled": self.config.plugins.knowledge_base.embedding_enabled,
            // 与 WebUI 知识库面板同一口径（dashboard_overview）：能不能造出
            // Embedder。原先报的是 `plugins.knowledge_base.embedding_*` 旧字段，
            // 运行时不读它，用内置 bge 时这里是两个空串（09-23）。
            "embedding_configured": embedder.is_some(),
            "embedding_model_id": embedder
                .as_ref()
                .map(|embedder| embedder.model_id().to_string())
                .unwrap_or_default(),
        }))
    }
}

async fn tool_search_readonly(
    args: Value,
    config: AppConfig,
    paths: YunXiPaths,
    context: ToolCallContext,
) -> Result<String> {
    ensure_enabled(&config)?;
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if query.is_empty() {
        bail!("query is required")
    }
    let max_results = args
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    Ok(KnowledgeBase::with_tool_context(config, paths, &context)?
        .search_readonly(query, max_results)
        .await?
        .to_string())
}

async fn tool_find_readonly(
    args: Value,
    config: AppConfig,
    paths: YunXiPaths,
    context: ToolCallContext,
) -> Result<String> {
    ensure_enabled(&config)?;
    // 合并后统一用 query;file_name_query 保留为兼容别名。
    let query = args
        .get("query")
        .or_else(|| args.get("file_name_query"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if query.is_empty() {
        bail!("query is required")
    }
    let max_results = args
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|value| value as usize);
    Ok(KnowledgeBase::with_tool_context(config, paths, &context)?
        .find_by_name_readonly(query, max_results)?
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use yunxi_base::paths::YunXiPaths;

    /// `yunxi kb stats` 报的是实际在用的 embedding，不是旧字段
    /// `plugins.knowledge_base.embedding_*`（09-23：用内置 bge 时那两个是空串）。
    #[test]
    fn stats_report_the_effective_embedding_model() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        let provider = config.providers[0].id.clone();
        config.embedding.provider_id = provider.clone();
        config.embedding.model = "bge-m3".to_string();
        assert!(config
            .plugins
            .knowledge_base
            .embedding_provider_id
            .is_empty());
        let stats = KnowledgeBase::new(config, paths).unwrap().stats().unwrap();
        assert_eq!(stats["embedding_configured"], true);
        assert_eq!(stats["embedding_model_id"], format!("{provider}/bge-m3"));
    }

    #[test]
    fn old_kb_schema_opens_and_backfills_source_provenance() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let kb_root = paths.data_dir.join("kb");
        std::fs::create_dir_all(kb_root.join("files/default-kb/docs")).unwrap();
        std::fs::create_dir_all(kb_root.join("files/notes")).unwrap();
        std::fs::write(
            kb_root.join("files/default-kb/docs/pacman.md"),
            "pacman -Syu",
        )
        .unwrap();
        std::fs::write(kb_root.join("files/notes/local.md"), "local note").unwrap();

        let meta = Connection::open(kb_root.join("kb_meta.db")).unwrap();
        meta.execute_batch(
            "CREATE TABLE files (
                name TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                mtime REAL NOT NULL,
                content_sha256 TEXT NOT NULL,
                updated_at REAL NOT NULL
            );
            INSERT INTO files VALUES
                ('default-kb/docs/pacman.md', 'stale/default-kb/docs/pacman.md', 11, 0, 'default', 0),
                ('notes/local.md', 'stale/notes/local.md', 10, 0, 'local', 0);",
        )
        .unwrap();
        drop(meta);

        let semantic = Connection::open(kb_root.join("semantic_index.db")).unwrap();
        semantic
            .execute_batch(
                "CREATE TABLE semantic_chunks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    provider_id TEXT NOT NULL,
                    model TEXT NOT NULL,
                    file_name TEXT NOT NULL,
                    content_sha256 TEXT NOT NULL,
                    chunk_index INTEGER NOT NULL,
                    start_char INTEGER NOT NULL,
                    end_char INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    embedding_json TEXT NOT NULL,
                    created_at REAL NOT NULL
                );
                INSERT INTO semantic_chunks
                    (provider_id, model, file_name, content_sha256, chunk_index,
                     start_char, end_char, text, embedding_json, created_at)
                VALUES ('test', 'test', 'default-kb/docs/pacman.md', 'default', 0,
                        0, 11, 'pacman -Syu', '[1.0]', 0);",
            )
            .unwrap();
        drop(semantic);

        let kb = KnowledgeBase::new(AppConfig::default(), paths).unwrap();
        let records = kb.list().unwrap();
        let bundled = records
            .iter()
            .find(|record| record.name == "default-kb/docs/pacman.md")
            .unwrap();
        assert_eq!(bundled.provenance.namespace, DEFAULT_KB_NAMESPACE);
        assert_eq!(bundled.provenance.source_kind, DEFAULT_KB_SOURCE_KIND);
        assert_eq!(bundled.provenance.source_uri, DEFAULT_KB_SOURCE_URI);

        let user = records
            .iter()
            .find(|record| record.name == "notes/local.md")
            .unwrap();
        assert_eq!(user.provenance.namespace, USER_KB_NAMESPACE);
        assert_eq!(user.provenance.source_kind, USER_KB_SOURCE_KIND);
        assert_eq!(user.provenance.source_uri, USER_KB_SOURCE_URI);

        let semantic = kb.semantic_conn().unwrap();
        let semantic_metadata: (String, String, String, String) = semantic
            .query_row(
                "SELECT namespace, source_kind, source_uri, source_revision
                 FROM semantic_chunks WHERE file_name='default-kb/docs/pacman.md'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            semantic_metadata,
            (
                DEFAULT_KB_NAMESPACE.to_string(),
                DEFAULT_KB_SOURCE_KIND.to_string(),
                DEFAULT_KB_SOURCE_URI.to_string(),
                String::new(),
            )
        );
    }

    #[tokio::test]
    async fn default_and_user_imports_emit_provenance_without_changing_search() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;
        let kb =
            KnowledgeBase::with_capability(config, paths, KnowledgeCapability::bundled_admin())
                .unwrap();
        let bundled_source = temp.path().join("bundled.md");
        let user_source = temp.path().join("user.md");
        std::fs::write(&bundled_source, "pacman command reference").unwrap();
        std::fs::write(&user_source, "personal reference note").unwrap();
        kb.import_file(&bundled_source, "default-kb/commands/pacman.md")
            .unwrap();
        kb.import_file(&user_source, "notes/reference.md").unwrap();

        let bundled = kb.search("pacman", Some(5)).await.unwrap();
        let result = &bundled["results"][0];
        assert_eq!(result["path"], "default-kb/commands/pacman.md");
        assert_eq!(result["provenance"]["namespace"], DEFAULT_KB_NAMESPACE);
        assert_eq!(result["provenance"]["source_kind"], DEFAULT_KB_SOURCE_KIND);
        assert_eq!(result["provenance"]["source_uri"], DEFAULT_KB_SOURCE_URI);

        let user = kb.search("personal", Some(5)).await.unwrap();
        let result = &user["results"][0];
        assert_eq!(result["path"], "notes/reference.md");
        assert_eq!(result["provenance"]["namespace"], USER_KB_NAMESPACE);
        assert_eq!(result["provenance"]["source_kind"], USER_KB_SOURCE_KIND);
        assert_eq!(result["provenance"]["source_uri"], USER_KB_SOURCE_URI);
    }

    #[tokio::test]
    async fn namespace_capability_filters_reads_and_protects_bundled_writes() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;
        let bundled_source = temp.path().join("bundled.md");
        let user_source = temp.path().join("user.md");
        std::fs::write(&bundled_source, "pacman command reference").unwrap();
        std::fs::write(&user_source, "personal reference note").unwrap();

        let admin = KnowledgeBase::with_capability(
            config.clone(),
            paths.clone(),
            KnowledgeCapability::bundled_admin(),
        )
        .unwrap();
        admin
            .import_file(&bundled_source, "default-kb/commands/pacman.md")
            .unwrap();
        admin
            .import_file(&user_source, "notes/reference.md")
            .unwrap();

        let user_only = KnowledgeCapability {
            read_namespaces: [USER_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            write_namespaces: [USER_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            delete_namespaces: [USER_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            allow_bundled_replace: false,
        };
        let user_view =
            KnowledgeBase::with_capability(config.clone(), paths.clone(), user_only).unwrap();
        let files = user_view.list().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "notes/reference.md");
        assert_eq!(
            user_view.find_by_name_readonly("pacman", Some(10)).unwrap()["total_matches"],
            0
        );
        assert!(user_view.read_file("pacman.md", 1, Some(10)).is_err());
        assert!(user_view
            .read_file("default-kb/commands/pacman.md", 1, Some(10))
            .is_err());
        assert_eq!(
            user_view.search("pacman", Some(10)).await.unwrap()["total_matches"],
            0
        );
        assert_eq!(
            user_view.search("personal", Some(10)).await.unwrap()["total_matches"],
            1
        );

        let linux_only = KnowledgeCapability {
            read_namespaces: [DEFAULT_KB_NAMESPACE]
                .into_iter()
                .map(str::to_string)
                .collect(),
            write_namespaces: HashSet::new(),
            delete_namespaces: HashSet::new(),
            allow_bundled_replace: false,
        };
        let linux_view =
            KnowledgeBase::with_capability(config.clone(), paths.clone(), linux_only).unwrap();
        assert!(linux_view
            .read_file("pacman.md", 1, Some(10))
            .unwrap()
            .contains("pacman command reference"));

        let owner = KnowledgeBase::new(config, paths.clone()).unwrap();
        assert!(owner
            .import_file(&bundled_source, "default-kb/commands/other.md")
            .is_err());
        assert!(owner.remove("default-kb/commands/pacman.md").is_err());
    }

    #[test]
    fn bundled_replace_requires_internal_capability_and_replaces_only_bundled_rows() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let config = AppConfig::default();
        let source = temp.path().join("bundled.md");
        std::fs::write(&source, "old command").unwrap();
        let admin = KnowledgeBase::with_capability(
            config.clone(),
            paths.clone(),
            KnowledgeCapability::bundled_admin(),
        )
        .unwrap();
        admin.import_file(&source, "default-kb/old.md").unwrap();
        admin.import_file(&source, "notes/user.md").unwrap();

        let maintenance =
            KnowledgeBase::bundled_maintenance(config.clone(), paths.clone()).unwrap();
        assert!(maintenance
            .import_file(&source, "notes/should-not-write.md")
            .is_err());
        assert!(maintenance.remove("notes/user.md").is_err());
        assert!(maintenance
            .list()
            .unwrap()
            .iter()
            .all(|record| record.provenance.namespace == DEFAULT_KB_NAMESPACE));

        let replacement = temp.path().join("replacement");
        std::fs::create_dir_all(&replacement).unwrap();
        std::fs::write(replacement.join("new.md"), "new command").unwrap();
        admin.replace_default_files(&replacement).unwrap();
        let names = admin
            .list()
            .unwrap()
            .into_iter()
            .map(|record| record.name)
            .collect::<Vec<_>>();
        assert!(names.contains(&"default-kb/new.md".to_string()));
        assert!(!names.contains(&"default-kb/old.md".to_string()));
        assert!(names.contains(&"notes/user.md".to_string()));

        let owner = KnowledgeBase::new(config, paths).unwrap();
        assert!(owner.replace_default_files(&replacement).is_err());
    }

    #[test]
    fn bundled_replace_preflights_every_file_before_removing_active_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let config = AppConfig::default();
        let source = temp.path().join("bundled.md");
        std::fs::write(&source, "old command").unwrap();
        let admin =
            KnowledgeBase::with_capability(config, paths, KnowledgeCapability::bundled_admin())
                .unwrap();
        admin.import_file(&source, "default-kb/old.md").unwrap();

        let replacement = temp.path().join("replacement");
        std::fs::create_dir_all(&replacement).unwrap();
        std::fs::write(replacement.join("new.md"), "new command").unwrap();
        std::fs::write(replacement.join("broken.exe"), "not a knowledge file").unwrap();

        assert!(admin.replace_default_files(&replacement).is_err());
        assert!(admin
            .read_file("default-kb/old.md", 1, Some(10))
            .unwrap()
            .ends_with("old command"));
        assert!(admin.read_file("default-kb/new.md", 1, Some(10)).is_err());
    }

    #[test]
    fn bundled_replace_restores_snapshot_when_import_fails_after_removal() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let config = AppConfig::default();
        let source = temp.path().join("bundled.md");
        std::fs::write(&source, "old command").unwrap();
        let admin =
            KnowledgeBase::with_capability(config, paths, KnowledgeCapability::bundled_admin())
                .unwrap();
        admin.import_file(&source, "default-kb/old.md").unwrap();
        admin.import_file(&source, "notes/user.md").unwrap();

        let replacement = temp.path().join("replacement");
        std::fs::create_dir_all(&replacement).unwrap();
        std::fs::write(replacement.join("new.md"), "new command").unwrap();
        admin
            .meta_conn()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER fail_default_insert
                 BEFORE INSERT ON files
                 WHEN NEW.name = 'default-kb/new.md'
                 BEGIN SELECT RAISE(ABORT, 'injected import failure'); END;",
            )
            .unwrap();

        let error = admin.replace_default_files(&replacement).unwrap_err();
        assert!(error.to_string().contains("previous snapshot restored"));
        admin
            .meta_conn()
            .unwrap()
            .execute("DROP TRIGGER fail_default_insert", [])
            .unwrap();
        assert!(admin
            .read_file("default-kb/old.md", 1, Some(10))
            .unwrap()
            .ends_with("old command"));
        assert!(admin.read_file("default-kb/new.md", 1, Some(10)).is_err());
        assert!(admin
            .read_file("notes/user.md", 1, Some(10))
            .unwrap()
            .ends_with("old command"));
    }

    #[test]
    fn upload_guard_only_blocks_yunxi_own_assets() {
        // 正经资料照收。退回这个提交之前,这四篇全被挡在门外——正文里出现
        // config / memory / 配置 / 记忆 就够了。
        for (name, body) in [
            ("arch/fcitx5.md", "编辑 ~/.config/fcitx5/config 之后重启。"),
            ("linux/mm.md", "The kernel reclaims memory under pressure."),
            ("notes/prompt-engineering.md", "写 prompt 的几条经验。"),
            ("wiki/记忆宫殿.md", "记忆宫殿的用法。"),
        ] {
            reject_non_kb_upload(body, "", name)
                .unwrap_or_else(|error| panic!("{name} should be accepted: {error}"));
        }

        // YunXi 自己的资产仍然挡下。
        for name in [
            "skills/my-skill/SKILL.md",
            "personas/yunxi/persona.md",
            "config.toml",
        ] {
            assert!(
                reject_non_kb_upload("正文", "", name).is_err(),
                "{name} should be refused"
            );
        }
        assert!(reject_non_kb_upload(
            "---\nname: helper\ndescription: x\nmetadata:\n  yunxi.generated: \"true\"\n---\n",
            "",
            "notes/helper.md"
        )
        .is_err());
    }

    /// 09-09 实机事故留桩：数据目录搬家之后，库里那一列绝对路径就烂了。
    ///
    /// `~/.local/share/yunxi` → `~/.yunxi/data` 那次老布局迁移把文件搬过去了，却
    /// 没有重写 `files.path`。用户库里 6426 条记录全指着不存在的旧根，重建语义
    /// 索引时每个文件都是 `No such file or directory`，而面板上文件明明还在。
    /// 落盘位置必须由 `name` 现算——它是主键、也是相对 `files/` 的路径。
    #[test]
    fn a_stale_absolute_path_in_the_index_does_not_hide_the_file() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;
        let kb = KnowledgeBase::new(config, paths).unwrap();
        let source = temp.path().join("note.md");
        std::fs::write(&source, "海压竹枝低复举").unwrap();
        kb.import_file(&source, "wiki/note.md").unwrap();

        // 把那一列改成一个早已不存在的旧根，模拟迁移之后的库。
        {
            let conn = kb.meta_conn().unwrap();
            conn.execute(
                "UPDATE files SET path = ?1",
                rusqlite::params!["/nonexistent/old-root/kb/files/wiki/note.md"],
            )
            .unwrap();
        }

        let records = kb.list().unwrap();
        assert_eq!(records.len(), 1);
        let listed = std::path::Path::new(&records[0].path);
        assert!(
            listed.exists(),
            "列表给出的路径必须能打开，实际是 {}",
            listed.display()
        );
        assert_eq!(
            std::fs::read_to_string(listed).unwrap(),
            "海压竹枝低复举",
            "读到的必须是那份文件本身"
        );
    }

    /// 09-09 排查留桩：知识库不按人格分库，也不按人格改工具面。
    ///
    /// 用户转来的报告是「新人格使用创建知识库和加载知识库功能会报错」。同一份
    /// 配置只改 `active_persona`，两侧必须拿到同一套知识库工具、同一个库根，
    /// 写入与检索也走通——真正拦住那位用户的是 `reject_non_kb_upload` 的旧闸
    /// （扫全文关键词），不是人格。09-01 那次「内置资源绑出厂人格」只门控了
    /// 内置技能与内置脚本，这条断言把知识库钉在门外。
    #[tokio::test]
    async fn knowledge_base_is_not_scoped_by_persona() {
        let mut roots = Vec::new();
        for persona in ["", "自定义人格.md"] {
            let temp = tempfile::tempdir().unwrap();
            let paths = test_paths(temp.path());
            std::fs::create_dir_all(&paths.config_dir).unwrap();
            let mut config = AppConfig::default();
            config.plugins.knowledge_base.embedding_enabled = false;
            config.prompt.active_persona = persona.to_string();

            let registry = crate::tools::build_tool_registry(
                &config,
                &paths,
                yunxi_base::config::PersonaLane::Active,
                false,
            )
            .unwrap_or_else(|error| panic!("registry build failed for {persona:?}: {error:#}"));
            for tool in ["kb", "search_knowledge_base", "read"] {
                assert!(registry.contains(tool), "{persona:?} lost {tool}");
            }

            registry
                .call(
                    "kb",
                    "{\"patchText\":\"*** Begin Patch\\n*** Add File: kb:notes/a.md\\n+hello world\\n*** End Patch\\n\"}",
                )
                .await
                .unwrap_or_else(|error| panic!("kb write failed for {persona:?}: {error:#}"));
            let found = registry
                .call("search_knowledge_base", "{\"query\":\"hello\"}")
                .await
                .unwrap_or_else(|error| panic!("search failed for {persona:?}: {error:#}"));
            assert!(found.contains("notes/a.md"), "{persona:?}: {found}");

            roots.push(
                KnowledgeBase::new(config, paths.clone())
                    .unwrap()
                    .dashboard_root()
                    .strip_prefix(temp.path())
                    .unwrap()
                    .to_path_buf(),
            );
        }
        assert_eq!(roots[0], roots[1], "库根跟着人格走了");
    }

    #[tokio::test]
    async fn registry_capability_is_required_for_knowledge_base_reads() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;
        let admin = KnowledgeBase::with_capability(
            config.clone(),
            paths.clone(),
            KnowledgeCapability::bundled_admin(),
        )
        .unwrap();
        admin.init().unwrap();
        let source = temp.path().join("pacman.md");
        std::fs::write(&source, "pacman installs packages").unwrap();
        admin.import_file(&source, "default-kb/pacman.md").unwrap();

        let mut denied = ToolRegistry::new();
        denied.clear_knowledge_capability();
        register_readonly(&mut denied, config.clone(), paths.clone());
        let denied_result = denied
            .call("search_knowledge_base", r#"{"query":"pacman"}"#)
            .await
            .unwrap();
        assert!(
            denied_result.contains(r#""total_matches":0"#),
            "{denied_result}"
        );

        let mut owner = ToolRegistry::new();
        register_readonly(&mut owner, config, paths);
        let owner_result = owner
            .call("search_knowledge_base", r#"{"query":"pacman"}"#)
            .await
            .unwrap();
        assert!(
            owner_result.contains("default-kb/pacman.md"),
            "{owner_result}"
        );
    }

    /// G0-03 边界护栏：知识库文件及其语义索引删除只能触碰 KB 两个库，
    /// 不得误删当前人格的 facts、episodes 或 memory_embeddings。
    #[test]
    fn removing_a_knowledge_file_does_not_touch_memory_or_memory_embeddings() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;

        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory.init().unwrap();
        let fact_id = memory
            .remember_fact("删除知识库文件时必须保留的事实", "g0-test")
            .unwrap();
        let (database_id, generation) = memory.identity().unwrap();
        assert!(memory
            .process_after_turn(
                "删除边界测试的问题",
                "删除边界测试的回答",
                &yunxi_core::memory::MemoryOrigin::local("g0-kb-boundary"),
                &database_id,
                generation,
            )
            .unwrap());

        let memory_db = config
            .active_persona_memory_data_dir(&paths)
            .join("memory/memory.db");
        let memory_conn = rusqlite::Connection::open(&memory_db).unwrap();
        let episode_id: i64 = memory_conn
            .query_row(
                "SELECT id FROM episodes ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        // 模拟已生成的长期向量；这里只验证删除边界，不依赖本地模型。
        memory_conn
            .execute(
                "INSERT INTO memory_embeddings (kind, id, model, content_sha256, embedding, created_at)
                 VALUES ('fact', ?1, 'g0-test', 'fact-sha', x'00000000', '2026-09-30T00:00:00Z'),
                        ('episode', ?2, 'g0-test', 'episode-sha', x'00000000', '2026-09-30T00:00:00Z')",
                rusqlite::params![fact_id, episode_id],
            )
            .unwrap();
        let before_memory_counts = memory_conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM facts),
                    (SELECT COUNT(*) FROM episodes),
                    (SELECT COUNT(*) FROM memory_embeddings)",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(before_memory_counts, (1, 1, 2));
        drop(memory_conn);

        let kb = KnowledgeBase::new(config, paths).unwrap();
        kb.init().unwrap();
        let source = temp.path().join("reference.md");
        std::fs::write(&source, "knowledge boundary fixture").unwrap();
        kb.import_file(&source, "notes/reference.md").unwrap();
        kb.semantic_conn()
            .unwrap()
            .execute(
                "INSERT INTO semantic_chunks
                    (provider_id, model, file_name, content_sha256, chunk_index,
                     start_char, end_char, text, embedding_json, created_at)
                 VALUES ('g0-test', 'g0-test', 'notes/reference.md', 'kb-sha', 0,
                         0, 26, 'knowledge boundary fixture', '[]', 0)",
                [],
            )
            .unwrap();
        assert_eq!(kb.list().unwrap().len(), 1);
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name='notes/reference.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        kb.remove("notes/reference.md").unwrap();
        assert!(kb.list().unwrap().is_empty());
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name='notes/reference.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );

        let memory_conn = rusqlite::Connection::open(&memory_db).unwrap();
        let after_memory_counts = memory_conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM facts),
                    (SELECT COUNT(*) FROM episodes),
                    (SELECT COUNT(*) FROM memory_embeddings)",
                [],
                |row| Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            after_memory_counts, before_memory_counts,
            "知识库删除误伤了记忆库或记忆向量"
        );
    }

    #[test]
    fn remove_rolls_back_when_semantic_delete_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;

        let kb = KnowledgeBase::new(config, paths).unwrap();
        kb.init().unwrap();
        let source = temp.path().join("rollback.md");
        std::fs::write(&source, b"rollback fixture bytes").unwrap();
        kb.import_file(&source, "notes/rollback.md").unwrap();
        kb.semantic_conn()
            .unwrap()
            .execute(
                "INSERT INTO semantic_chunks
                    (provider_id, model, file_name, content_sha256, chunk_index,
                     start_char, end_char, text, embedding_json, created_at)
                 VALUES ('g0-test', 'g0-test', 'notes/rollback.md', 'kb-sha', 0,
                         0, 21, 'rollback fixture bytes', '[]', 0)",
                [],
            )
            .unwrap();
        let kb_file = kb.files_dir.join("notes/rollback.md");
        let before_bytes = std::fs::read(&kb_file).unwrap();

        kb.semantic_conn()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER injected_semantic_delete_failure
                 BEFORE DELETE ON semantic_chunks
                 BEGIN
                     SELECT RAISE(ABORT, 'injected semantic delete failure');
                 END;",
            )
            .unwrap();
        let error = kb.remove("notes/rollback.md").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected semantic delete failure"),
            "unexpected remove error: {error:#}"
        );
        assert_eq!(std::fs::read(&kb_file).unwrap(), before_bytes);
        assert_eq!(
            kb.meta_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE name='notes/rollback.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name='notes/rollback.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        kb.semantic_conn()
            .unwrap()
            .execute("DROP TRIGGER injected_semantic_delete_failure", [])
            .unwrap();
        kb.remove("notes/rollback.md").unwrap();
        assert!(!kb_file.exists());
        assert_eq!(
            kb.meta_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE name='notes/rollback.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name='notes/rollback.md'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn remove_prefix_rolls_back_when_semantic_delete_fails() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;

        let kb = KnowledgeBase::new(config, paths).unwrap();
        kb.init().unwrap();
        let first_source = temp.path().join("batch-first.md");
        let second_source = temp.path().join("batch-second.md");
        std::fs::write(&first_source, b"first batch fixture").unwrap();
        std::fs::write(&second_source, b"second batch fixture").unwrap();
        kb.import_file(&first_source, "notes/batch/first.md")
            .unwrap();
        kb.import_file(&second_source, "notes/batch/second.md")
            .unwrap();
        kb.semantic_conn()
            .unwrap()
            .execute_batch(
                "INSERT INTO semantic_chunks
                    (provider_id, model, file_name, content_sha256, chunk_index,
                     start_char, end_char, text, embedding_json, created_at)
                 VALUES ('g0-test', 'g0-test', 'notes/batch/first.md', 'first-sha', 0,
                         0, 19, 'first batch fixture', '[]', 0),
                        ('g0-test', 'g0-test', 'notes/batch/second.md', 'second-sha', 0,
                         0, 20, 'second batch fixture', '[]', 0);",
            )
            .unwrap();

        let first_file = kb.files_dir.join("notes/batch/first.md");
        let second_file = kb.files_dir.join("notes/batch/second.md");
        let first_bytes = std::fs::read(&first_file).unwrap();
        let second_bytes = std::fs::read(&second_file).unwrap();
        let mut before_entries = std::fs::read_dir(&kb.files_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        before_entries.sort_unstable();

        kb.semantic_conn()
            .unwrap()
            .execute_batch(
                "CREATE TRIGGER injected_semantic_prefix_delete_failure
                 BEFORE DELETE ON semantic_chunks
                 BEGIN
                     SELECT RAISE(ABORT, 'injected semantic prefix delete failure');
                 END;",
            )
            .unwrap();
        let error = kb.remove_prefix("notes/batch/").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected semantic prefix delete failure"),
            "unexpected remove_prefix error: {error:#}"
        );
        assert_eq!(std::fs::read(&first_file).unwrap(), first_bytes);
        assert_eq!(std::fs::read(&second_file).unwrap(), second_bytes);
        let mut after_failure_entries = std::fs::read_dir(&kb.files_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        after_failure_entries.sort_unstable();
        assert_eq!(
            after_failure_entries, before_entries,
            "rollback must not leave a tomb directory"
        );
        assert_eq!(
            kb.meta_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE name LIKE 'notes/batch/%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name LIKE 'notes/batch/%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            2
        );

        kb.semantic_conn()
            .unwrap()
            .execute("DROP TRIGGER injected_semantic_prefix_delete_failure", [])
            .unwrap();
        kb.remove_prefix("notes/batch/").unwrap();
        assert!(!first_file.exists());
        assert!(!second_file.exists());
        assert_eq!(
            kb.meta_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM files WHERE name LIKE 'notes/batch/%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM semantic_chunks WHERE file_name LIKE 'notes/batch/%'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }

    /// G0-03 反向边界护栏：记忆库的全量清理不能触碰知识库的源文件、元数据
    /// 或语义索引；两个域必须可以独立恢复。
    #[test]
    fn resetting_memory_does_not_touch_knowledge_base_source_or_indexes() {
        let temp = tempfile::tempdir().unwrap();
        let paths = test_paths(temp.path());
        let mut config = AppConfig::default();
        config.plugins.knowledge_base.embedding_enabled = false;

        let kb = KnowledgeBase::new(config.clone(), paths.clone()).unwrap();
        kb.init().unwrap();
        let source = temp.path().join("reference.md");
        std::fs::write(&source, "memory and knowledge are separate").unwrap();
        kb.import_file(&source, "notes/reference.md").unwrap();
        kb.semantic_conn()
            .unwrap()
            .execute(
                "INSERT INTO semantic_chunks
                    (provider_id, model, file_name, content_sha256, chunk_index,
                     start_char, end_char, text, embedding_json, created_at)
                 VALUES ('g0-test', 'g0-test', 'notes/reference.md', 'kb-sha', 0,
                         0, 33, 'memory and knowledge are separate', '[]', 0)",
                [],
            )
            .unwrap();
        let kb_file = kb.files_dir.join("notes/reference.md");
        let before_bytes = std::fs::read(&kb_file).unwrap();
        let before_names = kb
            .list()
            .unwrap()
            .into_iter()
            .map(|record| record.name)
            .collect::<Vec<_>>();
        let before_semantic_count: i64 = kb
            .semantic_conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM semantic_chunks", [], |row| row.get(0))
            .unwrap();

        let memory = yunxi_core::memory::MemoryStore::new(&config, &paths);
        memory
            .remember_fact("这条记忆会被 reset_all 清掉", "g0-test")
            .unwrap();
        memory.reset_all().unwrap();

        assert_eq!(std::fs::read(&kb_file).unwrap(), before_bytes);
        assert_eq!(
            kb.list()
                .unwrap()
                .into_iter()
                .map(|record| record.name)
                .collect::<Vec<_>>(),
            before_names,
            "memory reset changed KB metadata"
        );
        assert_eq!(
            kb.semantic_conn()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM semantic_chunks", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            before_semantic_count,
            "memory reset changed KB semantic index"
        );
    }

    pub(super) fn test_paths(root: &Path) -> YunXiPaths {
        YunXiPaths {
            root_dir: root.to_path_buf(),
            config_dir: root.join("config"),
            config_file: root.join("config/config.jsonc"),
            skills_dir: root.join("config/skills"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
            state_dir: root.join("state"),
            pictures_dir: root.join("pictures"),
            fish_hook_file: root.join("fish/conf.d/yunxi.fish"),
            bash_hook_file: root.join("config/shell/bash-hook.sh"),
            zsh_hook_file: root.join("config/shell/zsh-hook.zsh"),
            scripts_dir: root.join("config/scripts"),
            system_scripts_dir: PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod scaling_probe {
    use super::*;
    use std::time::Instant;

    /// 量尺，不是断言：`cargo test --lib knowledge_base::scaling_probe -- --ignored --nocapture`
    ///
    /// keyword_search 对库里**每个**文件做「整读 + 整份 lowercase 拷贝」，
    /// 而且是在 `async fn search_existing` 里同步跑——这段时间 tokio worker
    /// 是卡住的。这里量的就是那段卡住有多长。
    #[test]
    #[ignore]
    fn keyword_search_scaling() {
        println!("\n  文件数  每文件KB   库总量MB   搜索耗时(ms)");
        for (files, kb_each) in [(20usize, 32usize), (50, 32), (100, 32), (200, 32)] {
            let temp = tempfile::tempdir().unwrap();
            let paths = super::tests::test_paths(temp.path());
            let kb = KnowledgeBase::new(AppConfig::default(), paths).unwrap();
            // 内容里不含查询词,走的是「全扫一遍都没命中」这条最坏路径
            let body = "lorem ipsum dolor sit amet ".repeat(kb_each * 1024 / 27);
            for index in 0..files {
                let source = temp.path().join(format!("doc{index}.md"));
                std::fs::write(&source, &body).unwrap();
                kb.import_file(&source, &format!("docs/doc{index}.md"))
                    .unwrap();
            }
            let start = Instant::now();
            let found = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(kb.keyword_search("需要检索的关键词", 5))
                .unwrap();
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(found);
            let total_mb = (files * kb_each) as f64 / 1024.0;
            println!("  {files:>6}  {kb_each:>8}  {total_mb:>9.1}  {ms:>13.1}");
        }
    }

    /// 真正要证明的不是搜索本身变快了（活儿一样多），而是**搜索期间别的
    /// 异步任务还转不转**。单 worker 运行时上放一个 5ms 心跳，量它被堵住的
    /// 最长间隔：同步跑 = 堵满整个搜索时长，spawn_blocking = 基本不堵。
    #[test]
    #[ignore]
    fn keyword_search_does_not_freeze_the_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let paths = super::tests::test_paths(temp.path());
        let kb = KnowledgeBase::new(AppConfig::default(), paths).unwrap();
        let body = "lorem ipsum dolor sit amet ".repeat(200 * 1024 / 27);
        for index in 0..30 {
            let source = temp.path().join(format!("doc{index}.md"));
            std::fs::write(&source, &body).unwrap();
            kb.import_file(&source, &format!("docs/doc{index}.md"))
                .unwrap();
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();

        for (label, blocking) in [
            ("同步跑（改前的做法）", true),
            ("spawn_blocking（现在）", false),
        ] {
            let gap = runtime.block_on(async {
                let worst = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                let probe = worst.clone();
                let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let halt = stop.clone();
                let ticker = tokio::spawn(async move {
                    let mut last = Instant::now();
                    while !halt.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        let gap = last.elapsed().as_millis() as u64;
                        probe.fetch_max(gap, std::sync::atomic::Ordering::Relaxed);
                        last = Instant::now();
                    }
                });
                tokio::task::yield_now().await;
                // 搜索必须也在 spawn 出来的任务里跑,才会和心跳抢同一个
                // worker——`block_on` 的 future 跑在调用线程上,放这儿量不出
                // 任何东西(第一版探针就是这么白跑的)。
                let records = kb.list().unwrap();
                let search = tokio::spawn(async move {
                    if blocking {
                        // 改前的形状:在 async 上下文里直接同步跑
                        let found =
                            keyword_search_blocking(records, "需要检索的关键词", 5, 200, 200);
                        std::hint::black_box(found.unwrap());
                    } else {
                        let found = tokio::task::spawn_blocking(move || {
                            keyword_search_blocking(records, "需要检索的关键词", 5, 200, 200)
                        })
                        .await
                        .unwrap();
                        std::hint::black_box(found.unwrap());
                    }
                });
                let _ = search.await;
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let _ = ticker.await;
                worst.load(std::sync::atomic::Ordering::Relaxed)
            });
            println!("  {label:<26} 心跳最长被堵 {gap:>5} ms");
        }
    }
}
