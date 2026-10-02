//! 写入：记事实、记事件、消化成日记。
//!
//! `process_after_turn` 是回合结束后的入口，它只是**排队**：真正的组织交给后台
//! 的 organizer，因为那要调模型，不能挡住回合返回。
//!
//! `next_organization_batch` / `apply_organized_batch` 成对：批次在应用前会校验
//! 会话还在不在——`reset_all` 之后飞在半路的批次必须作废，否则刚清空的记忆会被
//! 旧批次重新写回来。

use crate::memory::*;

fn collect_ids(tx: &rusqlite::Transaction<'_>, table: &str) -> Result<Vec<i64>> {
    if !matches!(table, "facts" | "episodes") {
        bail!("invalid memory id table: {table}");
    }
    let mut stmt = tx.prepare(&format!("SELECT id FROM {table} ORDER BY id"))?;
    let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

impl MemoryStore {
    pub fn clear_pending_events(&self) -> Result<()> {
        self.init()?;
        let data = self.data_conn()?;
        data.execute("DELETE FROM pending_events", [])?;
        data.execute(
            "DELETE FROM sqlite_sequence WHERE name = 'pending_events'",
            [],
        )?;
        Ok(())
    }

    pub fn remember_fact(&self, content: &str, source: &str) -> Result<i64> {
        if !self.config.enabled || !self.writes_enabled || content.trim().is_empty() {
            return Ok(0);
        }
        self.init()?;
        let ownership = self.manual_fact_ownership();
        let subjects = ownership_subjects_json(&ownership);
        let mut conn = self.data_conn()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation =
            tx.query_row("SELECT generation FROM memory_meta WHERE id=1", [], |row| {
                row.get::<_, i64>(0)
            })?;
        tx.execute(
            "INSERT INTO facts (
                content, source, status, confidence, recall_count, created_at, updated_at,
                visibility, owner_principal, owner_display_name, subjects, origin_session_id
             ) VALUES (?1, ?2, 'active', 1.0, 0, ?3, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                content.trim(),
                source.trim(),
                now(),
                ownership.visibility,
                ownership.owner_principal,
                ownership.owner_display_name,
                subjects,
                self.write_session_id(),
            ],
        )?;
        let id = tx.last_insert_rowid();
        // `reset_all` historically resets the AUTOINCREMENT sequence.  If a
        // fresh row reuses an id, its old deletion marker must not hide it
        // from future transfer imports.
        tx.execute(
            "DELETE FROM memory_tombstones WHERE kind='fact' AND id=?1",
            [id],
        )?;
        record_lifecycle_event(
            &tx,
            lifecycle_event(
                "fact",
                Some(id),
                MemoryLifecycleState::Transient,
                MemoryLifecycleState::Committed,
                MemoryLifecycleOwner::User,
                lifecycle_scope(&ownership),
                "manual_remember",
                Vec::new(),
                content_digest(content.trim()),
                generation,
                now(),
            )?,
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn process_after_turn(
        &self,
        user_message: &str,
        assistant_message: &str,
        origin: &MemoryOrigin,
        expected_database_id: &str,
        expected_generation: i64,
    ) -> Result<bool> {
        if !self.writes_enabled || !self.config.enabled || !self.config.auto_diary_enabled {
            return Ok(false);
        }
        if !self.data_db.is_file() {
            self.init()?;
        }
        let created_at = now();
        let expires_at = (Utc::now()
            + ChronoDuration::days(self.config.short_diary_retention_days as i64))
        .to_rfc3339();
        let content = diary_content(&created_at, user_message, assistant_message);
        let ownership = self.automatic_ownership(origin);
        let subjects = ownership_subjects_json(&ownership);
        let mut conn = self.data_conn_existing()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current_database_id, current_generation) = tx.query_row(
            "SELECT database_id, generation FROM memory_meta WHERE id=1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if current_database_id != expected_database_id || current_generation != expected_generation
        {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO episodes (
                content, source, status, strength, recall_count, created_at, updated_at,
                retention, user_message, assistant_message, expires_at,
                origin_kind, origin_platform, origin_account_id, origin_conversation_kind,
                origin_conversation_id, origin_sender_id, origin_sender_display_name,
                origin_session_id, origin_message_id,
                visibility, owner_principal, owner_display_name, subjects
             ) VALUES (?1, 'episode', 'active', 1.0, 0, ?2, ?2, ?3, ?4, ?5, ?6,
                       ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                content,
                created_at,
                SHORT_TERM,
                user_message.trim(),
                assistant_message.trim(),
                expires_at,
                origin.kind,
                origin.platform,
                origin.account_id,
                origin.conversation_kind,
                origin.conversation_id,
                origin.sender_id,
                origin.sender_display_name,
                origin.session_id,
                origin.message_id,
                ownership.visibility,
                ownership.owner_principal,
                ownership.owner_display_name,
                subjects,
            ],
        )?;
        let episode_id = tx.last_insert_rowid();
        tx.execute(
            "DELETE FROM memory_tombstones WHERE kind='episode' AND id=?1",
            [episode_id],
        )?;
        record_lifecycle_event(
            &tx,
            lifecycle_event(
                "episode",
                Some(episode_id),
                MemoryLifecycleState::Transient,
                MemoryLifecycleState::Short,
                MemoryLifecycleOwner::TurnLoop,
                lifecycle_scope(&ownership),
                "turn_completed",
                vec![episode_id],
                content_digest(&content),
                current_generation,
                created_at.clone(),
            )?,
        )?;
        tx.commit()?;
        self.cleanup_expired_short_diaries()?;
        Ok(true)
    }

    pub fn stats(&self) -> Result<Value> {
        self.init()?;
        self.prune_missing_skill_records()?;
        let data = self.data_conn()?;
        let state = self.state_conn()?;
        Ok(json!({
            "ok": true,
            "data_db": self.data_db.display().to_string(),
            "state_db": self.state_db.display().to_string(),
            "skills_dir": self.skills_dir.display().to_string(),
            "facts": count_rows(&data, "facts")?,
            "episodes": count_rows(&data, "episodes")?,
            "short_diaries": count_where(&data, "episodes", "retention='short_term'")?,
            "long_diaries": count_where(&data, "episodes", "retention='long_term'")?,
            "unconsolidated_diaries": count_where(&data, "episodes", "retention='short_term' AND consolidated_at IS NULL")?,
            "unprocessed_pending_events": count_where(&data, "pending_events", "processed_at IS NULL")?,
            "total_pending_events": count_rows(&data, "pending_events")?,
            "skill_records": count_rows(&data, "skill_records")?,
            "skill_dirs": count_skill_dirs(&self.skills_dir)?,
            "evicted_turns": count_rows(&state, "evicted_turns")?,
        }))
    }

    pub fn reset_all(&self) -> Result<()> {
        self.init()?;
        let mut data = self.data_conn()?;
        let tx = data.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let fact_ids = collect_ids(&tx, "facts")?;
        let episode_ids = collect_ids(&tx, "episodes")?;
        let deleted_at = now();
        record_memory_tombstones(&tx, "fact", &fact_ids, &deleted_at)?;
        record_memory_tombstones(&tx, "episode", &episode_ids, &deleted_at)?;
        tx.execute(
            "UPDATE memory_meta SET generation=generation+1 WHERE id=1",
            [],
        )?;
        tx.execute("DELETE FROM memory_embeddings", [])?;
        tx.execute("DELETE FROM facts", [])?;
        tx.execute("DELETE FROM episodes", [])?;
        tx.execute("DELETE FROM pending_events", [])?;
        tx.execute("DELETE FROM skill_records", [])?;
        tx.execute("DELETE FROM memory_revisions", [])?;
        tx.execute(
            "DELETE FROM sqlite_sequence WHERE name IN ('facts', 'episodes', 'pending_events', 'skill_records', 'memory_revisions')",
            [],
        )?;
        tx.commit()?;
        self.clear_evicted_context()?;
        Ok(())
    }

    /// 只清掉本会话产生的记忆。改动之前存下的旧行没有会话标记
    /// (`origin_session_id` 为空串),不会被这里删掉——那些只能走 `reset_all`。
    ///
    /// 空 id 直接报错而不是当成"清空标记的行":那批正是全部历史遗留数据,
    /// 静默清掉它们等于把 `reset_all` 伪装成会话级重置。
    pub fn reset_session(&self, session_id: &str) -> Result<MemoryResetSummary> {
        let session_id = session_id.trim();
        if session_id.is_empty() {
            bail!("session-scoped memory reset needs a session id");
        }
        self.init()?;
        let mut data = self.data_conn()?;
        let tx = data.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE memory_meta SET generation=generation+1 WHERE id=1",
            [],
        )?;
        let deleted_episode_ids = {
            let mut stmt =
                tx.prepare("SELECT id FROM episodes WHERE origin_session_id=?1 ORDER BY id")?;
            let rows = stmt.query_map(params![session_id], |row| row.get::<_, i64>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let deleted_fact_ids = {
            let mut stmt =
                tx.prepare("SELECT id FROM facts WHERE origin_session_id=?1 ORDER BY id")?;
            let rows = stmt.query_map(params![session_id], |row| row.get::<_, i64>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let deleted_at = now();
        record_memory_tombstones(&tx, "fact", &deleted_fact_ids, &deleted_at)?;
        record_memory_tombstones(&tx, "episode", &deleted_episode_ids, &deleted_at)?;
        // 向量按 (kind, id) 挂在行上,没有触发器跟着删。行走了它还在,而 id
        // 是自增的,迟早被新行撞上并读回一段别人的语义。kind 字面量与
        // `semantic::kind_name` 同源。
        tx.execute(
            "DELETE FROM memory_embeddings
              WHERE (kind='fact' AND id IN (SELECT id FROM facts WHERE origin_session_id=?1))
                 OR (kind='episode' AND id IN (SELECT id FROM episodes WHERE origin_session_id=?1))",
            params![session_id],
        )?;
        scrub_episode_references(&tx, &deleted_episode_ids)?;
        let facts = tx.execute(
            "DELETE FROM facts WHERE origin_session_id=?1",
            params![session_id],
        )?;
        // A session reset deletes facts as well as episodes.  Their revision
        // bodies are private history for those facts and must leave in the
        // same transaction, otherwise browse_revisions would retain content
        // for a row that no longer exists.
        tx.execute(
            "DELETE FROM memory_revisions
              WHERE memory_id NOT IN (SELECT id FROM facts)",
            [],
        )?;
        let episodes = tx.execute(
            "DELETE FROM episodes WHERE origin_session_id=?1",
            params![session_id],
        )?;
        let pending_events = tx.execute(
            "DELETE FROM pending_events WHERE origin_session_id=?1",
            params![session_id],
        )?;
        tx.commit()?;
        let evicted_turns = self.clear_session_evicted_context(session_id)?;
        Ok(MemoryResetSummary {
            facts,
            episodes,
            pending_events,
            evicted_turns,
        })
    }

    pub(crate) fn next_organization_batch(&self) -> Result<Option<OrganizationBatch>> {
        if !self.config.enabled || !self.config.auto_diary_enabled {
            return Ok(None);
        }
        if !self.data_db.is_file() {
            return Ok(None);
        }
        self.init_existing()?;
        self.cleanup_expired_short_diaries()?;
        let conn = self.data_conn_existing()?;
        let (database_id, generation) = conn.query_row(
            "SELECT database_id, generation FROM memory_meta WHERE id=1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        // 已被遗忘(主动 forget 或衰减)的日记不参与整理:否则会被组织
        // 批次"复活"为长期记忆。
        let forced = count_where(
            &conn,
            "episodes",
            "retention='short_term' AND promotion_pending=1 AND status != 'forgotten'",
        )?;
        let unconsolidated = count_where(
            &conn,
            "episodes",
            "retention='short_term' AND consolidated_at IS NULL AND status != 'forgotten'",
        )?;
        if forced == 0 && unconsolidated < self.config.diary_batch_size as i64 {
            return Ok(None);
        }

        let (sql, limit) = if forced > 0 {
            (
                "SELECT id, created_at, user_message, assistant_message, 1,
                        origin_kind, origin_platform, origin_account_id,
                        origin_conversation_kind, origin_conversation_id, origin_sender_id,
                        origin_sender_display_name, origin_session_id, origin_message_id
                 FROM episodes
                 WHERE retention='short_term' AND promotion_pending=1
                   AND status != 'forgotten'
                 ORDER BY id LIMIT ?1",
                self.config.diary_batch_size.max(1),
            )
        } else {
            (
                "SELECT id, created_at, user_message, assistant_message, 0,
                        origin_kind, origin_platform, origin_account_id,
                        origin_conversation_kind, origin_conversation_id, origin_sender_id,
                        origin_sender_display_name, origin_session_id, origin_message_id
                 FROM episodes
                 WHERE retention='short_term' AND consolidated_at IS NULL
                   AND status != 'forgotten'
                 ORDER BY id LIMIT ?1",
                self.config.diary_batch_size,
            )
        };
        let mut stmt = conn.prepare(sql)?;
        let diaries = stmt
            .query_map([limit as i64], |row| {
                let origin = MemoryOrigin {
                    kind: row.get(5)?,
                    platform: row.get(6)?,
                    account_id: row.get(7)?,
                    conversation_kind: row.get(8)?,
                    conversation_id: row.get(9)?,
                    sender_id: row.get(10)?,
                    sender_display_name: row.get(11)?,
                    session_id: row.get(12)?,
                    message_id: row.get(13)?,
                };
                Ok(ShortDiaryRecord {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    user_message: row.get(2)?,
                    assistant_message: row.get(3)?,
                    force_long_term: row.get::<_, i64>(4)? != 0,
                    owner_principal: origin
                        .principal_ownership()
                        .map(|ownership| ownership.owner_principal),
                    origin,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if diaries.is_empty() {
            return Ok(None);
        }
        let existing = load_existing_memory_candidates(&conn, &diaries)?;
        let mut audit_conn = self.data_conn_existing()?;
        let audit_tx = audit_conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for diary in &diaries {
            let diary_content = format!("{}\n{}", diary.user_message, diary.assistant_message);
            record_lifecycle_event(
                &audit_tx,
                lifecycle_event(
                    "episode",
                    Some(diary.id),
                    MemoryLifecycleState::Short,
                    MemoryLifecycleState::Candidate,
                    MemoryLifecycleOwner::MemoryOrganizer,
                    lifecycle_scope_from_principal(diary.owner_principal.as_deref()),
                    "organizer_batch_claimed",
                    vec![diary.id],
                    content_digest(&diary_content),
                    generation,
                    now(),
                )?,
            )?;
        }
        audit_tx.commit()?;
        Ok(Some(OrganizationBatch {
            database_id,
            generation,
            diaries,
            existing,
        }))
    }

    pub(crate) fn apply_organized_batch(
        &self,
        batch: &OrganizationBatch,
        output: OrganizedOutput,
    ) -> Result<()> {
        if !self.data_db.is_file() {
            bail!("memory database moved or removed while organization was running");
        }
        if output.knowledge.len() + output.long_diaries.len() > MAX_ORGANIZED_ITEMS {
            bail!("memory organizer returned too many items");
        }
        let diary_ids = batch
            .diaries
            .iter()
            .map(|diary| diary.id)
            .collect::<BTreeSet<_>>();
        let admission_by_id = batch
            .diaries
            .iter()
            .map(|diary| (diary.id, deterministic_admission(diary)))
            .collect::<BTreeMap<_, _>>();
        let candidate_metadata_by_id = batch
            .diaries
            .iter()
            .map(|diary| {
                let decision = admission_by_id
                    .get(&diary.id)
                    .expect("admission decision exists for every batch diary");
                (diary.id, candidate_metadata(diary, decision))
            })
            .collect::<BTreeMap<_, _>>();
        let forced_ids = batch
            .diaries
            .iter()
            .filter(|diary| diary.force_long_term)
            .map(|diary| diary.id)
            .collect::<BTreeSet<_>>();
        let candidate_fact_ids = batch
            .existing
            .iter()
            .filter(|memory| memory.kind == "knowledge")
            .map(|memory| memory.id)
            .collect::<BTreeSet<_>>();
        let candidate_facts = batch
            .existing
            .iter()
            .filter(|memory| memory.kind == "knowledge")
            .map(|memory| (memory.id, memory))
            .collect::<BTreeMap<_, _>>();
        // 校验失败只丢这一条,不丢整批:以前一条坏项目让整批 bail,批次永远
        // 待处理、每 5 分钟重发一次(08-16 评审记过的「毒批次」)。
        let mut output = output;
        let mut sensitive_output_ids = BTreeSet::new();
        output.knowledge.retain(|action| {
            let verdict = validate_knowledge_action(action, &diary_ids, &candidate_fact_ids)
                .and_then(|_| validate_knowledge_visibility(batch, action))
                .and_then(|_| validate_knowledge_update_scope(batch, action, &candidate_facts))
                .and_then(|_| {
                    if action.diary_ids.iter().all(|id| {
                        admission_by_id
                            .get(id)
                            .is_some_and(|decision| decision.verdict == AdmissionVerdict::Admit)
                    }) {
                        Ok(())
                    } else {
                        bail!("organizer admission source was not admitted")
                    }
                })
                .and_then(|_| {
                    if generated_content_is_sensitive(&action.content) {
                        sensitive_output_ids.extend(action.diary_ids.iter().copied());
                        bail!("organized content contains sensitive material")
                    }
                    Ok(())
                });
            if let Err(error) = &verdict {
                tracing::warn!(error = %error, "{}", yunxi_base::i18n::text("dropping one organized knowledge item", "丢弃一条不合规的整理知识点"));
            }
            verdict.is_ok()
        });
        output.long_diaries.retain(|diary| {
            let verdict = validate_long_diary(batch, diary, &diary_ids).and_then(|_| {
                if diary.diary_ids.iter().all(|id| {
                    admission_by_id
                        .get(id)
                        .is_some_and(|decision| decision.verdict == AdmissionVerdict::Admit)
                }) {
                    Ok(())
                } else {
                    bail!("organizer admission source was not admitted")
                }
            }).and_then(|_| {
                if generated_content_is_sensitive(&diary.content) {
                    sensitive_output_ids.extend(diary.diary_ids.iter().copied());
                    bail!("organized content contains sensitive material")
                }
                Ok(())
            });
            if let Err(error) = &verdict {
                tracing::warn!(error = %error, "{}", yunxi_base::i18n::text("dropping one organized long diary", "丢弃一条不合规的整理长期日记"));
            }
            verdict.is_ok()
        });
        let mut promoted_ids = BTreeSet::new();
        for diary in &output.long_diaries {
            promoted_ids.extend(diary.diary_ids.iter().copied());
        }
        let mut admitted_ids = promoted_ids.clone();
        if self.config.auto_fact_enabled {
            for action in &output.knowledge {
                if action.truth_status != "rejected" {
                    admitted_ids.extend(action.diary_ids.iter().copied());
                }
            }
        }
        if !forced_ids.is_subset(&promoted_ids) {
            // 模型没给被点名的日记写长期日记:记一笔,但这批照常落地并清掉
            // 待晋升标记,否则它会被无限次重新选中。
            tracing::warn!(missing = ?forced_ids.difference(&promoted_ids).collect::<Vec<_>>(), "{}", yunxi_base::i18n::text("memory organizer did not promote every required diary", "记忆整理器没有晋升全部被点名的日记"));
        }

        let mut conn = self.data_conn_existing()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current_database_id, current_generation) = tx.query_row(
            "SELECT database_id, generation FROM memory_meta WHERE id=1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        if current_database_id != batch.database_id || current_generation != batch.generation {
            bail!("memory database was moved, replaced, or reset while organization was running");
        }
        let timestamp = now();
        if self.config.auto_fact_enabled {
            for action in output.knowledge {
                let source_ids = normalized_ids_json(&action.diary_ids);
                let tags = normalized_tags_json(&action.tags);
                let ownership = knowledge_ownership(batch, &action);
                let subjects =
                    organized_subjects_json(batch, &action.diary_ids, &action.subjects, &ownership);
                match action.operation.as_str() {
                    "create" => {
                        let inserted = tx.execute(
                            "INSERT INTO facts (
                                content, source, status, confidence, strength, recall_count,
                                created_at, updated_at, memory_type, truth_status, importance,
                                tags, source_episode_ids,
                                visibility, owner_principal, owner_display_name, subjects,
                                origin_session_id
                             ) SELECT ?1, 'diary-organizer', 'active', ?2, 1.0, 0,
                                      ?3, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
                               WHERE NOT EXISTS (
                                    SELECT 1 FROM facts
                                     WHERE content=?1 AND truth_status!='rejected'
                                       AND visibility=?9 AND owner_principal=?10
                                )",
                            params![
                                action.content.trim(),
                                action.confidence,
                                timestamp,
                                action.memory_type,
                                action.truth_status,
                                action.importance,
                                tags,
                                source_ids,
                                ownership.visibility,
                                ownership.owner_principal,
                                ownership.owner_display_name,
                                subjects,
                                organized_session_id(batch, &action.diary_ids),
                            ],
                        )?;
                        if inserted == 1 {
                            tx.execute(
                                "DELETE FROM memory_tombstones WHERE kind='fact' AND id=?1",
                                [tx.last_insert_rowid()],
                            )?;
                        }
                    }
                    "update" => {
                        let target = action
                            .target_id
                            .context("missing knowledge update target")?;
                        let old_content = tx.query_row(
                            "SELECT content FROM facts WHERE id=?1",
                            [target],
                            |row| row.get::<_, String>(0),
                        )?;
                        tx.execute(
                            "INSERT INTO memory_revisions (
                                memory_id, old_content, new_content, source_episode_ids, created_at
                             ) VALUES (?1, ?2, ?3, ?4, ?5)",
                            params![
                                target,
                                old_content,
                                action.content.trim(),
                                source_ids,
                                timestamp
                            ],
                        )?;
                        // Content and truth status changed in one transaction.  The
                        // previous vector is no longer authoritative; leave the row
                        // without a vector for the normal backfill path.
                        tx.execute(
                            "DELETE FROM memory_embeddings WHERE kind='fact' AND id=?1",
                            [target],
                        )?;
                        tx.execute(
                            "UPDATE facts SET content=?1, source='diary-organizer', status='active',
                                confidence=?2, strength=1.0, updated_at=?3, memory_type=?4,
                                truth_status=?5, importance=?6, tags=?7, source_episode_ids=?8,
                                visibility=?9, owner_principal=?10, owner_display_name=?11,
                                subjects=?12
                              WHERE id=?13",
                            params![
                                action.content.trim(),
                                action.confidence,
                                timestamp,
                                action.memory_type,
                                action.truth_status,
                                action.importance,
                                tags,
                                source_ids,
                                ownership.visibility,
                                ownership.owner_principal,
                                ownership.owner_display_name,
                                subjects,
                                target,
                            ],
                        )?;
                    }
                    _ => unreachable!("validated operation"),
                }
            }
        }

        for diary in output.long_diaries {
            let source_ids = normalized_ids_json(&diary.diary_ids);
            let tags = normalized_tags_json(&diary.tags);
            let source_key = format!(
                "{}:{}",
                diary
                    .diary_ids
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                blake3::hash(diary.content.trim().as_bytes()).to_hex()
            );
            let ownership = diary_ownership(batch, &diary.diary_ids);
            let subjects =
                organized_subjects_json(batch, &diary.diary_ids, &diary.subjects, &ownership);
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO episodes (
                    content, source, status, strength, recall_count, created_at, updated_at,
                    retention, consolidated_at, importance, confidence, tags,
                    source_episode_ids, source_key,
                    visibility, owner_principal, owner_display_name, subjects,
                    origin_session_id
                 ) VALUES (?1, 'diary-organizer', 'active', 1.0, 0, ?2, ?2,
                           ?3, ?2, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    diary.content.trim(),
                    timestamp,
                    LONG_TERM,
                    diary.importance,
                    diary.confidence,
                    tags,
                    source_ids,
                    source_key,
                    ownership.visibility,
                    ownership.owner_principal,
                    ownership.owner_display_name,
                    subjects,
                    organized_session_id(batch, &diary.diary_ids),
                ],
            )?;
            if inserted == 1 {
                tx.execute(
                    "DELETE FROM memory_tombstones WHERE kind='episode' AND id=?1",
                    [tx.last_insert_rowid()],
                )?;
            }
        }

        for diary in &batch.diaries {
            tx.execute(
                "UPDATE episodes SET consolidated_at=COALESCE(consolidated_at, ?1),
                    promotion_pending=0,
                    promoted_at=CASE WHEN ?2 THEN COALESCE(promoted_at, ?1) ELSE promoted_at END
                 WHERE id=?3 AND retention='short_term'",
                params![timestamp, promoted_ids.contains(&diary.id), diary.id],
            )?;
            let decision = admission_by_id
                .get(&diary.id)
                .expect("admission decision exists for every batch diary");
            let metadata = candidate_metadata_by_id
                .get(&diary.id)
                .expect("candidate metadata exists for every batch diary");
            let (to_state, reason_code) = if admitted_ids.contains(&diary.id) {
                (
                    MemoryLifecycleState::Committed,
                    format!("organizer_admitted:{}", decision.reason_code),
                )
            } else if decision.verdict != AdmissionVerdict::Admit {
                (
                    MemoryLifecycleState::Rejected,
                    format!("organizer_rejected:{}", decision.reason_code),
                )
            } else if sensitive_output_ids.contains(&diary.id) {
                (
                    MemoryLifecycleState::Rejected,
                    "organizer_rejected:generated_sensitive".to_string(),
                )
            } else {
                (
                    MemoryLifecycleState::Rejected,
                    "organizer_rejected:no_valid_output".to_string(),
                )
            };
            record_lifecycle_event(
                &tx,
                lifecycle_event(
                    "episode",
                    Some(diary.id),
                    MemoryLifecycleState::Candidate,
                    to_state,
                    MemoryLifecycleOwner::MemoryOrganizer,
                    lifecycle_scope_from_principal(diary.owner_principal.as_deref()),
                    &reason_code,
                    vec![diary.id],
                    metadata.content_digest.clone(),
                    batch.generation,
                    timestamp.clone(),
                )?,
            )?;
        }
        tx.commit()?;
        self.cleanup_expired_short_diaries()?;
        Ok(())
    }

    /// 放弃一批整理不动的日记(模型连续几次都给不出能解析的结果):标成已
    /// 整理、清掉待晋升,让整理器往前走,不再每次唤醒都拿它们烧模型。
    pub(crate) fn skip_organization_batch(&self, batch: &OrganizationBatch) -> Result<()> {
        if !self.data_db.is_file() {
            return Ok(());
        }
        let mut conn = self.data_conn_existing()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let timestamp = now();
        for diary in &batch.diaries {
            tx.execute(
                "UPDATE episodes SET consolidated_at=COALESCE(consolidated_at, ?1), promotion_pending=0
                 WHERE id=?2 AND retention='short_term'",
                params![timestamp, diary.id],
            )?;
            let diary_content = format!("{}\n{}", diary.user_message, diary.assistant_message);
            record_lifecycle_event(
                &tx,
                lifecycle_event(
                    "episode",
                    Some(diary.id),
                    MemoryLifecycleState::Candidate,
                    MemoryLifecycleState::Rejected,
                    MemoryLifecycleOwner::MemoryOrganizer,
                    lifecycle_scope_from_principal(diary.owner_principal.as_deref()),
                    "organizer_failed",
                    vec![diary.id],
                    content_digest(&diary_content),
                    batch.generation,
                    timestamp.clone(),
                )?,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn cleanup_expired_short_diaries(&self) -> Result<usize> {
        if !self.data_db.is_file() {
            return Ok(0);
        }
        let mut conn = self.data_conn_existing()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation =
            tx.query_row("SELECT generation FROM memory_meta WHERE id=1", [], |row| {
                row.get::<_, i64>(0)
            })?;
        let expired = {
            let mut stmt = tx.prepare(
                "SELECT id, content, owner_principal
                   FROM episodes
                  WHERE retention='short_term'
                    AND status!='forgotten'
                    AND promotion_pending=0
                    AND expires_at IS NOT NULL
                    AND unixepoch(expires_at) IS NOT NULL
                    AND unixepoch(expires_at) <= unixepoch('now')
                  ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        if expired.is_empty() {
            tx.commit()?;
            return Ok(0);
        }
        let timestamp = now();
        for (id, content, owner_principal) in &expired {
            record_lifecycle_event(
                &tx,
                lifecycle_event(
                    "episode",
                    Some(*id),
                    MemoryLifecycleState::Short,
                    MemoryLifecycleState::Expired,
                    MemoryLifecycleOwner::MemoryGc,
                    lifecycle_scope_from_principal(
                        (!owner_principal.is_empty()).then_some(owner_principal.as_str()),
                    ),
                    "short_retention_elapsed",
                    vec![*id],
                    content_digest(content),
                    generation,
                    timestamp.clone(),
                )?,
            )?;
        }
        scrub_episode_references(
            &tx,
            &expired.iter().map(|(id, _, _)| *id).collect::<Vec<_>>(),
        )?;
        tx.execute(
            "DELETE FROM memory_embeddings
              WHERE kind='episode' AND id IN (
                  SELECT id FROM episodes
                   WHERE retention='short_term'
                     AND status!='forgotten'
                     AND promotion_pending=0
                     AND expires_at IS NOT NULL
                     AND unixepoch(expires_at) IS NOT NULL
                     AND unixepoch(expires_at) <= unixepoch('now')
              )",
            [],
        )?;
        tx.execute(
            "UPDATE episodes SET status='forgotten'
             WHERE retention='short_term'
               AND status!='forgotten'
               AND consolidated_at IS NULL
               AND promotion_pending=0
               AND expires_at IS NOT NULL
               AND unixepoch(expires_at) IS NOT NULL
               AND unixepoch(expires_at) <= unixepoch('now')",
            [],
        )?;
        let deleted_ids = {
            let mut stmt = tx.prepare(
                "SELECT id FROM episodes
                 WHERE retention='short_term'
                   AND consolidated_at IS NOT NULL
                   AND promotion_pending=0
                   AND expires_at IS NOT NULL
                   AND unixepoch(expires_at) IS NOT NULL
                   AND unixepoch(expires_at) <= unixepoch('now')
                 ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let deleted = tx.execute(
            "DELETE FROM episodes
             WHERE retention='short_term'
               AND consolidated_at IS NOT NULL
               AND promotion_pending=0
               AND expires_at IS NOT NULL
               AND unixepoch(expires_at) IS NOT NULL
               AND unixepoch(expires_at) <= unixepoch('now')",
            [],
        )?;
        record_memory_tombstones(&tx, "episode", &deleted_ids, &timestamp)?;
        tx.commit()?;
        Ok(deleted)
    }

    pub(crate) fn prune_missing_skill_records(&self) -> Result<()> {
        let conn = self.data_conn()?;
        let mut stmt = conn.prepare("SELECT id, path FROM skill_records")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut missing = Vec::new();
        for row in rows {
            let (id, path) = row?;
            if !PathBuf::from(path).exists() {
                missing.push(id);
            }
        }
        drop(stmt);
        for id in missing {
            conn.execute("DELETE FROM skill_records WHERE id=?1", params![id])?;
        }
        Ok(())
    }
}

fn lifecycle_scope(ownership: &MemoryOwnership) -> String {
    lifecycle_scope_from_principal(
        (!ownership.owner_principal.is_empty()).then_some(ownership.owner_principal.as_str()),
    )
}

fn lifecycle_scope_from_principal(principal: Option<&str>) -> String {
    principal
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "privileged".to_string())
}

fn lifecycle_event(
    memory_kind: &str,
    memory_id: Option<i64>,
    from_state: MemoryLifecycleState,
    to_state: MemoryLifecycleState,
    owner: MemoryLifecycleOwner,
    owner_scope: String,
    reason_code: &str,
    source_episode_ids: Vec<i64>,
    content_digest: String,
    generation: i64,
    created_at: String,
) -> Result<MemoryLifecycleEvent> {
    let source_episode_ids = normalized_episode_ids(&source_episode_ids)?;
    let transition_key = transition_key(
        generation,
        memory_kind,
        memory_id,
        from_state,
        to_state,
        &source_episode_ids,
    );
    let event = MemoryLifecycleEvent {
        memory_kind: memory_kind.to_string(),
        memory_id,
        from_state,
        to_state,
        owner,
        owner_scope,
        reason_code: reason_code.to_string(),
        source_episode_ids,
        content_digest,
        generation,
        transition_key,
        created_at,
    };
    event.validate()?;
    Ok(event)
}

fn record_lifecycle_event(conn: &Connection, event: MemoryLifecycleEvent) -> Result<bool> {
    event.validate()?;
    let source_episode_ids = serde_json::to_string(&event.source_episode_ids)?;
    let affected = conn.execute(
        "INSERT OR IGNORE INTO memory_lifecycle_events (
             memory_kind, memory_id, from_state, to_state, owner, owner_scope,
             reason_code, source_episode_ids, content_digest, generation,
             transition_key, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            event.memory_kind,
            event.memory_id,
            event.from_state.as_str(),
            event.to_state.as_str(),
            event.owner.as_str(),
            event.owner_scope,
            event.reason_code,
            source_episode_ids,
            event.content_digest,
            event.generation,
            event.transition_key,
            event.created_at,
        ],
    )?;
    Ok(affected == 1)
}

#[cfg(any(test, feature = "testkit"))]
mod test_support;
