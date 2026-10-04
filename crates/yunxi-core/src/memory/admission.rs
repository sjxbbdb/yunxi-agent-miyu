//! Deterministic admission for automatically organized short diaries.
//!
//! This module deliberately has no model/provider dependency.  It classifies the
//! source record before an organizer result can be written, and exposes only a
//! metadata value for a future read-only decision consumer.  Neither decisions
//! nor metadata retain the diary text.

use super::{content_digest, ShortDiaryRecord};
use crate::decision::{
    validate_result, DecisionCapability, DecisionError, DecisionOutcome, DecisionReason,
    DecisionRequest, DecisionResult, DecisionScope, DecisionTask, DECISION_SCHEMA_V1,
};
use crate::decision_shadow::{
    observe_with_budget, observe_with_queue, ShadowBudget, ShadowDecisionProvider, ShadowMode,
    ShadowObservation, ShadowQueue,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

pub(crate) const ADMISSION_SCHEMA_VERSION: u16 = 1;

/// Version of the allow-listed memory-admission payload.  This is deliberately
/// separate from the generic decision protocol version.
pub(crate) const MEMORY_ADMISSION_PAYLOAD_SCHEMA: &str = "memory.admission.v1";
const MEMORY_ADMISSION_CANDIDATES: [&str; 3] = ["admit", "reject", "abstain"];
const MAX_ADMISSION_RULES_VERSION_BYTES: usize = 64;
pub(crate) const ADMISSION_RULES_VERSION: &str = "admission-rules-1";
pub(crate) const ADMISSION_SHADOW_DEADLINE_MS: u64 = 80;

/// Memory-local controls for the optional admission observer.
///
/// This intentionally is not part of the application configuration: the first
/// consumer has no user-facing or persisted switch.  The default keeps the
/// observer fully disabled while retaining bounded values for explicit tests or
/// a future in-memory caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AdmissionShadowConfig {
    pub(crate) mode: ShadowMode,
    pub(crate) budget: ShadowBudget,
    pub(crate) cancelled: bool,
}

impl Default for AdmissionShadowConfig {
    fn default() -> Self {
        Self {
            mode: ShadowMode::Disabled,
            budget: ShadowBudget {
                deadline_ms: ADMISSION_SHADOW_DEADLINE_MS,
                queue_slots: 1,
            },
            cancelled: false,
        }
    }
}

/// In-memory counters for the admission shadow observer.
///
/// These counters are deliberately owned by the caller.  They are not part of
/// application configuration, are never persisted, and are not attached to a
/// replay payload or a [`ShadowObservation`].  Atomics keep recording safe for
/// callers that share one metrics value without introducing a runtime or a
/// storage dependency.
#[derive(Debug, Default)]
pub(crate) struct AdmissionShadowMetrics {
    shadow_started: AtomicU64,
    shadow_completed: AtomicU64,
    shadow_exact_match: AtomicU64,
    shadow_outcome_match: AtomicU64,
    shadow_outcome_mismatch: AtomicU64,
    shadow_both_abstain: AtomicU64,
    shadow_invalid_shadow: AtomicU64,
    shadow_timeout: AtomicU64,
    shadow_cancelled: AtomicU64,
    shadow_unavailable: AtomicU64,
    shadow_privacy_rejected: AtomicU64,
    shadow_queue_full: AtomicU64,
    shadow_stale_fingerprint: AtomicU64,
    latency_buckets: [AtomicU64; LATENCY_BUCKET_UPPER_BOUNDS_MS.len()],
}

const LATENCY_BUCKET_UPPER_BOUNDS_MS: [u64; 8] = [0, 1, 5, 10, 50, 100, 500, u64::MAX];

/// Stable, payload-free snapshot of [`AdmissionShadowMetrics`].
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct AdmissionShadowMetricsSnapshot {
    pub(crate) shadow_started: u64,
    pub(crate) shadow_completed: u64,
    pub(crate) shadow_exact_match: u64,
    pub(crate) shadow_outcome_match: u64,
    pub(crate) shadow_outcome_mismatch: u64,
    pub(crate) shadow_both_abstain: u64,
    pub(crate) shadow_invalid_shadow: u64,
    pub(crate) shadow_timeout: u64,
    pub(crate) shadow_cancelled: u64,
    pub(crate) shadow_unavailable: u64,
    pub(crate) shadow_privacy_rejected: u64,
    pub(crate) shadow_queue_full: u64,
    pub(crate) shadow_stale_fingerprint: u64,
    pub(crate) shadow_latency_p50_ms: u64,
    pub(crate) shadow_latency_p95_ms: u64,
    pub(crate) shadow_latency_p99_ms: u64,
}

impl AdmissionShadowMetrics {
    pub(crate) fn record_started(&self) {
        self.shadow_started.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one completed, validated admission observation and its latency.
    /// Returns false for observations belonging to another consumer.
    pub(crate) fn record_completed(
        &self,
        observation: &ShadowObservation,
        elapsed_ms: u64,
    ) -> bool {
        if observation.task != DecisionTask::MemoryAdmission
            || observation.scope != DecisionScope::Memory
        {
            return false;
        }
        self.shadow_completed.fetch_add(1, Ordering::Relaxed);
        self.record_kind(observation);
        self.latency_buckets[latency_bucket(elapsed_ms)].fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Record exactly one classification from an admission observation when it
    /// belongs to this consumer. Other decision consumers are ignored.
    pub(crate) fn record(&self, observation: &ShadowObservation) -> bool {
        if observation.task != DecisionTask::MemoryAdmission
            || observation.scope != DecisionScope::Memory
        {
            return false;
        }
        self.record_kind(observation);
        true
    }

    fn record_kind(&self, observation: &ShadowObservation) {
        let counter = match observation.match_kind {
            crate::decision_shadow::ShadowMatchKind::ExactMatch => &self.shadow_exact_match,
            crate::decision_shadow::ShadowMatchKind::OutcomeMatch => &self.shadow_outcome_match,
            crate::decision_shadow::ShadowMatchKind::OutcomeMismatch => {
                &self.shadow_outcome_mismatch
            }
            crate::decision_shadow::ShadowMatchKind::BothAbstain => &self.shadow_both_abstain,
            crate::decision_shadow::ShadowMatchKind::InvalidShadow => &self.shadow_invalid_shadow,
            crate::decision_shadow::ShadowMatchKind::Timeout => &self.shadow_timeout,
            crate::decision_shadow::ShadowMatchKind::Cancelled => &self.shadow_cancelled,
            crate::decision_shadow::ShadowMatchKind::Unavailable => &self.shadow_unavailable,
            crate::decision_shadow::ShadowMatchKind::PrivacyRejected => {
                &self.shadow_privacy_rejected
            }
            crate::decision_shadow::ShadowMatchKind::QueueFull => &self.shadow_queue_full,
            crate::decision_shadow::ShadowMatchKind::StaleFingerprint => {
                &self.shadow_stale_fingerprint
            }
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Return a fixed-shape snapshot. Individual atomic loads are deliberately
    /// non-transactional; a concurrent caller may observe different moments.
    pub(crate) fn snapshot(&self) -> AdmissionShadowMetricsSnapshot {
        AdmissionShadowMetricsSnapshot {
            shadow_started: self.shadow_started.load(Ordering::Relaxed),
            shadow_completed: self.shadow_completed.load(Ordering::Relaxed),
            shadow_exact_match: self.shadow_exact_match.load(Ordering::Relaxed),
            shadow_outcome_match: self.shadow_outcome_match.load(Ordering::Relaxed),
            shadow_outcome_mismatch: self.shadow_outcome_mismatch.load(Ordering::Relaxed),
            shadow_both_abstain: self.shadow_both_abstain.load(Ordering::Relaxed),
            shadow_invalid_shadow: self.shadow_invalid_shadow.load(Ordering::Relaxed),
            shadow_timeout: self.shadow_timeout.load(Ordering::Relaxed),
            shadow_cancelled: self.shadow_cancelled.load(Ordering::Relaxed),
            shadow_unavailable: self.shadow_unavailable.load(Ordering::Relaxed),
            shadow_privacy_rejected: self.shadow_privacy_rejected.load(Ordering::Relaxed),
            shadow_queue_full: self.shadow_queue_full.load(Ordering::Relaxed),
            shadow_stale_fingerprint: self.shadow_stale_fingerprint.load(Ordering::Relaxed),
            shadow_latency_p50_ms: self.percentile(50),
            shadow_latency_p95_ms: self.percentile(95),
            shadow_latency_p99_ms: self.percentile(99),
        }
    }

    fn percentile(&self, percentile: u64) -> u64 {
        let counts: Vec<u64> = self
            .latency_buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .collect();
        percentile_bucket(&counts, percentile)
    }
}

fn latency_bucket(elapsed_ms: u64) -> usize {
    LATENCY_BUCKET_UPPER_BOUNDS_MS
        .iter()
        .position(|upper_bound| elapsed_ms <= *upper_bound)
        .unwrap_or(LATENCY_BUCKET_UPPER_BOUNDS_MS.len() - 1)
}

fn percentile_bucket(counts: &[u64], percentile: u64) -> u64 {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return 0;
    }
    let rank = ((total.saturating_mul(percentile) + 99) / 100).max(1);
    let mut cumulative: u64 = 0;
    for (index, count) in counts.iter().enumerate() {
        cumulative = cumulative.saturating_add(*count);
        if cumulative >= rank {
            return LATENCY_BUCKET_UPPER_BOUNDS_MS[index];
        }
    }
    *LATENCY_BUCKET_UPPER_BOUNDS_MS.last().unwrap_or(&0)
}

/// Private identity carried by a future asynchronous admission response.
///
/// This type intentionally does not implement `Serialize`; it cannot be
/// copied into a replay payload or an observation by accident.  It contains
/// only bounded protocol and lifecycle metadata, never diary content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionShadowToken {
    task: DecisionTask,
    scope: DecisionScope,
    input_fingerprint: String,
    diary_id: i64,
    batch_database_id: String,
    batch_generation: i64,
    consumer_epoch: u64,
    mode: ShadowMode,
}

/// Current admission context used to validate a private response token.
///
/// This is an in-memory comparison value only.  It has no database handle and
/// does not participate in decision request or observation serialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionShadowContext {
    pub(crate) task: DecisionTask,
    pub(crate) scope: DecisionScope,
    pub(crate) input_fingerprint: String,
    pub(crate) diary_id: i64,
    pub(crate) batch_database_id: String,
    pub(crate) batch_generation: i64,
    pub(crate) consumer_epoch: u64,
    pub(crate) mode: ShadowMode,
}

impl AdmissionShadowContext {
    pub(crate) fn new(
        task: DecisionTask,
        scope: DecisionScope,
        input_fingerprint: impl Into<String>,
        diary_id: i64,
        batch_database_id: impl Into<String>,
        batch_generation: i64,
        consumer_epoch: u64,
        mode: ShadowMode,
    ) -> Self {
        Self {
            task,
            scope,
            input_fingerprint: input_fingerprint.into(),
            diary_id,
            batch_database_id: batch_database_id.into(),
            batch_generation,
            consumer_epoch,
            mode,
        }
    }
}

impl AdmissionShadowToken {
    pub(crate) fn from_envelope(
        envelope: &AdmissionDecisionEnvelope,
        diary_id: i64,
        batch_database_id: impl Into<String>,
        batch_generation: i64,
        consumer_epoch: u64,
        mode: ShadowMode,
    ) -> Result<Self, DecisionError> {
        validate_admission_request(&envelope.request)?;
        validate_result(&envelope.request, &envelope.primary)?;
        if diary_id < 0 || batch_generation < 0 {
            return Err(DecisionError::PrivacyRejected);
        }
        let fingerprint = &envelope.request.input_fingerprint;
        if !is_sha256_fingerprint(fingerprint) {
            return Err(DecisionError::PrivacyRejected);
        }
        let batch_database_id = batch_database_id.into();
        if !is_safe_database_id(&batch_database_id) {
            return Err(DecisionError::PrivacyRejected);
        }
        Ok(Self {
            task: envelope.request.task,
            scope: envelope.request.scope,
            input_fingerprint: fingerprint.clone(),
            diary_id,
            batch_database_id,
            batch_generation,
            consumer_epoch,
            mode,
        })
    }

    pub(crate) fn matches(&self, current: &AdmissionShadowContext) -> bool {
        self.task == current.task
            && self.scope == current.scope
            && self.input_fingerprint == current.input_fingerprint
            && self.diary_id == current.diary_id
            && self.batch_database_id == current.batch_database_id
            && self.batch_generation == current.batch_generation
            && self.consumer_epoch == current.consumer_epoch
            && self.mode == current.mode
    }

    pub(crate) fn matches_context(&self, current: &AdmissionShadowContext) -> bool {
        self.matches(current)
    }

    pub(crate) fn matches_envelope(
        &self,
        envelope: &AdmissionDecisionEnvelope,
        diary_id: i64,
        batch_database_id: &str,
        batch_generation: i64,
        consumer_epoch: u64,
        mode: ShadowMode,
    ) -> bool {
        let Ok(()) = validate_admission_request(&envelope.request) else {
            return false;
        };
        if validate_result(&envelope.request, &envelope.primary).is_err() {
            return false;
        }
        self.matches_current(
            envelope.request.task,
            envelope.request.scope,
            &envelope.request.input_fingerprint,
            diary_id,
            batch_database_id,
            batch_generation,
            consumer_epoch,
            mode,
        )
    }

    pub(crate) fn matches_current(
        &self,
        task: DecisionTask,
        scope: DecisionScope,
        input_fingerprint: &str,
        diary_id: i64,
        batch_database_id: &str,
        batch_generation: i64,
        consumer_epoch: u64,
        mode: ShadowMode,
    ) -> bool {
        self.matches(&AdmissionShadowContext::new(
            task,
            scope,
            input_fingerprint,
            diary_id,
            batch_database_id,
            batch_generation,
            consumer_epoch,
            mode,
        ))
    }
}

/// Admit a shadow response only when it still belongs to the current context.
///
/// Callers must pass this gate immediately before applying any response.  A
/// stale response is rejected with [`DecisionError::StaleFingerprint`] and
/// must be discarded (the caller may record the stale result); it never grants
/// write permission and this function performs no callback, database write, or
/// scheduler/organizer interaction.
pub(crate) fn admit_admission_shadow_response(
    token: &AdmissionShadowToken,
    current: &AdmissionShadowContext,
) -> Result<(), DecisionError> {
    if token.matches(current) {
        Ok(())
    } else {
        Err(DecisionError::StaleFingerprint)
    }
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_safe_database_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionVerdict {
    Admit,
    Reject,
    Abstain,
}

impl AdmissionVerdict {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Admit => "admit",
            Self::Reject => "reject",
            Self::Abstain => "abstain",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionClass {
    Explicit,
    StablePreference,
    ProjectConstraint,
    SystemConstraint,
    FutureValue,
    SensitiveCredential,
    SensitiveSecret,
    OneTimeToken,
    Ephemeral,
    Ambiguous,
}

impl AdmissionClass {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::StablePreference => "stable_preference",
            Self::ProjectConstraint => "project_constraint",
            Self::SystemConstraint => "system_constraint",
            Self::FutureValue => "future_value",
            Self::SensitiveCredential => "sensitive_credential",
            Self::SensitiveSecret => "sensitive_secret",
            Self::OneTimeToken => "one_time_token",
            Self::Ephemeral => "ephemeral",
            Self::Ambiguous => "ambiguous",
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionSensitivity {
    None,
    Sensitive,
}

/// Coarse provenance class used by the decision observer.  It is intentionally
/// not an identifier and never carries diary or owner data.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AdmissionSourceClass {
    Standalone,
    Mixed,
    Unknown,
}

impl AdmissionSourceClass {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Mixed => "mixed",
            Self::Unknown => "unknown",
        }
    }
}

impl AdmissionSensitivity {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Sensitive => "sensitive",
        }
    }
}

/// A stable, bounded deterministic result.  It intentionally contains no
/// source or generated memory text.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AdmissionDecision {
    pub(crate) verdict: AdmissionVerdict,
    pub(crate) reason_code: String,
    pub(crate) class: AdmissionClass,
    pub(crate) sensitivity: AdmissionSensitivity,
    pub(crate) score: u8,
}

/// Raw-free request and deterministic primary result for one admission check.
///
/// The envelope is a local adapter only: callers may observe the request and
/// primary result, but neither value can apply a memory write or carry diary
/// text.  `deterministic_admission` remains the sole source of the primary
/// verdict.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AdmissionDecisionEnvelope {
    pub(crate) request: DecisionRequest,
    pub(crate) primary: DecisionResult,
}

/// Builder for the memory-admission request/primary pair.  Inputs are all
/// bounded metadata; no diary, owner, profile, path, or generated text is
/// accepted by this type.
#[derive(Debug, Clone)]
pub(crate) struct AdmissionDecisionRequestBuilder {
    source_class: AdmissionSourceClass,
    force_long_term: bool,
    admission_rules_version: String,
    deadline_ms: u64,
}

impl AdmissionDecisionRequestBuilder {
    pub(crate) fn new(
        source_class: AdmissionSourceClass,
        force_long_term: bool,
        admission_rules_version: impl Into<String>,
        deadline_ms: u64,
    ) -> Result<Self, DecisionError> {
        let admission_rules_version = admission_rules_version.into();
        validate_admission_rules_version(&admission_rules_version)?;
        Ok(Self {
            source_class,
            force_long_term,
            admission_rules_version,
            deadline_ms,
        })
    }

    pub(crate) fn build(
        &self,
        decision: &AdmissionDecision,
    ) -> Result<AdmissionDecisionEnvelope, DecisionError> {
        // Keep the metadata bit tied to the deterministic primary.  A forced
        // non-sensitive admission always carries this reason; a caller cannot
        // accidentally advertise a different force flag to a future provider.
        if (!self.force_long_term && decision.reason_code == "force_long_term")
            || (self.force_long_term
                && decision.verdict == AdmissionVerdict::Admit
                && decision.reason_code != "force_long_term")
        {
            return Err(DecisionError::InvalidRequest(
                "memory admission force metadata does not match primary".to_owned(),
            ));
        }
        let mut payload = Map::new();
        payload.insert(
            "schema_version".to_owned(),
            Value::String(MEMORY_ADMISSION_PAYLOAD_SCHEMA.to_owned()),
        );
        payload.insert(
            "lifecycle_state".to_owned(),
            Value::String("candidate".to_owned()),
        );
        payload.insert(
            "source_class".to_owned(),
            Value::String(self.source_class.as_str().to_owned()),
        );
        payload.insert(
            "sensitivity".to_owned(),
            Value::String(decision.sensitivity.as_str().to_owned()),
        );
        payload.insert(
            "force_long_term".to_owned(),
            Value::Bool(self.force_long_term),
        );
        payload.insert(
            "admission_rules_version".to_owned(),
            Value::String(self.admission_rules_version.clone()),
        );
        let payload = Value::Object(payload);
        validate_admission_payload(&payload)?;

        let request = DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            memory_admission_candidates(),
            DecisionScope::Memory,
            self.deadline_ms,
            vec![DecisionCapability::ChoiceOnly, DecisionCapability::Abstain],
            payload,
        )?;
        validate_admission_request(&request)?;
        let primary = primary_result(&request, decision)?;
        validate_result(&request, &primary)?;
        Ok(AdmissionDecisionEnvelope { request, primary })
    }
}

/// Observe one raw-free admission envelope without giving the observer any
/// write capability.  A caller-owned queue is used when supplied; otherwise
/// the bounded budget path is sufficient.  The primary result is borrowed and
/// never modified by this adapter.
pub(crate) fn observe_admission_shadow(
    envelope: &AdmissionDecisionEnvelope,
    config: AdmissionShadowConfig,
    provider: Option<&dyn ShadowDecisionProvider>,
    queue: Option<&ShadowQueue>,
) -> Result<Option<ShadowObservation>, DecisionError> {
    match queue {
        Some(queue) => observe_with_queue(
            config.mode,
            &envelope.request,
            &envelope.primary,
            provider,
            config.budget,
            config.cancelled,
            queue,
        ),
        None => observe_with_budget(
            config.mode,
            &envelope.request,
            &envelope.primary,
            provider,
            config.budget,
            config.cancelled,
        ),
    }
}

/// Observe an admission shadow and optionally update caller-owned metrics.
/// Metrics are best effort: they never alter the observer result or primary.
pub(crate) fn observe_admission_shadow_with_metrics(
    envelope: &AdmissionDecisionEnvelope,
    config: AdmissionShadowConfig,
    provider: Option<&dyn ShadowDecisionProvider>,
    queue: Option<&ShadowQueue>,
    metrics: Option<&AdmissionShadowMetrics>,
) -> Result<Option<ShadowObservation>, DecisionError> {
    let should_record = config.mode == ShadowMode::RecordOnly && metrics.is_some();
    if should_record {
        metrics.expect("metrics checked above").record_started();
    }
    let started_at = should_record.then(Instant::now);
    let result = observe_admission_shadow(envelope, config, provider, queue);
    if let (Some(metrics), Some(started_at), Ok(Some(observation))) = (metrics, started_at, &result)
    {
        let elapsed_ms = started_at.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        metrics.record_completed(observation, elapsed_ms);
    }
    result
}

pub(crate) fn validate_admission_request(request: &DecisionRequest) -> Result<(), DecisionError> {
    request.validate()?;
    if request.task != DecisionTask::MemoryAdmission
        || request.scope != DecisionScope::Memory
        || request.candidate_ids != memory_admission_candidates()
        || request.capabilities.as_slice()
            != [DecisionCapability::ChoiceOnly, DecisionCapability::Abstain]
    {
        return Err(DecisionError::InvalidRequest(
            "memory admission request shape mismatch".to_owned(),
        ));
    }
    validate_admission_payload(&request.payload)
}

fn memory_admission_candidates() -> Vec<String> {
    MEMORY_ADMISSION_CANDIDATES
        .iter()
        .map(|candidate| (*candidate).to_owned())
        .collect()
}

fn primary_result(
    request: &DecisionRequest,
    decision: &AdmissionDecision,
) -> Result<DecisionResult, DecisionError> {
    let (outcome, abstain) = match decision.verdict {
        AdmissionVerdict::Admit => (
            DecisionOutcome::Choice {
                candidate_id: "admit".to_owned(),
                confidence: f32::from(decision.score) / 100.0,
            },
            false,
        ),
        AdmissionVerdict::Reject => (
            DecisionOutcome::Choice {
                candidate_id: "reject".to_owned(),
                confidence: f32::from(decision.score) / 100.0,
            },
            false,
        ),
        AdmissionVerdict::Abstain => (DecisionOutcome::Abstain, true),
    };
    let result = DecisionResult {
        schema_version: DECISION_SCHEMA_V1.to_owned(),
        task: DecisionTask::MemoryAdmission,
        input_fingerprint: request.input_fingerprint.clone(),
        outcome,
        abstain,
        reason_code: DecisionReason::DeterministicBaseline,
        provider: "deterministic".to_owned(),
        elapsed_ms: 0,
    };
    Ok(result)
}

fn validate_admission_rules_version(value: &str) -> Result<(), DecisionError> {
    if value.is_empty()
        || value.len() > MAX_ADMISSION_RULES_VERSION_BYTES
        || value
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(DecisionError::PrivacyRejected);
    }
    Ok(())
}

fn validate_admission_payload(payload: &Value) -> Result<(), DecisionError> {
    let Value::Object(map) = payload else {
        return Err(DecisionError::PrivacyRejected);
    };
    const ALLOWLIST: [&str; 6] = [
        "schema_version",
        "lifecycle_state",
        "source_class",
        "sensitivity",
        "force_long_term",
        "admission_rules_version",
    ];
    if map.keys().any(|key| !ALLOWLIST.contains(&key.as_str())) {
        return Err(DecisionError::PrivacyRejected);
    }
    if map.get("schema_version") != Some(&json!(MEMORY_ADMISSION_PAYLOAD_SCHEMA))
        || map.get("lifecycle_state") != Some(&json!("candidate"))
    {
        return Err(DecisionError::PrivacyRejected);
    }
    if !matches!(
        map.get("source_class"),
        Some(Value::String(value)) if matches!(value.as_str(), "standalone" | "mixed" | "unknown")
    ) || !matches!(
        map.get("sensitivity"),
        Some(Value::String(value)) if matches!(value.as_str(), "none" | "sensitive")
    ) || !matches!(map.get("force_long_term"), Some(Value::Bool(_)))
    {
        return Err(DecisionError::PrivacyRejected);
    }
    let Some(Value::String(version)) = map.get("admission_rules_version") else {
        return Err(DecisionError::PrivacyRejected);
    };
    validate_admission_rules_version(version)?;
    Ok(())
}

/// Read-only consumer seam for a future decision component.  This is metadata
/// only: there is no provider, request, or raw content field here.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct AdmissionCandidateMetadata {
    pub(crate) candidate_id: i64,
    pub(crate) source_episode_ids: Vec<i64>,
    pub(crate) owner_scope: String,
    pub(crate) content_digest: String,
    pub(crate) schema_version: u16,
    pub(crate) labels: Vec<String>,
    pub(crate) sensitivity: AdmissionSensitivity,
}

pub(crate) fn deterministic_admission(diary: &ShortDiaryRecord) -> AdmissionDecision {
    let content = format!("{}\n{}", diary.user_message, diary.assistant_message);
    deterministic_admission_text(&content, diary.force_long_term)
}

pub(crate) fn deterministic_admission_text(
    content: &str,
    force_long_term: bool,
) -> AdmissionDecision {
    let normalized = content.to_lowercase();

    // Sensitive material always wins, including over an explicit force flag.
    if looks_like_credential(&normalized)
        || contains_any(
            &normalized,
            &[
                "password",
                "passwd",
                "api key",
                "apikey",
                "api-key",
                "credential",
                "bearer ",
                "authorization:",
                "private key",
                "ssh key",
                "cookie",
                "密码",
                "凭据",
                "授权令牌",
                "密钥",
            ],
        )
    {
        return decision(
            AdmissionVerdict::Reject,
            "sensitive_credential",
            AdmissionClass::SensitiveCredential,
            AdmissionSensitivity::Sensitive,
            0,
        );
    }
    if contains_any(
        &normalized,
        &["secret", "秘密", "机密", "private token", "私密令牌"],
    ) {
        return decision(
            AdmissionVerdict::Reject,
            "sensitive_secret",
            AdmissionClass::SensitiveSecret,
            AdmissionSensitivity::Sensitive,
            0,
        );
    }
    if contains_any(
        &normalized,
        &[
            "one-time token",
            "one time token",
            "otp",
            "验证码",
            "一次性口令",
            "一次性令牌",
            "口令",
        ],
    ) {
        return decision(
            AdmissionVerdict::Reject,
            "sensitive_one_time_token",
            AdmissionClass::OneTimeToken,
            AdmissionSensitivity::Sensitive,
            0,
        );
    }

    if force_long_term {
        return decision(
            AdmissionVerdict::Admit,
            "force_long_term",
            AdmissionClass::Explicit,
            AdmissionSensitivity::None,
            100,
        );
    }
    if contains_any(
        &normalized,
        &[
            "preference",
            "prefer ",
            "i like",
            "i dislike",
            "always use",
            "usually use",
            "default ",
            "喜欢",
            "偏好",
            "习惯",
            "默认",
            "不喜欢",
        ],
    ) {
        return decision(
            AdmissionVerdict::Admit,
            "stable_preference",
            AdmissionClass::StablePreference,
            AdmissionSensitivity::None,
            90,
        );
    }
    if contains_any(
        &normalized,
        &[
            "project",
            "repository",
            "repo ",
            "codebase",
            "constraint",
            "requirement",
            "must ",
            "system constraint",
            "system configuration",
            "项目",
            "工程",
            "仓库",
            "代码库",
            "约束",
            "规范",
            "必须",
            "系统约束",
            "系统配置",
        ],
    ) {
        let class = if contains_any(&normalized, &["system", "系统", "环境", "配置"]) {
            AdmissionClass::SystemConstraint
        } else {
            AdmissionClass::ProjectConstraint
        };
        return decision(
            AdmissionVerdict::Admit,
            if class == AdmissionClass::SystemConstraint {
                "system_constraint"
            } else {
                "project_constraint"
            },
            class,
            AdmissionSensitivity::None,
            85,
        );
    }
    if contains_any(
        &normalized,
        &[
            "for future",
            "future use",
            "next time",
            "keep in mind",
            "long-term",
            "以后",
            "将来",
            "下次",
            "今后",
            "长期",
            "记住",
            "后续",
            "避免再次",
        ],
    ) {
        return decision(
            AdmissionVerdict::Admit,
            "future_value",
            AdmissionClass::FutureValue,
            AdmissionSensitivity::None,
            80,
        );
    }
    if contains_any(
        &normalized,
        &[
            "temporary",
            "temporarily",
            "ephemeral",
            "one-off",
            "one off",
            "just now",
            "临时",
            "暂时",
            "一次性",
            "刚才",
            "今天",
            "现在",
        ],
    ) {
        return decision(
            AdmissionVerdict::Reject,
            "ephemeral",
            AdmissionClass::Ephemeral,
            AdmissionSensitivity::None,
            10,
        );
    }

    decision(
        AdmissionVerdict::Abstain,
        "no_long_term_signal",
        AdmissionClass::Ambiguous,
        AdmissionSensitivity::None,
        0,
    )
}

pub(crate) fn candidate_metadata(
    diary: &ShortDiaryRecord,
    decision: &AdmissionDecision,
) -> AdmissionCandidateMetadata {
    let content = format!("{}\n{}", diary.user_message, diary.assistant_message);
    AdmissionCandidateMetadata {
        candidate_id: diary.id,
        source_episode_ids: vec![diary.id],
        owner_scope: diary
            .owner_principal
            .clone()
            .unwrap_or_else(|| "privileged".to_string()),
        content_digest: content_digest(&content),
        schema_version: ADMISSION_SCHEMA_VERSION,
        labels: vec![
            decision.class.as_str().to_string(),
            decision.reason_code.clone(),
        ],
        sensitivity: decision.sensitivity,
    }
}

pub(crate) fn generated_content_is_sensitive(content: &str) -> bool {
    let decision = deterministic_admission_text(content, false);
    decision.sensitivity == AdmissionSensitivity::Sensitive
}

fn decision(
    verdict: AdmissionVerdict,
    reason_code: &str,
    class: AdmissionClass,
    sensitivity: AdmissionSensitivity,
    score: u8,
) -> AdmissionDecision {
    AdmissionDecision {
        verdict,
        reason_code: reason_code.to_string(),
        class,
        sensitivity,
        score,
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn looks_like_credential(value: &str) -> bool {
    ["sk-", "ghp_", "github_pat_", "aiza", "akia"]
        .iter()
        .any(|prefix| {
            value
                .split_whitespace()
                .any(|token| token.starts_with(prefix) && token.len() >= prefix.len() + 8)
        })
}
