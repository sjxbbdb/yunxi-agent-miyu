//! Stable, replayable decision protocol and deterministic fallback.
//!
//! This module deliberately contains no provider, model, storage, or execution
//! dependencies.  A provider may suggest a result through [`DecisionPort`], but
//! consumers remain responsible for deciding whether to act on that suggestion.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fmt;

/// The only schema understood by this implementation.
pub const DECISION_SCHEMA_V1: &str = "yunxi.decision.v1";

const MAX_CANDIDATE_IDS: usize = 256;
const MAX_CANDIDATE_ID_BYTES: usize = 128;
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_PAYLOAD_DEPTH: usize = 16;
const MAX_PAYLOAD_COLLECTION_ITEMS: usize = 1024;
const MAX_PROVIDER_BYTES: usize = 128;

/// A bounded decision request kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionTask {
    ContextSalience,
    MemoryAdmission,
    RecallRerank,
    TerminalIntent,
    ProactiveRanking,
}

/// Isolation label for a decision request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionScope {
    Conversation,
    Session,
    Memory,
    KnowledgeBase,
    TerminalTurn,
    CompanionJob,
}

/// Operations a provider is permitted to suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCapability {
    RankOnly,
    ChoiceOnly,
    Score,
    Abstain,
}

/// Stable machine-readable reason for a result or fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionReason {
    ProviderChoice,
    ProviderRanking,
    DeterministicBaseline,
    EmptyCandidates,
    InvalidRequest,
    UnsupportedSchema,
    Timeout,
    Cancelled,
    ProviderUnavailable,
    InvalidResult,
    UnknownCandidate,
    StaleFingerprint,
    LowConfidence,
    PrivacyRejected,
    CapabilityViolation,
    InternalError,
}

/// One of the protocol's mutually exclusive provider outcomes.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionOutcome {
    Choice {
        candidate_id: String,
        confidence: f32,
    },
    Ranking {
        ordered_ids: Vec<String>,
    },
    Score {
        score: f32,
        min: f32,
        max: f32,
    },
    Abstain,
}

/// Versioned, fingerprinted input to a decision provider.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    pub schema_version: String,
    pub task: DecisionTask,
    pub candidate_ids: Vec<String>,
    pub scope: DecisionScope,
    pub input_fingerprint: String,
    pub deadline_ms: u64,
    pub capabilities: Vec<DecisionCapability>,
    pub payload: Value,
}

impl DecisionRequest {
    /// Construct and fingerprint a request.  Validation is also performed so
    /// callers cannot accidentally send a malformed request to a port.
    pub fn new(
        task: DecisionTask,
        candidate_ids: Vec<String>,
        scope: DecisionScope,
        deadline_ms: u64,
        capabilities: Vec<DecisionCapability>,
        payload: Value,
    ) -> Result<Self, DecisionError> {
        let mut request = Self {
            schema_version: DECISION_SCHEMA_V1.to_owned(),
            task,
            candidate_ids,
            scope,
            input_fingerprint: String::new(),
            deadline_ms,
            capabilities,
            payload,
        };
        request.input_fingerprint = request.compute_fingerprint()?;
        request.validate()?;
        Ok(request)
    }

    /// Recalculate the request fingerprint without trusting the stored value.
    pub fn compute_fingerprint(&self) -> Result<String, DecisionError> {
        validate_request_shape(self, false)?;
        let canonical = canonical_request_input(self);
        let digest = Sha256::digest(canonical.as_bytes());
        Ok(format!("sha256:{digest:x}"))
    }

    /// Validate schema, bounds, privacy, and the stored fingerprint.
    pub fn validate(&self) -> Result<(), DecisionError> {
        validate_request_shape(self, true)?;
        let expected = self.compute_fingerprint()?;
        if self.input_fingerprint != expected {
            return Err(DecisionError::StaleFingerprint);
        }
        Ok(())
    }
}

/// Versioned result returned by a decision provider.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionResult {
    pub schema_version: String,
    pub task: DecisionTask,
    pub input_fingerprint: String,
    pub outcome: DecisionOutcome,
    pub abstain: bool,
    pub reason_code: DecisionReason,
    pub provider: String,
    pub elapsed_ms: u64,
}

/// Errors exposed by request/result validation.  Details never contain payload
/// text; they are limited to protocol metadata safe for diagnostics.
#[derive(Clone, PartialEq, Eq)]
pub enum DecisionError {
    InvalidRequest(String),
    UnsupportedSchema,
    PrivacyRejected,
    UnknownCandidate(String),
    InvalidResult(String),
    StaleFingerprint,
    CapabilityViolation(String),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(_) => f.write_str("invalid decision request"),
            Self::UnsupportedSchema => f.write_str("unsupported decision schema"),
            Self::PrivacyRejected => f.write_str("decision payload rejected by privacy policy"),
            Self::UnknownCandidate(_) => {
                f.write_str("decision result referenced unknown candidate")
            }
            Self::InvalidResult(_) => f.write_str("invalid decision result"),
            Self::StaleFingerprint => {
                f.write_str("decision result or request has a stale fingerprint")
            }
            Self::CapabilityViolation(_) => f.write_str("decision capability violation"),
        }
    }
}

impl std::error::Error for DecisionError {}

impl fmt::Debug for DecisionOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Choice { confidence, .. } => f
                .debug_struct("Choice")
                .field("confidence", confidence)
                .finish(),
            Self::Ranking { ordered_ids } => f
                .debug_struct("Ranking")
                .field("candidate_count", &ordered_ids.len())
                .finish(),
            Self::Score { score, min, max } => f
                .debug_struct("Score")
                .field("score", score)
                .field("min", min)
                .field("max", max)
                .finish(),
            Self::Abstain => f.write_str("Abstain"),
        }
    }
}

impl fmt::Debug for DecisionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecisionRequest")
            .field(
                "schema_supported",
                &(self.schema_version == DECISION_SCHEMA_V1),
            )
            .field("task", &self.task)
            .field("candidate_count", &self.candidate_ids.len())
            .field("scope", &self.scope)
            .field("fingerprint_present", &!self.input_fingerprint.is_empty())
            .field("deadline_ms", &self.deadline_ms)
            .field("capabilities", &self.capabilities)
            .field("payload", &"<redacted>")
            .finish()
    }
}

impl fmt::Debug for DecisionResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecisionResult")
            .field(
                "schema_supported",
                &(self.schema_version == DECISION_SCHEMA_V1),
            )
            .field("task", &self.task)
            .field("fingerprint_present", &!self.input_fingerprint.is_empty())
            .field("outcome", &self.outcome)
            .field("abstain", &self.abstain)
            .field("reason_code", &self.reason_code)
            .field("provider_present", &!self.provider.is_empty())
            .field("elapsed_ms", &self.elapsed_ms)
            .finish()
    }
}

impl fmt::Debug for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(_) => f.write_str("InvalidRequest(<redacted>)"),
            Self::UnsupportedSchema => f.write_str("UnsupportedSchema"),
            Self::PrivacyRejected => f.write_str("PrivacyRejected"),
            Self::UnknownCandidate(_) => f.write_str("UnknownCandidate(<redacted>)"),
            Self::InvalidResult(_) => f.write_str("InvalidResult(<redacted>)"),
            Self::StaleFingerprint => f.write_str("StaleFingerprint"),
            Self::CapabilityViolation(_) => f.write_str("CapabilityViolation(<redacted>)"),
        }
    }
}

/// Narrow seam implemented by deterministic and future provider backends.
pub trait DecisionPort {
    fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, DecisionError>;
}

/// A replayable fail-closed baseline.  It never infers semantics or executes an action.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeterministicDecisionPort;

impl DecisionPort for DeterministicDecisionPort {
    fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, DecisionError> {
        request.validate()?;
        require_capability(request, DecisionCapability::Abstain)?;
        Ok(DecisionResult {
            schema_version: DECISION_SCHEMA_V1.to_owned(),
            task: request.task,
            input_fingerprint: request.input_fingerprint.clone(),
            outcome: DecisionOutcome::Abstain,
            abstain: true,
            reason_code: DecisionReason::DeterministicBaseline,
            provider: "deterministic".to_owned(),
            elapsed_ms: 0,
        })
    }
}

/// Validate a provider result against its request without executing anything.
pub fn validate_result(
    request: &DecisionRequest,
    result: &DecisionResult,
) -> Result<(), DecisionError> {
    request.validate()?;
    if result.schema_version != DECISION_SCHEMA_V1 {
        return Err(DecisionError::UnsupportedSchema);
    }
    if result.task != request.task {
        return Err(DecisionError::InvalidResult("task mismatch".to_owned()));
    }
    if result.input_fingerprint != request.input_fingerprint {
        return Err(DecisionError::StaleFingerprint);
    }
    if result.elapsed_ms > request.deadline_ms {
        return Err(DecisionError::InvalidResult(
            "elapsed time exceeds deadline".to_owned(),
        ));
    }
    if result.provider.trim().is_empty()
        || result.provider.len() > MAX_PROVIDER_BYTES
        || result.provider.chars().any(char::is_control)
        || result.provider.chars().any(|character| {
            !character.is_ascii_alphanumeric() && !matches!(character, '.' | '_' | ':' | '-')
        })
    {
        return Err(DecisionError::InvalidResult(
            "provider is empty, too long, or contains unsafe characters".to_owned(),
        ));
    }

    match (&result.outcome, result.abstain) {
        (DecisionOutcome::Abstain, true) => {
            require_capability(request, DecisionCapability::Abstain)?;
        }
        (DecisionOutcome::Abstain, false) => {
            return Err(DecisionError::InvalidResult("abstain mismatch".to_owned()));
        }
        (
            DecisionOutcome::Choice {
                candidate_id,
                confidence,
            },
            false,
        ) => {
            require_capability(request, DecisionCapability::ChoiceOnly)?;
            ensure_candidate(request, candidate_id)?;
            if !confidence.is_finite() || !(0.0..=1.0).contains(confidence) {
                return Err(DecisionError::InvalidResult(
                    "confidence out of range".to_owned(),
                ));
            }
        }
        (DecisionOutcome::Choice { .. }, true) => {
            return Err(DecisionError::InvalidResult("abstain mismatch".to_owned()));
        }
        (DecisionOutcome::Ranking { ordered_ids }, false) => {
            require_capability(request, DecisionCapability::RankOnly)?;
            if ordered_ids.is_empty() {
                return Err(DecisionError::InvalidResult(
                    "ranking must contain at least one candidate".to_owned(),
                ));
            }
            let mut seen = std::collections::HashSet::with_capacity(ordered_ids.len());
            for id in ordered_ids {
                ensure_candidate(request, id)?;
                if !seen.insert(id) {
                    return Err(DecisionError::InvalidResult(
                        "duplicate ranking candidate".to_owned(),
                    ));
                }
            }
        }
        (DecisionOutcome::Ranking { .. }, true) => {
            return Err(DecisionError::InvalidResult("abstain mismatch".to_owned()));
        }
        (DecisionOutcome::Score { score, min, max }, false) => {
            require_capability(request, DecisionCapability::Score)?;
            if !score.is_finite() || !min.is_finite() || !max.is_finite() || min > max {
                return Err(DecisionError::InvalidResult(
                    "score bounds are not finite".to_owned(),
                ));
            }
            if *score < *min || *score > *max {
                return Err(DecisionError::InvalidResult(
                    "score out of range".to_owned(),
                ));
            }
        }
        (DecisionOutcome::Score { .. }, true) => {
            return Err(DecisionError::InvalidResult("abstain mismatch".to_owned()));
        }
    }
    Ok(())
}

fn require_capability(
    request: &DecisionRequest,
    capability: DecisionCapability,
) -> Result<(), DecisionError> {
    if request.capabilities.contains(&capability) {
        Ok(())
    } else {
        Err(DecisionError::CapabilityViolation(format!(
            "missing {}",
            serde_json::to_string(&capability).unwrap_or_else(|_| "capability".to_owned())
        )))
    }
}

fn ensure_candidate(request: &DecisionRequest, id: &str) -> Result<(), DecisionError> {
    if request
        .candidate_ids
        .iter()
        .any(|candidate| candidate == id)
    {
        Ok(())
    } else {
        // Candidate IDs are caller-controlled and may themselves be sensitive;
        // never echo an unknown ID in a diagnostic.
        Err(DecisionError::UnknownCandidate(
            "candidate not present in request".to_owned(),
        ))
    }
}

fn validate_request_shape(
    request: &DecisionRequest,
    check_fingerprint: bool,
) -> Result<(), DecisionError> {
    if request.schema_version != DECISION_SCHEMA_V1 {
        return Err(DecisionError::UnsupportedSchema);
    }
    if request.candidate_ids.is_empty() {
        return Err(DecisionError::InvalidRequest(
            "candidate_ids must not be empty".to_owned(),
        ));
    }
    if request.candidate_ids.len() > MAX_CANDIDATE_IDS {
        return Err(DecisionError::InvalidRequest(
            "too many candidate_ids".to_owned(),
        ));
    }
    let mut seen = std::collections::HashSet::with_capacity(request.candidate_ids.len());
    for id in &request.candidate_ids {
        if id.is_empty() || id.len() > MAX_CANDIDATE_ID_BYTES || id.chars().any(char::is_control) {
            return Err(DecisionError::InvalidRequest(
                "candidate ID is empty, too long, or contains control characters".to_owned(),
            ));
        }
        if !seen.insert(id) {
            return Err(DecisionError::InvalidRequest(
                "duplicate candidate ID".to_owned(),
            ));
        }
    }
    if request.capabilities.is_empty() {
        return Err(DecisionError::InvalidRequest(
            "capabilities must not be empty".to_owned(),
        ));
    }
    let mut capabilities = std::collections::HashSet::with_capacity(request.capabilities.len());
    for capability in &request.capabilities {
        if !capabilities.insert(capability) {
            return Err(DecisionError::InvalidRequest(
                "duplicate capability".to_owned(),
            ));
        }
    }
    if !request.payload.is_object() {
        return Err(DecisionError::InvalidRequest(
            "payload must be a JSON object".to_owned(),
        ));
    }
    validate_payload_limits(&request.payload)?;
    if contains_sensitive_key(&request.payload) {
        return Err(DecisionError::PrivacyRejected);
    }
    if check_fingerprint && request.input_fingerprint.is_empty() {
        return Err(DecisionError::InvalidRequest(
            "input_fingerprint must not be empty".to_owned(),
        ));
    }
    Ok(())
}

fn validate_payload_limits(value: &Value) -> Result<(), DecisionError> {
    let encoded_len = serde_json::to_vec(value)
        .map_err(|_| DecisionError::InvalidRequest("payload cannot be serialized".to_owned()))?
        .len();
    if encoded_len > MAX_PAYLOAD_BYTES {
        return Err(DecisionError::InvalidRequest(
            "payload exceeds size limit".to_owned(),
        ));
    }
    let mut collection_items = 0;
    validate_payload_node(value, 0, &mut collection_items)
}

fn validate_payload_node(
    value: &Value,
    depth: usize,
    collection_items: &mut usize,
) -> Result<(), DecisionError> {
    if depth > MAX_PAYLOAD_DEPTH {
        return Err(DecisionError::InvalidRequest(
            "payload exceeds nesting depth limit".to_owned(),
        ));
    }
    match value {
        Value::Array(values) => {
            *collection_items = collection_items.saturating_add(values.len());
            if *collection_items > MAX_PAYLOAD_COLLECTION_ITEMS {
                return Err(DecisionError::InvalidRequest(
                    "payload exceeds collection item limit".to_owned(),
                ));
            }
            for value in values {
                validate_payload_node(value, depth + 1, collection_items)?;
            }
        }
        Value::Object(map) => {
            *collection_items = collection_items.saturating_add(map.len());
            if *collection_items > MAX_PAYLOAD_COLLECTION_ITEMS {
                return Err(DecisionError::InvalidRequest(
                    "payload exceeds collection item limit".to_owned(),
                ));
            }
            for value in map.values() {
                validate_payload_node(value, depth + 1, collection_items)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn contains_sensitive_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let normalized = key
                .chars()
                .filter(|character| character.is_ascii_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>();
            [
                "token",
                "password",
                "secret",
                "privatekey",
                "apikey",
                "credential",
                "profile",
            ]
            .iter()
            .any(|term| normalized.contains(term))
                || contains_sensitive_key(value)
        }),
        Value::Array(values) => values.iter().any(contains_sensitive_key),
        _ => false,
    }
}

fn canonical_request_input(request: &DecisionRequest) -> String {
    let mut root = Map::new();
    root.insert(
        "schema_version".to_owned(),
        Value::String(request.schema_version.clone()),
    );
    root.insert(
        "task".to_owned(),
        serde_json::to_value(request.task).expect("task serializes"),
    );
    root.insert(
        "candidate_ids".to_owned(),
        serde_json::to_value(&request.candidate_ids).expect("IDs serialize"),
    );
    root.insert(
        "scope".to_owned(),
        serde_json::to_value(request.scope).expect("scope serializes"),
    );
    let mut capabilities = request.capabilities.clone();
    capabilities.sort_by_key(|capability| {
        serde_json::to_string(capability).expect("capability serializes")
    });
    root.insert(
        "capabilities".to_owned(),
        serde_json::to_value(capabilities).expect("capabilities serialize"),
    );
    root.insert(
        "deadline_ms".to_owned(),
        serde_json::to_value(request.deadline_ms).expect("deadline serializes"),
    );
    root.insert("payload".to_owned(), request.payload.clone());
    canonical_value(&Value::Object(root))
}

fn canonical_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).expect("string serializes"),
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_value)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let fields = keys.into_iter().map(|key| {
                format!(
                    "{}:{}",
                    serde_json::to_string(key).expect("key serializes"),
                    canonical_value(&map[key])
                )
            });
            format!("{{{}}}", fields.collect::<Vec<_>>().join(","))
        }
    }
}

/// Return the stable digest of a result envelope after the caller has
/// validated it with [`validate_result`].  The shadow seam uses this helper
/// instead of formatting a result with `Debug`, whose representation is not a
/// protocol contract.
pub(crate) fn canonical_result_digest(result: &DecisionResult) -> String {
    let value = serde_json::to_value(result).expect("decision result serializes");
    let canonical = canonical_value(&value);
    let digest = Sha256::digest(canonical.as_bytes());
    format!("sha256:{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(payload: Value) -> DecisionRequest {
        DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["keep".to_owned(), "drop".to_owned()],
            DecisionScope::Memory,
            80,
            vec![
                DecisionCapability::Abstain,
                DecisionCapability::ChoiceOnly,
                DecisionCapability::RankOnly,
                DecisionCapability::Score,
            ],
            payload,
        )
        .expect("valid request")
    }

    #[test]
    fn fingerprint_is_stable_and_object_order_independent() {
        let first = request(json!({"b": 2, "a": [true, 1]}));
        let second = request(json!({"a": [true, 1], "b": 2}));
        assert_eq!(first.input_fingerprint, second.input_fingerprint);
        let reordered = request(json!({"a": [1, true], "b": 2}));
        assert_ne!(first.input_fingerprint, reordered.input_fingerprint);

        let reordered_capabilities = DecisionRequest::new(
            first.task,
            first.candidate_ids.clone(),
            first.scope,
            first.deadline_ms,
            vec![
                DecisionCapability::Score,
                DecisionCapability::RankOnly,
                DecisionCapability::ChoiceOnly,
                DecisionCapability::Abstain,
            ],
            first.payload.clone(),
        )
        .expect("reordered capabilities are valid");
        assert_eq!(
            first.input_fingerprint,
            reordered_capabilities.input_fingerprint
        );
    }

    #[test]
    fn debug_output_redacts_payload_candidate_ids_and_error_details() {
        let request = request(json!({"redacted_text": "private sentence"}));
        let request_debug = format!("{request:?}");
        assert!(!request_debug.contains("private sentence"));
        assert!(!request_debug.contains("keep"));

        let result = DeterministicDecisionPort
            .decide(&request)
            .expect("decision");
        let result_debug = format!("{result:?}");
        assert!(!result_debug.contains("keep"));
        assert!(!result_debug.contains("private sentence"));

        let error = DecisionError::InvalidRequest("private sentence".to_owned());
        assert!(!format!("{error:?}").contains("private sentence"));
        assert!(!format!("{error}").contains("private sentence"));

        let schema_error = DecisionError::UnsupportedSchema;
        assert_eq!(format!("{schema_error}"), "unsupported decision schema");
        assert!(!format!("{schema_error:?}").contains(DECISION_SCHEMA_V1));
    }

    #[test]
    fn deadline_changes_fingerprint_but_zero_deadline_baseline_is_valid() {
        let first = request(json!({"same": true}));
        let second = DecisionRequest::new(
            first.task,
            first.candidate_ids.clone(),
            first.scope,
            first.deadline_ms + 1,
            first.capabilities.clone(),
            first.payload.clone(),
        )
        .expect("valid request");
        assert_ne!(first.input_fingerprint, second.input_fingerprint);

        let immediate = DecisionRequest::new(
            first.task,
            first.candidate_ids,
            first.scope,
            0,
            first.capabilities,
            first.payload,
        )
        .expect("zero deadline is an immediate fallback budget");
        let result = DeterministicDecisionPort
            .decide(&immediate)
            .expect("baseline does not wait");
        assert_eq!(result.elapsed_ms, 0);
        validate_result(&immediate, &result).expect("zero elapsed fits zero deadline");
    }

    #[test]
    fn deterministic_port_abstains_stably() {
        let req = request(json!({"redacted_text": "hello"}));
        let result = DeterministicDecisionPort.decide(&req).expect("decision");
        assert_eq!(result.outcome, DecisionOutcome::Abstain);
        assert!(result.abstain);
        assert_eq!(result.reason_code, DecisionReason::DeterministicBaseline);
        assert_eq!(result.elapsed_ms, 0);
        validate_result(&req, &result).expect("baseline validates");
    }

    #[test]
    fn deterministic_port_requires_abstain_capability() {
        let req = DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["keep".to_owned()],
            DecisionScope::Memory,
            80,
            vec![DecisionCapability::ChoiceOnly],
            json!({"redacted_text": "hello"}),
        )
        .expect("request shape is valid");
        assert!(matches!(
            DeterministicDecisionPort.decide(&req),
            Err(DecisionError::CapabilityViolation(_))
        ));
    }

    #[test]
    fn rejects_bad_candidates_and_sensitive_payload() {
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["x".into()],
            DecisionScope::Memory,
            0,
            vec![],
            json!({})
        )
        .is_err());
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec![],
            DecisionScope::Memory,
            0,
            vec![DecisionCapability::Abstain],
            json!({})
        )
        .is_err());
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["x".into(), "x".into()],
            DecisionScope::Memory,
            0,
            vec![DecisionCapability::Abstain],
            json!({})
        )
        .is_err());
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["x".into()],
            DecisionScope::Memory,
            0,
            vec![DecisionCapability::Abstain],
            json!({"nested": {"api_key": "redacted"}})
        )
        .is_err());
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["x".into()],
            DecisionScope::Memory,
            0,
            vec![DecisionCapability::Abstain],
            json!("not object")
        )
        .is_err());
    }

    #[test]
    fn validates_result_ids_numbers_fingerprint_and_abstain() {
        let req = request(json!({}));
        let base = |outcome, abstain| DecisionResult {
            schema_version: DECISION_SCHEMA_V1.into(),
            task: req.task,
            input_fingerprint: req.input_fingerprint.clone(),
            outcome,
            abstain,
            reason_code: DecisionReason::ProviderChoice,
            provider: "test".into(),
            elapsed_ms: 0,
        };
        assert!(matches!(
            validate_result(
                &req,
                &base(
                    DecisionOutcome::Choice {
                        candidate_id: "new".into(),
                        confidence: 0.5
                    },
                    false
                )
            ),
            Err(DecisionError::UnknownCandidate(_))
        ));
        assert!(validate_result(
            &req,
            &base(
                DecisionOutcome::Ranking {
                    ordered_ids: vec!["keep".into(), "keep".into()]
                },
                false
            )
        )
        .is_err());
        assert!(validate_result(
            &req,
            &base(
                DecisionOutcome::Choice {
                    candidate_id: "keep".into(),
                    confidence: f32::NAN
                },
                false
            )
        )
        .is_err());
        assert!(validate_result(&req, &base(DecisionOutcome::Abstain, false)).is_err());
        assert!(validate_result(
            &req,
            &base(
                DecisionOutcome::Ranking {
                    ordered_ids: Vec::new()
                },
                false
            )
        )
        .is_err());
        let mut over_deadline = base(DecisionOutcome::Abstain, true);
        over_deadline.elapsed_ms = req.deadline_ms + 1;
        assert!(matches!(
            validate_result(&req, &over_deadline),
            Err(DecisionError::InvalidResult(_))
        ));
        let mut bad_provider = base(DecisionOutcome::Abstain, true);
        bad_provider.provider = "bad\nprovider".into();
        assert!(validate_result(&req, &bad_provider).is_err());
        bad_provider.provider = "p".repeat(MAX_PROVIDER_BYTES + 1);
        assert!(validate_result(&req, &bad_provider).is_err());
        let mut stale = base(DecisionOutcome::Abstain, true);
        stale.input_fingerprint = "sha256:deadbeef".into();
        assert!(matches!(
            validate_result(&req, &stale),
            Err(DecisionError::StaleFingerprint)
        ));
    }

    #[test]
    fn payload_limits_reject_size_depth_and_collection_blowups() {
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["candidate".into()],
            DecisionScope::Memory,
            1,
            vec![DecisionCapability::Abstain],
            json!({"text": "x".repeat(MAX_PAYLOAD_BYTES)}),
        )
        .is_err());

        let mut deep = Value::String("leaf".into());
        for _ in 0..=MAX_PAYLOAD_DEPTH {
            deep = json!({"nested": deep});
        }
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["candidate".into()],
            DecisionScope::Memory,
            1,
            vec![DecisionCapability::Abstain],
            json!({"root": deep}),
        )
        .is_err());

        let mut many = Map::new();
        for index in 0..=MAX_PAYLOAD_COLLECTION_ITEMS {
            many.insert(index.to_string(), Value::Bool(true));
        }
        assert!(DecisionRequest::new(
            DecisionTask::MemoryAdmission,
            vec!["candidate".into()],
            DecisionScope::Memory,
            1,
            vec![DecisionCapability::Abstain],
            Value::Object(many),
        )
        .is_err());
    }

    #[test]
    fn serde_roundtrip() {
        let req = request(json!({"kind": "turn"}));
        let encoded = serde_json::to_string(&req).expect("encode");
        let decoded: DecisionRequest = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(req, decoded);
    }
}
