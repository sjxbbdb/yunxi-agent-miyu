//! 文件的增删改查。
//!
//! 路径全部过 `safe_file_path`——名字来自模型，必须限制在库根之内。
//! `*_readonly` 与 `*_existing` 是三套并行的入口：只读模式不许写、
//! 「existing」变体要求库已存在而不是顺手建一个。

use crate::tools::knowledge_base::*;

struct DefaultFileBackup {
    name: String,
    source_revision: String,
}

struct ReplacementBackup {
    root: tempfile::TempDir,
    files: Vec<DefaultFileBackup>,
}

impl KnowledgeBase {
    pub async fn add_path(&self, source: &Path) -> Result<Vec<String>> {
        self.init()?;
        let mut added = Vec::new();
        if source.is_dir() {
            let root_name = source
                .file_name()
                .and_then(|name| name.to_str())
                .context("source directory has no valid directory name")?;
            for file in collect_files(source)? {
                let rel = file.strip_prefix(source).unwrap_or(&file);
                let name = normalize_relative_path(&format!(
                    "{}/{}",
                    root_name,
                    rel.display().to_string().replace('\\', "/")
                ))?;
                if let Ok(name) = self.import_file(&file, &name) {
                    added.push(name);
                }
            }
        } else {
            let name = normalize_relative_path(
                source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("source file has no valid file name")?,
            )?;
            added.push(self.import_file(source, &name)?);
        }
        self.spawn_embedding_reindex()?;
        Ok(added)
    }

    pub fn replace_default_files(&self, source: &Path) -> Result<Vec<String>> {
        self.replace_default_files_with_revision(source, "")
    }

    /// Replace the bundled namespace and stamp every imported row with the
    /// source revision that produced the snapshot.  User imports retain the
    /// empty revision and are never rewritten by this path.
    pub(crate) fn replace_default_files_with_revision(
        &self,
        source: &Path,
        source_revision: &str,
    ) -> Result<Vec<String>> {
        if !self.capability.can_replace_bundled() {
            bail!("knowledge base bundled namespace requires an internal capability")
        }
        self.init()?;
        // Validate the complete incoming snapshot before removing the active one.
        // A single malformed/unsupported file must not turn a failed update into an
        // empty or partial default namespace.
        let imports = self.prepare_default_imports(source)?;
        let backup = self.create_replacement_backup()?;
        let result = (|| -> Result<Vec<String>> {
            self.remove_prefix("default-kb/")?;
            let mut added = Vec::new();
            for (file, name) in imports {
                added.push(self.import_file_with_revision(&file, &name, source_revision)?);
            }
            self.spawn_embedding_reindex()?;
            Ok(added)
        })();
        match result {
            Ok(added) => Ok(added),
            Err(error) => {
                if let Err(recovery) = self.restore_replacement_backup(&backup) {
                    return Err(error.context(format!(
                        "default knowledge-base update failed and snapshot recovery failed: {recovery:#}"
                    )));
                }
                Err(error
                    .context("default knowledge-base update failed; previous snapshot restored"))
            }
        }
    }

    pub(crate) fn bundled_snapshot_matches(
        &self,
        source: &Path,
        source_revision: &str,
    ) -> Result<bool> {
        let expected = self.prepare_default_imports(source)?;
        let actual = self
            .list_existing()?
            .into_iter()
            .filter(|record| record.name == "default-kb" || record.name.starts_with("default-kb/"))
            .collect::<Vec<_>>();
        if expected.len() != actual.len() {
            return Ok(false);
        }
        let actual = actual
            .into_iter()
            .map(|record| (record.name.clone(), record))
            .collect::<std::collections::HashMap<_, _>>();
        for (file, name) in expected {
            let Some(record) = actual.get(&name) else {
                return Ok(false);
            };
            let bytes = std::fs::read(file)?;
            if record.content_sha256 != sha256_hex(&bytes)
                || record.provenance.source_revision != source_revision
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn create_replacement_backup(&self) -> Result<ReplacementBackup> {
        let root = tempfile::tempdir_in(&self.root)?;
        let mut files = Vec::new();
        for record in self
            .list_existing()?
            .into_iter()
            .filter(|record| record.name == "default-kb" || record.name.starts_with("default-kb/"))
        {
            let source = self.safe_file_path(&record.name)?;
            let destination = root.path().join("files").join(&record.name);
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&source, &destination).with_context(|| {
                format!(
                    "failed to back up default knowledge-base file {}",
                    record.name
                )
            })?;
            files.push(DefaultFileBackup {
                name: record.name,
                source_revision: record.provenance.source_revision,
            });
        }
        Ok(ReplacementBackup { root, files })
    }

    fn restore_replacement_backup(&self, backup: &ReplacementBackup) -> Result<()> {
        self.remove_prefix("default-kb/")?;
        // import_file writes bytes before the metadata upsert. If that upsert
        // fails, the orphan has no row for remove_prefix to discover; clear only
        // the bundled directory before restoring, never the user namespace.
        let default_files = self.files_dir.join("default-kb");
        if default_files.exists() {
            std::fs::remove_dir_all(default_files)?;
        }
        for file in &backup.files {
            let source = backup.root.path().join("files").join(&file.name);
            self.import_file_with_revision(&source, &file.name, &file.source_revision)?;
        }
        // The failed replacement removed only bundled semantic rows. Rebuild them
        // asynchronously from the restored default files; user vectors stay intact.
        self.spawn_embedding_reindex()?;
        Ok(())
    }

    fn prepare_default_imports(&self, source: &Path) -> Result<Vec<(PathBuf, String)>> {
        let mut files = collect_files(source)?;
        files.sort();
        let mut imports = Vec::with_capacity(files.len());
        for file in files {
            let rel = file.strip_prefix(source).unwrap_or(&file);
            let rel = rel.display().to_string().replace('\\', "/");
            let name = normalize_relative_path(&format!("default-kb/{rel}"))?;
            let bytes = std::fs::read(&file).with_context(|| {
                format!(
                    "failed to read default knowledge-base file {}",
                    file.display()
                )
            })?;
            self.validate_file(&name, &bytes).with_context(|| {
                format!("invalid default knowledge-base file {}", file.display())
            })?;
            imports.push((file, name));
        }
        if imports.is_empty() {
            bail!("default knowledge base snapshot contains no files")
        }
        Ok(imports)
    }

    pub fn list(&self) -> Result<Vec<FileRecord>> {
        self.init()?;
        self.list_existing()
    }

    /// 落盘位置一律由 `name` 现算，**不读库里那一列绝对路径**。
    ///
    /// `files.path` 存的是导入当刻的绝对路径，而数据目录是会搬家的：
    /// `~/.local/share/yunxi` → `~/.yunxi/data` 那次老布局迁移把文件搬了、却没有
    /// 重写这一列。于是库里 6426 条记录全指着一个不存在的旧根，重建语义索引时
    /// 每个文件都是 `No such file or directory`，而面板上文件明明还在
    /// （09-09 用户实机，日志逐行可查）。
    ///
    /// `name` 是主键、也是相对 `files/` 的路径，永远跟着当前根走。那一列因此是
    /// 冗余的，留着只为不动 schema。
    pub(in crate::tools::knowledge_base) fn list_existing(&self) -> Result<Vec<FileRecord>> {
        let conn = self.meta_conn()?;
        let mut stmt = conn.prepare(
            "SELECT name, size_bytes, content_sha256, namespace, source_kind, source_uri, source_revision FROM files ORDER BY name",
        )?;
        let files_dir = self.files_dir.clone();
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                SourceMetadata {
                    namespace: row.get(3)?,
                    source_kind: row.get(4)?,
                    source_uri: row.get(5)?,
                    source_revision: row.get(6)?,
                },
            ))
        })?;
        let mut records = Vec::new();
        for row in rows {
            let (name, size_bytes, content_sha256, provenance) = row?;
            if !self.capability.can_read(&provenance.namespace) {
                continue;
            }
            records.push(FileRecord {
                path: files_dir.join(&name).display().to_string(),
                name,
                size_bytes,
                content_sha256,
                provenance,
            });
        }
        Ok(records)
    }

    pub fn find_by_name(&self, query: &str, max_results: Option<usize>) -> Result<Value> {
        self.init()?;
        self.find_by_name_existing(query, max_results)
    }

    pub fn find_by_name_readonly(&self, query: &str, max_results: Option<usize>) -> Result<Value> {
        if !self.readonly_available() {
            return Ok(json!({"ok": true, "query": query, "total_matches": 0, "results": []}));
        }
        self.find_by_name_existing(query, max_results)
    }

    pub(in crate::tools::knowledge_base) fn find_by_name_existing(
        &self,
        query: &str,
        max_results: Option<usize>,
    ) -> Result<Value> {
        let limit = max_results
            .unwrap_or(self.config.plugins.knowledge_base.max_search_results)
            .clamp(1, 50);
        let mut results = Vec::new();
        for record in self.list()? {
            let (score, reason) = score_file_name(query, &record.name);
            if score <= 0.0 {
                continue;
            }
            results.push(json!({
                "path": record.name,
                "name": file_name(&record.name),
                "directory": directory_name(&record.name),
                "score": score,
                "match_reason": reason,
                "size_kb": (record.size_bytes as f64 / 1024.0 * 10.0).round() / 10.0,
            }));
        }
        results.sort_by(|a, b| {
            b.get("score")
                .and_then(Value::as_f64)
                .unwrap_or_default()
                .partial_cmp(&a.get("score").and_then(Value::as_f64).unwrap_or_default())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(json!({
            "ok": true,
            "query": query,
            "total_matches": results.len(),
            "results": results,
        }))
    }

    pub fn read_file(
        &self,
        name: &str,
        start_line: usize,
        max_lines: Option<usize>,
    ) -> Result<String> {
        self.init()?;
        self.read_file_existing(name, start_line, max_lines, true)
    }

    /// Resolve a caller-supplied path to a stored record name, tolerating
    /// omitted directory prefixes (e.g. `4. xx/文件.md` for
    /// `default-kb/kb/4. xx/文件.md`): exact match first, then a unique
    /// suffix match; otherwise fail with concrete candidates so the model
    /// can self-correct in one step.
    pub(in crate::tools::knowledge_base) fn resolve_stored_name(
        &self,
        rel: &str,
    ) -> Result<String> {
        let records = self.list_existing()?;
        if records.iter().any(|record| record.name == rel) {
            return Ok(rel.to_string());
        }
        let suffix = format!("/{rel}");
        let matches = records
            .iter()
            .filter(|record| record.name.ends_with(&suffix))
            .map(|record| record.name.clone())
            .collect::<Vec<_>>();
        match matches.len() {
            1 => Ok(matches.into_iter().next().unwrap()),
            0 => {
                let mut scored = records
                    .iter()
                    .map(|record| (score_file_name(rel, &record.name).0, &record.name))
                    .filter(|(score, _)| *score > 0.0)
                    .collect::<Vec<_>>();
                scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                let hints = scored
                    .iter()
                    .take(3)
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>();
                if hints.is_empty() {
                    bail!("knowledge base file not found: {rel}")
                }
                bail!(
                    "knowledge base file not found: {rel}；相近的文件: {}",
                    hints.join("、")
                )
            }
            _ => bail!(
                "path {rel} matches multiple knowledge base files: {}；请用完整路径",
                matches.join("、")
            ),
        }
    }

    /// Authorize a `read` tool `kb:` path before the generic filesystem reader
    /// receives its absolute form.  The generic reader has no KB namespace
    /// semantics, so this check must happen while the path is still a KB name.
    pub(crate) fn resolve_read_path(&self, name: &str) -> Result<PathBuf> {
        let mut rel = normalize_relative_path(name)?;
        let mut path = self.existing_file_path(&rel).unwrap_or_default();
        if !path.exists() {
            rel = self.resolve_stored_name(&rel)?;
            path = self.existing_file_path(&rel)?;
        }
        if !path.exists()
            || !self
                .capability
                .can_read(&SourceMetadata::for_file(&rel).namespace)
        {
            bail!("knowledge base file not found: {rel}")
        }
        Ok(path)
    }

    pub(in crate::tools::knowledge_base) fn read_file_existing(
        &self,
        name: &str,
        start_line: usize,
        max_lines: Option<usize>,
        create_parent: bool,
    ) -> Result<String> {
        let mut rel = normalize_relative_path(name)?;
        let build_path = |rel: &str| -> Result<PathBuf> {
            if create_parent {
                self.safe_file_path(rel)
            } else {
                // Fails with ENOENT when the parent directory itself is
                // missing — treat that the same as "file not found" so the
                // prefix-tolerant resolution below still gets its chance.
                self.existing_file_path(rel)
            }
        };
        let mut path = build_path(&rel).unwrap_or_default();
        if !path.exists() {
            rel = self.resolve_stored_name(&rel)?;
            path = build_path(&rel)?;
        }
        if !path.exists() {
            bail!("knowledge base file not found: {rel}")
        }
        let resolved_namespace = SourceMetadata::for_file(&rel).namespace;
        if !self.capability.can_read(&resolved_namespace) {
            // Do not disclose that a protected namespace contains a matching
            // path; callers see the same not-found shape as a missing file.
            bail!("knowledge base file not found: {rel}")
        }
        let content = std::fs::read_to_string(&path)?;
        let start = start_line.max(1);
        let max_lines = max_lines
            .unwrap_or(self.config.plugins.knowledge_base.max_read_lines)
            .clamp(1, 5000);
        let mut total = 0usize;
        let mut selected = Vec::new();
        for (index, line) in content.lines().enumerate() {
            let line_no = index + 1;
            total = line_no;
            if line_no >= start && selected.len() < max_lines {
                selected.push(line);
            }
        }
        if start > total.max(1) {
            return Ok(format!(
                "=== {rel} | start_line {start} out of range / {total} lines ==="
            ));
        }
        let end = (start + max_lines - 1).min(total);
        let mut output = format!("=== {rel} | lines {start}-{end} / {total} ===\n");
        output.push_str(&selected.join("\n"));
        if end < total {
            output.push_str(&format!(
                "\n\n... {remaining} more lines; continue with start_line={next}",
                remaining = total - end,
                next = end + 1
            ));
        }
        Ok(output)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        self.init()?;
        let rel = normalize_relative_path(name)?;
        let namespace = SourceMetadata::for_file(&rel).namespace;
        if !self.capability.can_delete(&namespace) {
            bail!("knowledge base namespace is not deletable: {namespace}")
        }
        let path = self.safe_file_path(&rel)?;
        // `exists` follows symlinks (and reports a dangling link as absent), while
        // remove_file historically removed the link itself.  Keep that behavior,
        // but stage the directory entry with rename so a DB failure can restore it.
        let file_existed = std::fs::symlink_metadata(&path).is_ok();
        let tomb_dir = if file_existed {
            Some(tempfile::tempdir_in(&self.files_dir)?)
        } else {
            None
        };
        let tomb_path = tomb_dir.as_ref().map(|dir| dir.path().join("payload"));
        if let Some(tomb_path) = &tomb_path {
            if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_dir())
            {
                // Match remove_file's error for directories instead of moving one
                // into the tomb and changing the old API's behavior.
                std::fs::remove_file(&path)?;
            }
            std::fs::rename(&path, tomb_path)?;
        }

        let result = (|| -> Result<()> {
            let mut conn = self.meta_conn()?;
            let mut semantic = self.semantic_conn()?;
            let meta_tx = conn.transaction()?;
            let semantic_tx = semantic.transaction()?;
            let deleted_rows = meta_tx.execute("DELETE FROM files WHERE name=?1", params![rel])?;
            let deleted_semantic_rows = semantic_tx.execute(
                "DELETE FROM semantic_chunks WHERE file_name=?1",
                params![rel],
            )?;
            // 目标本来就不存在时必须报错:静默返回 ok 会让模型以为删除成功。
            // 若文件已丢失但仍有任一索引行，仍允许把孤儿索引清掉。
            if !file_existed && deleted_rows == 0 && deleted_semantic_rows == 0 {
                anyhow::bail!("knowledge base file not found: {rel}");
            }
            // 两个事务属于不同 SQLite 文件，提交不是跨库原子的；把所有可失败的
            // execute 放在 commit 前，保证触发器/prepare 失败时两边都由 drop 回滚。
            meta_tx.commit()?;
            semantic_tx.commit()?;
            Ok(())
        })();

        if let Err(error) = result {
            if let (Some(tomb_path), true) = (tomb_path.as_ref(), file_existed) {
                if let Err(restore_error) = std::fs::rename(tomb_path, &path) {
                    return Err(error.context(format!(
                        "failed to restore knowledge base file after deletion error: {restore_error}"
                    )));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::tools::knowledge_base) fn remove_prefix(&self, prefix: &str) -> Result<()> {
        let names = {
            let conn = self.meta_conn()?;
            let mut stmt = conn.prepare("SELECT name FROM files WHERE name LIKE ?1")?;
            let names = stmt
                .query_map(params![format!("{prefix}%")], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            names
                .into_iter()
                .filter(|name| {
                    let namespace = SourceMetadata::for_file(name).namespace;
                    self.capability.can_delete(&namespace)
                })
                .collect::<Vec<_>>()
        };
        // Keep the historical empty-prefix no-op: do not create a tomb when there
        // is nothing to delete.
        if names.is_empty() {
            return Ok(());
        }

        // The tomb lives under files_dir, so every rename stays on the same file
        // system.  A single tomb set covers the whole batch and is removed by
        // TempDir after either a successful commit or rollback.
        let tomb_dir = tempfile::tempdir_in(&self.files_dir)?;
        let mut moved = Vec::new();
        let result = (|| -> Result<()> {
            for name in &names {
                let path = self.safe_file_path(name)?;
                match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        if metadata.file_type().is_dir() {
                            // Keep remove_file's old error semantics for a directory.
                            std::fs::remove_file(&path)?;
                        }
                        let tomb_path = tomb_dir.path().join(name);
                        if let Some(parent) = tomb_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::rename(&path, &tomb_path)?;
                        moved.push((path, tomb_path));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }

            let mut meta = self.meta_conn()?;
            let mut semantic = self.semantic_conn()?;
            let meta_tx = meta.transaction()?;
            let semantic_tx = semantic.transaction()?;
            meta_tx.execute(
                "DELETE FROM files WHERE name LIKE ?1",
                params![format!("{prefix}%")],
            )?;
            semantic_tx.execute(
                "DELETE FROM semantic_chunks WHERE file_name LIKE ?1",
                params![format!("{prefix}%")],
            )?;
            // These are separate SQLite files, so their commits are not
            // cross-database atomic. All fallible statements run before commit;
            // on statement/prepare failure both transactions drop and roll back.
            meta_tx.commit()?;
            semantic_tx.commit()?;
            Ok(())
        })();

        if let Err(error) = result {
            let mut restore_error = None;
            for (path, tomb_path) in moved.iter().rev() {
                if let Err(error) = std::fs::rename(tomb_path, path) {
                    restore_error.get_or_insert(error);
                }
            }
            if let Some(restore_error) = restore_error {
                return Err(error.context(format!(
                    "failed to restore knowledge base file after prefix deletion error: {restore_error}"
                )));
            }
            return Err(error);
        }
        Ok(())
    }

    pub(in crate::tools) fn import_file(&self, source: &Path, name: &str) -> Result<String> {
        self.import_file_with_revision(source, name, "")
    }

    pub(crate) fn import_file_with_revision(
        &self,
        source: &Path,
        name: &str,
        source_revision: &str,
    ) -> Result<String> {
        let name = normalize_relative_path(name)?;
        let namespace = SourceMetadata::for_file(&name).namespace;
        if !self.capability.can_write(&namespace) {
            bail!("knowledge base namespace is not writable: {namespace}")
        }
        // 先看元数据再整读:超大文件不该先撑满 RAM 再被大小校验拒绝。
        let max_bytes = self.config.plugins.knowledge_base.max_file_size_kb * 1024;
        let size = std::fs::metadata(source)?.len();
        if size > max_bytes as u64 {
            bail!("file too large: {size} bytes");
        }
        let bytes = std::fs::read(source)?;
        self.validate_file(&name, &bytes)?;
        let dest = self.safe_file_path(&name)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &bytes)?;
        let hash = sha256_hex(&bytes);
        let mtime = unix_time(std::fs::metadata(&dest)?.modified()?);
        let mut provenance = SourceMetadata::for_file(&name);
        if provenance.namespace == DEFAULT_KB_NAMESPACE {
            provenance.source_revision = source_revision.trim().to_string();
        }
        let conn = self.meta_conn()?;
        init_meta_db(&conn)?;
        conn.execute(
            "INSERT INTO files (name, path, size_bytes, mtime, content_sha256, updated_at, namespace, source_kind, source_uri, source_revision) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) ON CONFLICT(name) DO UPDATE SET path=excluded.path, size_bytes=excluded.size_bytes, mtime=excluded.mtime, content_sha256=excluded.content_sha256, updated_at=excluded.updated_at, namespace=excluded.namespace, source_kind=excluded.source_kind, source_uri=excluded.source_uri, source_revision=excluded.source_revision",
            params![
                &name,
                dest.display().to_string(),
                bytes.len() as i64,
                mtime,
                hash,
                now_secs(),
                provenance.namespace,
                provenance.source_kind,
                provenance.source_uri,
                provenance.source_revision,
            ],
        )?;
        Ok(name)
    }

    pub(in crate::tools::knowledge_base) fn validate_file(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Result<()> {
        if bytes.is_empty() {
            bail!("file is empty")
        }
        if bytes.len() > self.config.plugins.knowledge_base.max_file_size_kb * 1024 {
            bail!("file too large: {} bytes", bytes.len())
        }
        std::str::from_utf8(bytes).context("file is not valid UTF-8 text")?;
        let file_name = file_name(name).to_ascii_lowercase();
        let ext = Path::new(&file_name)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| format!(".{ext}"));
        let allowed_ext = split_csv(&self.config.plugins.knowledge_base.allowed_extensions);
        let allowed_names = split_csv(&self.config.plugins.knowledge_base.allowed_filenames);
        if ext.as_ref().is_some_and(|ext| allowed_ext.contains(ext))
            || allowed_names.contains(&file_name)
        {
            Ok(())
        } else {
            bail!("unsupported file type or name: {file_name}")
        }
    }

    pub(in crate::tools) fn safe_file_path(&self, rel: &str) -> Result<PathBuf> {
        let rel = normalize_relative_path(rel)?;
        let path = self.files_dir.join(&rel);
        let parent = path.parent().unwrap_or(&self.files_dir);
        // 两个目录都建好之后再取真实路径。基准目录还不存在时 canonicalize 会
        // 失败、退回未解析的原始路径,而 parent 那边解析成功——于是路径里只要
        // 有一层符号链接,两边就对不上,第一次往新库写文件必然报「逃出目录」。
        // macOS 的 /tmp→/private/tmp、/var→/private/var 是现成的踩法。
        std::fs::create_dir_all(&self.files_dir)?;
        std::fs::create_dir_all(parent)?;
        let base = self
            .files_dir
            .canonicalize()
            .unwrap_or_else(|_| self.files_dir.clone());
        let resolved_parent = parent.canonicalize()?;
        if !resolved_parent.starts_with(&base) {
            bail!("knowledge base path escapes files dir")
        }
        Ok(path)
    }

    pub(in crate::tools::knowledge_base) fn existing_file_path(
        &self,
        rel: &str,
    ) -> Result<PathBuf> {
        let rel = normalize_relative_path(rel)?;
        let path = self.files_dir.join(&rel);
        let base = self
            .files_dir
            .canonicalize()
            .unwrap_or_else(|_| self.files_dir.clone());
        let parent = path.parent().unwrap_or(&self.files_dir);
        let resolved_parent = parent.canonicalize()?;
        if !resolved_parent.starts_with(&base) {
            bail!("knowledge base path escapes files dir")
        }
        Ok(path)
    }
}
