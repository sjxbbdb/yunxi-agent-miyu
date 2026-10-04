use super::shared::*;
use crate::decision::{validate_result, DecisionError, DecisionOutcome};
use crate::decision_shadow::{
    observation_replay_bytes, observation_replay_digest, ShadowCallContext, ShadowDecisionProvider,
    ShadowError, ShadowMatchKind, ShadowMode, ShadowObservation,
};
use crate::memory::*;
use serde_json::json;
use std::cell::Cell;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, TryRecvError, TrySendError};
use std::sync::{Arc, Barrier};
use std::thread;
use yunxi_base::config::AppConfig;

fn diary(
    id: i64,
    user_message: &str,
    assistant_message: &str,
    force_long_term: bool,
) -> ShortDiaryRecord {
    ShortDiaryRecord {
        id,
        created_at: "2026-10-02T00:00:00Z".to_string(),
        user_message: user_message.to_string(),
        assistant_message: assistant_message.to_string(),
        force_long_term,
        owner_principal: None,
        origin: MemoryOrigin::local("admission-test"),
    }
}

#[test]
fn stable_signals_and_force_are_admitted() {
    for (text, class) in [
        ("我的终端偏好是使用 fish", AdmissionClass::StablePreference),
        (
            "项目仓库必须运行 cargo fmt",
            AdmissionClass::ProjectConstraint,
        ),
        (
            "系统约束必须使用 UTC 环境",
            AdmissionClass::SystemConstraint,
        ),
        ("下次继续这个工作时记住它", AdmissionClass::FutureValue),
    ] {
        let result = deterministic_admission_text(&text, false);
        assert_eq!(result.verdict, AdmissionVerdict::Admit);
        assert_eq!(result.class, class);
        assert_eq!(result.sensitivity, AdmissionSensitivity::None);
        assert!(result.score <= 100);
    }
    let result = deterministic_admission(&diary(1, "闲聊", "请记住", true));
    assert_eq!(result.verdict, AdmissionVerdict::Admit);
    assert_eq!(result.reason_code, "force_long_term");
}

#[test]
fn sensitive_and_ephemeral_signals_are_not_admitted() {
    let credential_samples = [
        (
            format!("{}{}", "sk-", "probable-test-token-1234"),
            AdmissionClass::SensitiveCredential,
        ),
        (
            format!("{}{}", "ghp_", "1234567890abcdef"),
            AdmissionClass::SensitiveCredential,
        ),
        (
            format!("{}{}", "AIza", "1234567890abcdef"),
            AdmissionClass::SensitiveCredential,
        ),
        (
            format!("{}{}", "AKIA", "1234567890abcdef"),
            AdmissionClass::SensitiveCredential,
        ),
    ];
    for (text, class) in credential_samples.into_iter().chain([
        (
            "password: hunter2".to_string(),
            AdmissionClass::SensitiveCredential,
        ),
        (
            "这是一个 secret".to_string(),
            AdmissionClass::SensitiveSecret,
        ),
        (
            "one-time token 123456".to_string(),
            AdmissionClass::OneTimeToken,
        ),
        (
            "这是临时的一次性记录".to_string(),
            AdmissionClass::Ephemeral,
        ),
    ]) {
        let result = deterministic_admission_text(&text, false);
        assert_ne!(result.verdict, AdmissionVerdict::Admit);
        assert_eq!(result.class, class);
    }
    let ambiguous = deterministic_admission_text("你好，谢谢", false);
    assert_eq!(ambiguous.verdict, AdmissionVerdict::Abstain);
    assert_eq!(ambiguous.reason_code, "no_long_term_signal");
    let forced_secret = deterministic_admission_text("请记住 password: hunter2", true);
    assert_eq!(forced_secret.verdict, AdmissionVerdict::Reject);
}

#[test]
fn candidate_metadata_contains_digest_and_no_raw_text() {
    let marker = "ADMISSION_RAW_MARKER";
    let source = diary(7, marker, "稳定偏好", false);
    let decision = deterministic_admission(&source);
    let metadata = candidate_metadata(&source, &decision);
    let encoded = serde_json::to_string(&metadata).unwrap();
    assert!(!encoded.contains(marker));
    assert_eq!(metadata.candidate_id, 7);
    assert_eq!(metadata.source_episode_ids, vec![7]);
    assert_eq!(metadata.content_digest.len(), 64);
    assert_eq!(metadata.schema_version, ADMISSION_SCHEMA_VERSION);
}

#[test]
fn mixed_sources_require_all_admitted_per_action() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.plugins.memory.diary_batch_size = 2;
    let store = MemoryStore::new(&config, &test_paths(&temp));
    assert!(record_turn(
        &store,
        "我的稳定偏好是 fish",
        "以后一直使用 fish"
    ));
    assert!(record_turn(&store, "临时记录", "刚才的一次性上下文"));
    let batch = store.next_organization_batch().unwrap().unwrap();
    let admitted_id = batch.diaries[0].id;
    let rejected_id = batch.diaries[1].id;
    store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: Vec::new(),
                long_diaries: vec![
                    LongDiaryDraft {
                        content: "稳定来源可以落地".to_string(),
                        importance: 3,
                        confidence: 0.9,
                        visibility: VISIBILITY_PRIVILEGED.to_string(),
                        subjects: Vec::new(),
                        tags: Vec::new(),
                        diary_ids: vec![admitted_id],
                    },
                    LongDiaryDraft {
                        content: "混合来源不能落地".to_string(),
                        importance: 3,
                        confidence: 0.9,
                        visibility: VISIBILITY_PRIVILEGED.to_string(),
                        subjects: Vec::new(),
                        tags: Vec::new(),
                        diary_ids: vec![admitted_id, rejected_id],
                    },
                ],
            },
        )
        .unwrap();
    let conn = store.data_conn().unwrap();
    assert_eq!(
        count_where(&conn, "episodes", "retention='long_term'").unwrap(),
        1
    );
}

#[test]
fn organizer_content_sensitive_gate_is_deterministic() {
    assert!(generated_content_is_sensitive(
        "Do not retain this password: hunter2"
    ));
    assert!(!generated_content_is_sensitive(
        "The project requires cargo fmt."
    ));
}

fn envelope_for(text: &str, force_long_term: bool) -> AdmissionDecisionEnvelope {
    let decision = deterministic_admission_text(text, force_long_term);
    AdmissionDecisionRequestBuilder::new(
        AdmissionSourceClass::Standalone,
        force_long_term,
        "admission-rules-1",
        80,
    )
    .unwrap()
    .build(&decision)
    .unwrap()
}

struct MatchingAdmissionProvider {
    calls: AtomicUsize,
}

impl ShadowDecisionProvider for MatchingAdmissionProvider {
    fn observe(
        &self,
        request: &crate::decision::DecisionRequest,
    ) -> Result<crate::decision::DecisionResult, ShadowError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(crate::decision::DecisionResult {
            schema_version: crate::decision::DECISION_SCHEMA_V1.to_owned(),
            task: request.task,
            input_fingerprint: request.input_fingerprint.clone(),
            outcome: DecisionOutcome::Choice {
                candidate_id: "admit".to_owned(),
                confidence: 1.0,
            },
            abstain: false,
            reason_code: crate::decision::DecisionReason::ProviderChoice,
            provider: "fake".to_owned(),
            elapsed_ms: 0,
        })
    }

    fn observe_with_context(
        &self,
        request: &crate::decision::DecisionRequest,
        _context: ShadowCallContext,
    ) -> Result<crate::decision::DecisionResult, ShadowError> {
        self.observe(request)
    }
}

struct FaultAdmissionProvider {
    calls: AtomicUsize,
}

impl ShadowDecisionProvider for FaultAdmissionProvider {
    fn observe(
        &self,
        _request: &crate::decision::DecisionRequest,
    ) -> Result<crate::decision::DecisionResult, ShadowError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(ShadowError::Timeout)
    }
}

#[test]
fn admission_shadow_is_disabled_by_default_and_record_only() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let disabled = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let observation = observe_admission_shadow(
        &envelope,
        AdmissionShadowConfig::default(),
        Some(&disabled),
        None,
    )
    .unwrap();
    assert!(observation.is_none());
    assert_eq!(disabled.calls.load(Ordering::SeqCst), 0);
    assert_eq!(envelope.primary, primary);

    let provider = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let mut config = AdmissionShadowConfig::default();
    config.mode = ShadowMode::RecordOnly;
    let observation = observe_admission_shadow(&envelope, config, Some(&provider), None)
        .unwrap()
        .expect("record-only provider produces an observation");
    assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMatch);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(envelope.primary, primary);
}

#[test]
fn admission_shadow_fault_budget_and_cancel_are_observations_only() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let provider = FaultAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let mut config = AdmissionShadowConfig::default();
    config.mode = ShadowMode::RecordOnly;
    let observation = observe_admission_shadow(&envelope, config, Some(&provider), None)
        .unwrap()
        .expect("provider fault is represented");
    assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let cancelled = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    config.cancelled = true;
    let observation = observe_admission_shadow(&envelope, config, Some(&cancelled), None)
        .unwrap()
        .expect("cancellation is represented");
    assert_eq!(observation.match_kind, ShadowMatchKind::Cancelled);
    assert_eq!(cancelled.calls.load(Ordering::SeqCst), 0);

    config.cancelled = false;
    config.budget.queue_slots = 0;
    let queued = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let observation = observe_admission_shadow(&envelope, config, Some(&queued), None)
        .unwrap()
        .expect("queue exhaustion is represented");
    assert_eq!(observation.match_kind, ShadowMatchKind::QueueFull);
    assert_eq!(queued.calls.load(Ordering::SeqCst), 0);
    assert_eq!(envelope.primary, primary);
}

fn shadow_observation(kind: ShadowMatchKind) -> ShadowObservation {
    ShadowObservation {
        schema_version: crate::decision::DECISION_SCHEMA_V1.to_owned(),
        task: crate::decision::DecisionTask::MemoryAdmission,
        scope: crate::decision::DecisionScope::Memory,
        input_fingerprint: "sha256:admission".to_owned(),
        primary_digest: "sha256:primary".to_owned(),
        shadow_digest: None,
        match_kind: kind,
        provider_id: "test".to_owned(),
        elapsed_ms: 0,
    }
}

#[test]
fn admission_shadow_metrics_count_every_match_kind_in_stable_snapshot() {
    let metrics = AdmissionShadowMetrics::default();
    for kind in [
        ShadowMatchKind::ExactMatch,
        ShadowMatchKind::OutcomeMatch,
        ShadowMatchKind::OutcomeMismatch,
        ShadowMatchKind::BothAbstain,
        ShadowMatchKind::InvalidShadow,
        ShadowMatchKind::Timeout,
        ShadowMatchKind::Cancelled,
        ShadowMatchKind::Unavailable,
        ShadowMatchKind::PrivacyRejected,
        ShadowMatchKind::QueueFull,
        ShadowMatchKind::StaleFingerprint,
    ] {
        metrics.record_started();
        assert!(metrics.record_completed(&shadow_observation(kind), 20));
    }
    metrics.record_started();
    assert!(metrics.record_completed(&shadow_observation(ShadowMatchKind::Timeout), 100));

    assert_eq!(
        metrics.snapshot(),
        AdmissionShadowMetricsSnapshot {
            shadow_started: 12,
            shadow_completed: 12,
            shadow_exact_match: 1,
            shadow_outcome_match: 1,
            shadow_outcome_mismatch: 1,
            shadow_both_abstain: 1,
            shadow_invalid_shadow: 1,
            shadow_timeout: 2,
            shadow_cancelled: 1,
            shadow_unavailable: 1,
            shadow_privacy_rejected: 1,
            shadow_queue_full: 1,
            shadow_stale_fingerprint: 1,
            shadow_latency_p50_ms: 50,
            shadow_latency_p95_ms: 100,
            shadow_latency_p99_ms: 100,
        }
    );
    assert_eq!(metrics.snapshot(), metrics.snapshot());
}

#[test]
fn admission_shadow_token_requires_an_exact_current_context() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let current = AdmissionShadowContext::new(
        envelope.request.task,
        envelope.request.scope,
        &envelope.request.input_fingerprint,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    );
    assert!(token.matches(&current));
    assert!(token.matches_context(&current));
    assert!(token.matches_current(
        current.task,
        current.scope,
        &current.input_fingerprint,
        current.diary_id,
        &current.batch_database_id,
        current.batch_generation,
        current.consumer_epoch,
        current.mode,
    ));
    assert!(token.matches_envelope(&envelope, 41, "memory-db-a", 7, 3, ShadowMode::RecordOnly));

    let mismatches = [
        AdmissionShadowContext::new(
            crate::decision::DecisionTask::RecallRerank,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            crate::decision::DecisionScope::Conversation,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            "sha256:stale",
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            42,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            "memory-db-b",
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            42,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            8,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            4,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            ShadowMode::Disabled,
        ),
    ];
    assert!(mismatches.iter().all(|context| !token.matches(context)));
}

#[test]
fn admission_shadow_response_gate_drops_stale_responses_without_applying() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let current = AdmissionShadowContext::new(
        envelope.request.task,
        envelope.request.scope,
        &envelope.request.input_fingerprint,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    );
    let apply_count = Cell::new(0);
    let apply = || apply_count.set(apply_count.get() + 1);

    assert_eq!(admit_admission_shadow_response(&token, &current), Ok(()));
    if admit_admission_shadow_response(&token, &current).is_ok() {
        apply();
    }
    assert_eq!(apply_count.get(), 1);
    assert_eq!(envelope.primary, primary);

    let mismatches = [
        AdmissionShadowContext::new(
            crate::decision::DecisionTask::RecallRerank,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            crate::decision::DecisionScope::Conversation,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            "sha256:stale",
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            42,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            "memory-db-b",
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            8,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            4,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            ShadowMode::Disabled,
        ),
    ];
    for stale in mismatches {
        assert_eq!(
            admit_admission_shadow_response(&token, &stale),
            Err(DecisionError::StaleFingerprint)
        );
        if admit_admission_shadow_response(&token, &stale).is_ok() {
            apply();
        }
        assert_eq!(apply_count.get(), 1);
        assert_eq!(envelope.primary, primary);
    }
}

struct PendingAdmissionResponse {
    token: AdmissionShadowToken,
    request: crate::decision::DecisionRequest,
    response: crate::decision::DecisionResult,
    state: PendingAdmissionState,
}

#[derive(Clone, Copy)]
enum PendingAdmissionState {
    Ready,
    Cancelled,
    TimedOut,
    Unavailable,
}

/// A test-only consumer model for the future async seam.
///
/// The real consumer is intentionally not implemented here.  This harness
/// proves the required order without a database, callback, runtime, or
/// scheduler: validate the response first, then reject stale context, then
/// count an in-memory apply.
struct AdmissionResponseHarness {
    current: AdmissionShadowContext,
    pending: VecDeque<PendingAdmissionResponse>,
    applied: Cell<usize>,
    stale_dropped: Cell<usize>,
    invalid_dropped: Cell<usize>,
    cancelled_dropped: Cell<usize>,
    timed_out_dropped: Cell<usize>,
    unavailable_dropped: Cell<usize>,
}

impl AdmissionResponseHarness {
    fn new(current: AdmissionShadowContext) -> Self {
        Self {
            current,
            pending: VecDeque::new(),
            applied: Cell::new(0),
            stale_dropped: Cell::new(0),
            invalid_dropped: Cell::new(0),
            cancelled_dropped: Cell::new(0),
            timed_out_dropped: Cell::new(0),
            unavailable_dropped: Cell::new(0),
        }
    }

    fn push(
        &mut self,
        token: AdmissionShadowToken,
        request: crate::decision::DecisionRequest,
        response: crate::decision::DecisionResult,
    ) {
        self.push_with_state(token, request, response, PendingAdmissionState::Ready);
    }

    fn push_with_state(
        &mut self,
        token: AdmissionShadowToken,
        request: crate::decision::DecisionRequest,
        response: crate::decision::DecisionResult,
        state: PendingAdmissionState,
    ) {
        self.pending.push_back(PendingAdmissionResponse {
            token,
            request,
            response,
            state,
        });
    }

    fn drain(&mut self) {
        while let Some(pending) = self.pending.pop_front() {
            match pending.state {
                PendingAdmissionState::Ready => {}
                PendingAdmissionState::Cancelled => {
                    self.cancelled_dropped
                        .set(self.cancelled_dropped.get().saturating_add(1));
                    continue;
                }
                PendingAdmissionState::TimedOut => {
                    self.timed_out_dropped
                        .set(self.timed_out_dropped.get().saturating_add(1));
                    continue;
                }
                PendingAdmissionState::Unavailable => {
                    self.unavailable_dropped
                        .set(self.unavailable_dropped.get().saturating_add(1));
                    continue;
                }
            }
            if validate_result(&pending.request, &pending.response).is_err() {
                self.invalid_dropped
                    .set(self.invalid_dropped.get().saturating_add(1));
                continue;
            }
            if admit_admission_shadow_response(&pending.token, &self.current).is_err() {
                self.stale_dropped
                    .set(self.stale_dropped.get().saturating_add(1));
                continue;
            }
            self.applied.set(self.applied.get().saturating_add(1));
        }
    }
}

fn admission_shadow_context_for(
    envelope: &AdmissionDecisionEnvelope,
    diary_id: i64,
    database_id: &str,
    generation: i64,
    epoch: u64,
    mode: ShadowMode,
) -> AdmissionShadowContext {
    AdmissionShadowContext::new(
        envelope.request.task,
        envelope.request.scope,
        &envelope.request.input_fingerprint,
        diary_id,
        database_id,
        generation,
        epoch,
        mode,
    )
}

#[test]
fn admission_consumer_harness_applies_fresh_response_once_and_drops_invalid() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let provider = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let shadow_response = provider.observe(&envelope.request).unwrap();
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let current =
        admission_shadow_context_for(&envelope, 41, "memory-db-a", 7, 3, ShadowMode::RecordOnly);
    let mut harness = AdmissionResponseHarness::new(current);
    harness.push(token, envelope.request.clone(), shadow_response);
    let mut invalid = envelope.primary.clone();
    invalid.provider.clear();
    harness.push(
        AdmissionShadowToken::from_envelope(
            &envelope,
            41,
            "memory-db-a",
            7,
            3,
            ShadowMode::RecordOnly,
        )
        .unwrap(),
        envelope.request.clone(),
        invalid,
    );

    harness.drain();
    assert_eq!(harness.applied.get(), 1);
    assert_eq!(harness.invalid_dropped.get(), 1);
    assert_eq!(harness.stale_dropped.get(), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(harness.pending.is_empty());
    harness.drain();
    assert_eq!(harness.applied.get(), 1);
    assert_eq!(envelope.primary, primary);
}

#[test]
fn admission_consumer_harness_drops_every_stale_context_without_applying() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let current =
        admission_shadow_context_for(&envelope, 41, "memory-db-a", 7, 3, ShadowMode::RecordOnly);
    let mut stale_contexts = Vec::new();
    for context in [
        AdmissionShadowContext::new(
            crate::decision::DecisionTask::RecallRerank,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            crate::decision::DecisionScope::Conversation,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            "sha256:stale",
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            "memory-db-b",
            current.batch_generation,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            8,
            current.consumer_epoch,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            4,
            current.mode,
        ),
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation,
            current.consumer_epoch,
            ShadowMode::Disabled,
        ),
    ] {
        stale_contexts.push(context);
    }

    for stale in stale_contexts {
        let mut harness = AdmissionResponseHarness::new(stale);
        harness.push(
            token.clone(),
            envelope.request.clone(),
            envelope.primary.clone(),
        );
        harness.drain();
        assert_eq!(harness.applied.get(), 0);
        assert_eq!(harness.invalid_dropped.get(), 0);
        assert_eq!(harness.stale_dropped.get(), 1);
        assert!(harness.pending.is_empty());
    }
}

fn pending_response(
    envelope: &AdmissionDecisionEnvelope,
    token: AdmissionShadowToken,
    response: crate::decision::DecisionResult,
    state: PendingAdmissionState,
) -> PendingAdmissionResponse {
    PendingAdmissionResponse {
        token,
        request: envelope.request.clone(),
        response,
        state,
    }
}

fn threaded_drain(
    current: AdmissionShadowContext,
    pending: PendingAdmissionResponse,
) -> AdmissionResponseHarness {
    let (sender, receiver) = sync_channel(1);
    let barrier = Arc::new(Barrier::new(2));
    let producer_barrier = Arc::clone(&barrier);
    let producer = thread::spawn(move || {
        sender.send(pending).unwrap();
        producer_barrier.wait();
    });
    let consumer_barrier = Arc::clone(&barrier);
    let consumer = thread::spawn(move || {
        consumer_barrier.wait();
        let mut harness = AdmissionResponseHarness::new(current);
        let pending = receiver.recv().unwrap();
        harness.push_with_state(
            pending.token,
            pending.request,
            pending.response,
            pending.state,
        );
        harness.drain();
        harness
    });
    producer.join().unwrap();
    consumer.join().unwrap()
}

#[test]
fn admission_async_like_harness_delivers_fresh_and_stale_across_threads() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let provider = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let response = provider.observe(&envelope.request).unwrap();
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let current =
        admission_shadow_context_for(&envelope, 41, "memory-db-a", 7, 3, ShadowMode::RecordOnly);
    let fresh = threaded_drain(
        current.clone(),
        pending_response(
            &envelope,
            token.clone(),
            response.clone(),
            PendingAdmissionState::Ready,
        ),
    );
    assert_eq!(fresh.applied.get(), 1);
    assert_eq!(fresh.stale_dropped.get(), 0);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let stale = threaded_drain(
        AdmissionShadowContext::new(
            current.task,
            current.scope,
            &current.input_fingerprint,
            current.diary_id,
            &current.batch_database_id,
            current.batch_generation + 1,
            current.consumer_epoch,
            current.mode,
        ),
        pending_response(&envelope, token, response, PendingAdmissionState::Ready),
    );
    assert_eq!(stale.applied.get(), 0);
    assert_eq!(stale.stale_dropped.get(), 1);
}

#[test]
fn admission_async_like_harness_is_bounded_and_drops_cancelled_faults() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let response = envelope.primary.clone();
    let current =
        admission_shadow_context_for(&envelope, 41, "memory-db-a", 7, 3, ShadowMode::RecordOnly);

    let (sender, receiver) = sync_channel(1);
    let first = pending_response(
        &envelope,
        token.clone(),
        response.clone(),
        PendingAdmissionState::Ready,
    );
    let second = pending_response(
        &envelope,
        token.clone(),
        response.clone(),
        PendingAdmissionState::Ready,
    );
    sender.send(first).unwrap();
    assert!(matches!(
        sender.try_send(second),
        Err(TrySendError::Full(_))
    ));

    let consumer = thread::spawn(move || {
        let mut harness = AdmissionResponseHarness::new(current);
        let pending = receiver.recv().unwrap();
        harness.push_with_state(
            pending.token,
            pending.request,
            pending.response,
            pending.state,
        );
        harness.push_with_state(
            AdmissionShadowToken::from_envelope(
                &envelope,
                41,
                "memory-db-a",
                7,
                3,
                ShadowMode::RecordOnly,
            )
            .unwrap(),
            envelope.request.clone(),
            envelope.primary.clone(),
            PendingAdmissionState::Cancelled,
        );
        harness.push_with_state(
            AdmissionShadowToken::from_envelope(
                &envelope,
                41,
                "memory-db-a",
                7,
                3,
                ShadowMode::RecordOnly,
            )
            .unwrap(),
            envelope.request.clone(),
            envelope.primary.clone(),
            PendingAdmissionState::TimedOut,
        );
        harness.push_with_state(
            AdmissionShadowToken::from_envelope(
                &envelope,
                41,
                "memory-db-a",
                7,
                3,
                ShadowMode::RecordOnly,
            )
            .unwrap(),
            envelope.request.clone(),
            envelope.primary.clone(),
            PendingAdmissionState::Unavailable,
        );
        harness.drain();
        harness
    });
    drop(sender);
    let harness = consumer.join().unwrap();
    assert_eq!(harness.applied.get(), 1);
    assert_eq!(harness.cancelled_dropped.get(), 1);
    assert_eq!(harness.timed_out_dropped.get(), 1);
    assert_eq!(harness.unavailable_dropped.get(), 1);
    assert_eq!(harness.invalid_dropped.get(), 0);
    assert_eq!(harness.stale_dropped.get(), 0);
    assert!(harness.pending.is_empty());
}

#[test]
fn admission_fault_replay_is_stable_and_closed_transport_has_no_observation() {
    let metrics = AdmissionShadowMetrics::default();
    for kind in [
        ShadowMatchKind::InvalidShadow,
        ShadowMatchKind::Timeout,
        ShadowMatchKind::Cancelled,
        ShadowMatchKind::Unavailable,
        ShadowMatchKind::QueueFull,
        ShadowMatchKind::StaleFingerprint,
    ] {
        let observation = shadow_observation(kind);
        metrics.record_started();
        assert!(metrics.record_completed(&observation, 20));
        let first_bytes = observation_replay_bytes(&observation);
        let second_bytes = observation_replay_bytes(&observation);
        assert_eq!(first_bytes, second_bytes);
        let first_digest = observation_replay_digest(&observation);
        let second_digest = observation_replay_digest(&observation);
        assert_eq!(first_digest, second_digest,);
        assert!(first_digest.starts_with("sha256:"));
        assert_eq!(first_digest.len(), 71);
        let encoded = String::from_utf8(first_bytes).unwrap();
        assert!(!encoded.contains("raw"));
        assert!(!encoded.contains("diary_id"));
        assert!(!encoded.contains("batch_database_id"));
        assert!(!encoded.contains("batch_generation"));
        assert!(!encoded.contains("consumer_epoch"));
        assert!(!encoded.contains("record_only"));
    }
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.shadow_started, 6);
    assert_eq!(snapshot.shadow_completed, 6);
    assert_eq!(snapshot.shadow_invalid_shadow, 1);
    assert_eq!(snapshot.shadow_timeout, 1);
    assert_eq!(snapshot.shadow_cancelled, 1);
    assert_eq!(snapshot.shadow_unavailable, 1);
    assert_eq!(snapshot.shadow_privacy_rejected, 0);
    assert_eq!(snapshot.shadow_queue_full, 1);
    assert_eq!(snapshot.shadow_stale_fingerprint, 1);

    let (sender, receiver) = sync_channel::<PendingAdmissionResponse>(1);
    drop(sender);
    assert!(matches!(
        receiver.try_recv(),
        Err(TryRecvError::Disconnected)
    ));
    let (sender, receiver) = sync_channel::<PendingAdmissionResponse>(1);
    drop(receiver);
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let pending = pending_response(
        &envelope,
        token,
        envelope.primary.clone(),
        PendingAdmissionState::Ready,
    );
    assert!(matches!(
        sender.try_send(pending),
        Err(TrySendError::Disconnected(_))
    ));
}

#[test]
fn admission_replay_convergence_is_test_only_and_primary_stays_unchanged() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let metrics = AdmissionShadowMetrics::default();
    let mut unique_replays = HashSet::new();
    let observations = [
        (shadow_observation(ShadowMatchKind::OutcomeMatch), 20),
        (shadow_observation(ShadowMatchKind::OutcomeMatch), 20),
        (shadow_observation(ShadowMatchKind::Timeout), 100),
        (shadow_observation(ShadowMatchKind::Timeout), 100),
    ];
    for (observation, elapsed_ms) in observations {
        let bytes = observation_replay_bytes(&observation);
        let digest = observation_replay_digest(&observation);
        assert_eq!(bytes, observation_replay_bytes(&observation));
        assert_eq!(digest, observation_replay_digest(&observation));
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71);
        assert!(!String::from_utf8(bytes)
            .unwrap()
            .contains("ADMISSION_RAW_MARKER"));
        if unique_replays.insert(digest) {
            metrics.record_started();
            assert!(metrics.record_completed(&observation, elapsed_ms));
        }
    }
    assert_eq!(unique_replays.len(), 2);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.shadow_started, 2);
    assert_eq!(snapshot.shadow_completed, 2);
    assert_eq!(snapshot.shadow_outcome_match, 1);
    assert_eq!(snapshot.shadow_timeout, 1);
    assert_eq!(snapshot.shadow_latency_p50_ms, 50);
    assert_eq!(snapshot.shadow_latency_p95_ms, 100);
    assert_eq!(snapshot.shadow_latency_p99_ms, 100);
    assert_eq!(envelope.primary, primary);
}

#[test]
fn admission_shadow_token_stays_out_of_observation_payload() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let token = AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .unwrap();
    let observation = shadow_observation(ShadowMatchKind::OutcomeMatch);
    let encoded = serde_json::to_string(&observation).unwrap();
    assert!(encoded.contains("sha256:admission"));
    assert!(!encoded.contains("diary_id"));
    assert!(!encoded.contains("batch_database_id"));
    assert!(!encoded.contains("batch_generation"));
    assert!(!encoded.contains("consumer_epoch"));
    assert!(!encoded.contains("record_only"));
    let _ = token;
}

#[test]
fn admission_shadow_metrics_reject_other_consumers() {
    let metrics = AdmissionShadowMetrics::default();
    let mut observation = shadow_observation(ShadowMatchKind::OutcomeMatch);
    observation.task = crate::decision::DecisionTask::RecallRerank;
    assert!(!metrics.record(&observation));
    observation.task = crate::decision::DecisionTask::MemoryAdmission;
    observation.scope = crate::decision::DecisionScope::Conversation;
    assert!(!metrics.record_completed(&observation, 10));
    assert_eq!(
        metrics.snapshot(),
        AdmissionShadowMetricsSnapshot::default()
    );
}

#[test]
fn admission_shadow_token_rejects_invalid_fingerprint_and_database_id() {
    let mut envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    envelope.request.input_fingerprint = "sha256:short".to_owned();
    assert!(AdmissionShadowToken::from_envelope(
        &envelope,
        41,
        "memory-db-a",
        7,
        3,
        ShadowMode::RecordOnly,
    )
    .is_err());

    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    for database_id in ["", "memory db", "memory\ndb", &"x".repeat(129)] {
        assert!(AdmissionShadowToken::from_envelope(
            &envelope,
            41,
            database_id,
            7,
            3,
            ShadowMode::RecordOnly,
        )
        .is_err());
    }
}

#[test]
fn admission_shadow_metrics_wrapper_preserves_primary_and_default_disabled_counts() {
    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary = envelope.primary.clone();
    let provider = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let metrics = AdmissionShadowMetrics::default();
    let mut config = AdmissionShadowConfig::default();
    config.mode = ShadowMode::RecordOnly;
    let observation = observe_admission_shadow_with_metrics(
        &envelope,
        config,
        Some(&provider),
        None,
        Some(&metrics),
    )
    .unwrap()
    .expect("record-only provider produces an observation");
    assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMatch);
    assert_eq!(envelope.primary, primary);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.shadow_started, 1);
    assert_eq!(snapshot.shadow_completed, 1);
    assert_eq!(snapshot.shadow_outcome_match, 1);

    let fault_metrics = AdmissionShadowMetrics::default();
    let fault_provider = FaultAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    let fault = observe_admission_shadow_with_metrics(
        &envelope,
        config,
        Some(&fault_provider),
        None,
        Some(&fault_metrics),
    )
    .unwrap()
    .expect("provider fault is represented");
    assert_eq!(fault.match_kind, ShadowMatchKind::Timeout);
    assert_eq!(fault_metrics.snapshot().shadow_timeout, 1);
    assert_eq!(fault_metrics.snapshot().shadow_completed, 1);

    let unavailable_metrics = AdmissionShadowMetrics::default();
    let unavailable = observe_admission_shadow_with_metrics(
        &envelope,
        config,
        None,
        None,
        Some(&unavailable_metrics),
    )
    .unwrap()
    .expect("missing provider is represented");
    assert_eq!(unavailable.match_kind, ShadowMatchKind::Unavailable);
    assert_eq!(unavailable_metrics.snapshot().shadow_started, 1);
    assert_eq!(unavailable_metrics.snapshot().shadow_completed, 1);
    assert_eq!(unavailable_metrics.snapshot().shadow_unavailable, 1);

    let disabled_metrics = AdmissionShadowMetrics::default();
    let disabled_provider = MatchingAdmissionProvider {
        calls: AtomicUsize::new(0),
    };
    assert!(observe_admission_shadow_with_metrics(
        &envelope,
        AdmissionShadowConfig::default(),
        Some(&disabled_provider),
        None,
        Some(&disabled_metrics),
    )
    .unwrap()
    .is_none());
    assert_eq!(disabled_provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        disabled_metrics.snapshot(),
        AdmissionShadowMetricsSnapshot::default()
    );
}

#[test]
fn decision_adapter_maps_all_three_verdicts_to_fixed_candidates() {
    let admitted = envelope_for("项目仓库必须运行 cargo fmt", false);
    assert_eq!(
        admitted.request.candidate_ids,
        ["admit", "reject", "abstain"]
    );
    assert!(matches!(
        admitted.primary.outcome,
        DecisionOutcome::Choice {
            ref candidate_id,
            confidence
        } if candidate_id == "admit" && confidence > 0.0 && confidence <= 1.0
    ));
    assert!(!admitted.primary.abstain);
    validate_result(&admitted.request, &admitted.primary).unwrap();
    validate_admission_request(&admitted.request).unwrap();

    let rejected = envelope_for("password: hunter2", false);
    assert!(matches!(
        rejected.primary.outcome,
        DecisionOutcome::Choice { ref candidate_id, .. } if candidate_id == "reject"
    ));
    assert!(!rejected.primary.abstain);
    validate_result(&rejected.request, &rejected.primary).unwrap();

    let abstained = envelope_for("你好，谢谢", false);
    assert_eq!(abstained.primary.outcome, DecisionOutcome::Abstain);
    assert!(abstained.primary.abstain);
    validate_result(&abstained.request, &abstained.primary).unwrap();
}

#[test]
fn decision_adapter_fingerprint_is_stable_and_payload_is_raw_free() {
    let marker = "ADMISSION_RAW_MARKER";
    let first = envelope_for("我的稳定偏好是 fish", false);
    let second = envelope_for("完全不同的原文", false);
    assert_eq!(
        first.request.input_fingerprint,
        second.request.input_fingerprint
    );
    let encoded = serde_json::to_string(&first.request.payload).unwrap();
    for forbidden in [
        marker,
        "diary",
        "user",
        "assistant",
        "generated",
        "owner",
        "profile",
        "path",
        "secret",
        "password",
        "credential",
        "token",
    ] {
        assert!(!encoded.to_ascii_lowercase().contains(forbidden));
    }
    assert_eq!(
        first.request.payload["schema_version"],
        json!(MEMORY_ADMISSION_PAYLOAD_SCHEMA)
    );
    assert_eq!(first.request.payload["lifecycle_state"], json!("candidate"));
}

#[test]
fn decision_adapter_rejects_invalid_metadata_and_candidate_results() {
    assert!(AdmissionDecisionRequestBuilder::new(
        AdmissionSourceClass::Standalone,
        false,
        "contains spaces",
        80,
    )
    .is_err());

    let forced = deterministic_admission_text("闲聊", true);
    assert!(AdmissionDecisionRequestBuilder::new(
        AdmissionSourceClass::Standalone,
        false,
        "admission-rules-1",
        80,
    )
    .unwrap()
    .build(&forced)
    .is_err());
    let ordinary = deterministic_admission_text("项目仓库必须运行 cargo fmt", false);
    assert!(AdmissionDecisionRequestBuilder::new(
        AdmissionSourceClass::Standalone,
        true,
        "admission-rules-1",
        80,
    )
    .unwrap()
    .build(&ordinary)
    .is_err());

    let envelope = envelope_for("以后记住这个", false);
    validate_admission_request(&envelope.request).unwrap();
    let mut unknown = envelope.primary.clone();
    unknown.outcome = DecisionOutcome::Choice {
        candidate_id: "unknown".to_owned(),
        confidence: 1.0,
    };
    assert!(matches!(
        validate_result(&envelope.request, &unknown),
        Err(DecisionError::UnknownCandidate(_))
    ));
    let mut out_of_range = envelope.primary.clone();
    out_of_range.outcome = DecisionOutcome::Choice {
        candidate_id: "admit".to_owned(),
        confidence: 1.1,
    };
    assert!(validate_result(&envelope.request, &out_of_range).is_err());
}

#[test]
fn sensitive_source_cannot_be_promoted_by_organizer_output() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.plugins.memory.diary_batch_size = 1;
    let store = MemoryStore::new(&config, &test_paths(&temp));
    assert!(record_turn(&store, "password: hunter2", "请记住这个凭据"));
    let batch = store.next_organization_batch().unwrap().unwrap();
    let source_id = batch.diaries[0].id;
    store
        .apply_organized_batch(
            &batch,
            OrganizedOutput {
                knowledge: vec![KnowledgeAction {
                    operation: "create".to_string(),
                    target_id: None,
                    memory_type: "fact".to_string(),
                    content: "项目配置很重要".to_string(),
                    truth_status: "reported".to_string(),
                    importance: 3,
                    confidence: 0.9,
                    visibility: VISIBILITY_PRIVILEGED.to_string(),
                    subjects: Vec::new(),
                    tags: Vec::new(),
                    diary_ids: vec![source_id],
                }],
                long_diaries: vec![LongDiaryDraft {
                    content: "不应保存的长期摘要".to_string(),
                    importance: 3,
                    confidence: 0.9,
                    visibility: VISIBILITY_PRIVILEGED.to_string(),
                    subjects: Vec::new(),
                    tags: Vec::new(),
                    diary_ids: vec![source_id],
                }],
            },
        )
        .unwrap();
    let conn = store.data_conn().unwrap();
    assert_eq!(count_rows(&conn, "facts").unwrap(), 0);
    assert_eq!(
        count_where(&conn, "episodes", "retention='long_term'").unwrap(),
        0
    );
    let reason: String = conn
        .query_row(
            "SELECT reason_code FROM memory_lifecycle_events WHERE memory_id=?1 AND to_state='rejected'",
            [source_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "organizer_rejected:sensitive_credential");
}

#[test]
fn g5_05_deterministic_admission_evaluation_matrix_is_replayable() {
    #[derive(Clone, Copy)]
    struct Case {
        id: &'static str,
        text: &'static str,
        force_long_term: bool,
        verdict: AdmissionVerdict,
        class: AdmissionClass,
    }

    let cases = [
        Case {
            id: "important-project-constraint",
            text: "项目仓库必须运行 cargo fmt",
            force_long_term: false,
            verdict: AdmissionVerdict::Admit,
            class: AdmissionClass::ProjectConstraint,
        },
        Case {
            id: "chitchat-no-long-term-signal",
            text: "你好，谢谢",
            force_long_term: false,
            verdict: AdmissionVerdict::Abstain,
            class: AdmissionClass::Ambiguous,
        },
        Case {
            id: "sensitive-password",
            text: "password: <redacted>",
            force_long_term: false,
            verdict: AdmissionVerdict::Reject,
            class: AdmissionClass::SensitiveCredential,
        },
        Case {
            id: "sensitive-secret",
            text: "这是一个 secret",
            force_long_term: false,
            verdict: AdmissionVerdict::Reject,
            class: AdmissionClass::SensitiveSecret,
        },
        Case {
            id: "stable-preference",
            text: "我的终端偏好是使用 fish",
            force_long_term: false,
            verdict: AdmissionVerdict::Admit,
            class: AdmissionClass::StablePreference,
        },
        Case {
            id: "forced-sensitive-still-rejected",
            text: "请记住 password: <redacted>",
            force_long_term: true,
            verdict: AdmissionVerdict::Reject,
            class: AdmissionClass::SensitiveCredential,
        },
    ];

    let mut replay = Vec::new();
    let mut admit_count = 0;
    let mut reject_count = 0;
    let mut abstain_count = 0;
    for case in cases {
        let decision = deterministic_admission_text(case.text, case.force_long_term);
        assert_eq!(decision.verdict, case.verdict, "case {}", case.id);
        assert_eq!(decision.class, case.class, "case {}", case.id);
        match decision.verdict {
            AdmissionVerdict::Admit => admit_count += 1,
            AdmissionVerdict::Reject => reject_count += 1,
            AdmissionVerdict::Abstain => abstain_count += 1,
        }
        replay.push((case.id, decision.verdict, decision.class));
    }
    assert_eq!((admit_count, reject_count, abstain_count), (2, 3, 1));

    let second_pass = cases
        .into_iter()
        .map(|case| {
            let decision = deterministic_admission_text(case.text, case.force_long_term);
            (case.id, decision.verdict, decision.class)
        })
        .collect::<Vec<_>>();
    assert_eq!(replay, second_pass);

    let envelope = envelope_for("项目仓库必须运行 cargo fmt", false);
    let primary_before = envelope.primary.clone();
    validate_admission_request(&envelope.request).unwrap();
    validate_result(&envelope.request, &envelope.primary).unwrap();
    assert_eq!(envelope.primary, primary_before);
}
