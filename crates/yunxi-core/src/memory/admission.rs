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
