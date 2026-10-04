//! Record-only comparison of a decision result with an optional shadow provider.
//!
//! This module deliberately has no scheduling, storage, network, or model
//! implementation. A shadow provider is called synchronously and its result
//! is reduced to a small observation; the primary result is never changed.
//! Budget and cancellation are cooperative at this seam: preflight checks
//! prevent a call from starting, while a provider that has already started
//! must honor [`ShadowCallContext`] itself. This module never force-stops a
//! blocking provider.

use crate::decision::{
    canonical_result_digest, validate_result, DecisionError, DecisionOutcome, DecisionRequest,
    DecisionResult, DecisionScope, DecisionTask, DECISION_SCHEMA_V1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Controls whether the optional shadow branch is entered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowMode {
    Disabled,
    RecordOnly,
}

impl Default for ShadowMode {
    fn default() -> Self {
        Self::Disabled
    }
}

/// Synchronous limits for one record-only shadow call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowBudget {
    pub deadline_ms: u64,
    pub queue_slots: usize,
}

impl ShadowBudget {
    /// A compatibility budget for the original [`observe`] API.
    pub const fn unlimited() -> Self {
        Self {
            deadline_ms: u64::MAX,
            queue_slots: usize::MAX,
        }
    }
}

/// Cooperative context supplied to a shadow provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowCallContext {
    deadline_ms: u64,
    cancelled: bool,
}

impl ShadowCallContext {
    pub const fn new(deadline_ms: u64, cancelled: bool) -> Self {
        Self {
            deadline_ms,
            cancelled,
        }
    }

    pub const fn deadline_ms(self) -> u64 {
        self.deadline_ms
    }

    pub const fn is_cancelled(self) -> bool {
        self.cancelled
    }
}

/// Bounded caller-owned capacity for synchronous shadow calls.
#[derive(Debug)]
pub struct ShadowQueue {
    capacity: usize,
    in_flight: AtomicUsize,
}

impl ShadowQueue {
    pub const fn new(capacity: usize) -> Self {
        Self {
            capacity,
            in_flight: AtomicUsize::new(0),
        }
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    fn try_acquire(&self) -> Option<ShadowPermit<'_>> {
        if self.capacity == 0 {
            return None;
        }
        let mut current = self.in_flight.load(Ordering::Acquire);
        loop {
            if current >= self.capacity {
                return None;
            }
            match self.in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(ShadowPermit { queue: self }),
                Err(observed) => current = observed,
            }
        }
    }
}

struct ShadowPermit<'a> {
    queue: &'a ShadowQueue,
}

impl Drop for ShadowPermit<'_> {
    fn drop(&mut self) {
        self.queue.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Stable failure classes exposed by a shadow provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowError {
    Unavailable,
    Timeout,
    Cancelled,
    PrivacyRejected,
    QueueFull,
}

impl fmt::Display for ShadowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::PrivacyRejected => "privacy_rejected",
            Self::QueueFull => "queue_full",
        };
        f.write_str(name)
    }
}

impl std::error::Error for ShadowError {}

/// Classification of a primary/shadow comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowMatchKind {
    ExactMatch,
    OutcomeMatch,
    OutcomeMismatch,
    BothAbstain,
    InvalidShadow,
    Timeout,
    Cancelled,
    Unavailable,
    PrivacyRejected,
    QueueFull,
    StaleFingerprint,
}

/// Minimal, payload-free record of one shadow comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowObservation {
    pub schema_version: String,
    pub task: DecisionTask,
    pub scope: DecisionScope,
    pub input_fingerprint: String,
    pub primary_digest: String,
    pub shadow_digest: Option<String>,
    pub match_kind: ShadowMatchKind,
    pub provider_id: String,
    pub elapsed_ms: u64,
}

/// Synchronous seam for a read-only shadow decision provider.
pub trait ShadowDecisionProvider {
    fn observe(&self, request: &DecisionRequest) -> Result<DecisionResult, ShadowError>;

    /// Cooperative extension point. Legacy providers continue to work via
    /// the default implementation; new providers should check this context
    /// before and during their own work.
    fn observe_with_context(
        &self,
        request: &DecisionRequest,
        _context: ShadowCallContext,
    ) -> Result<DecisionResult, ShadowError> {
        self.observe(request)
    }
}

/// Validate the request and primary result, then optionally record a shadow
/// comparison.  Errors from the shadow provider are represented in the
/// observation and never make the primary result fail.
pub fn observe(
    mode: ShadowMode,
    request: &DecisionRequest,
    primary: &DecisionResult,
    provider: Option<&dyn ShadowDecisionProvider>,
) -> Result<Option<ShadowObservation>, DecisionError> {
    observe_with_budget(
        mode,
        request,
        primary,
        provider,
        ShadowBudget::unlimited(),
        false,
    )
}

/// Synchronous, best-effort shadow observation with explicit cancellation and
/// queue/deadline prechecks.  The provider is never called when a precheck
/// classifies the observation.
pub fn observe_with_budget(
    mode: ShadowMode,
    request: &DecisionRequest,
    primary: &DecisionResult,
    provider: Option<&dyn ShadowDecisionProvider>,
    budget: ShadowBudget,
    cancelled: bool,
) -> Result<Option<ShadowObservation>, DecisionError> {
    observe_inner(mode, request, primary, provider, budget, cancelled, None)
}

/// Observe through a caller-owned queue with real bounded capacity. A permit
/// is held only for the synchronous provider call and is released on every
/// return path, including validation failures.
pub fn observe_with_queue(
    mode: ShadowMode,
    request: &DecisionRequest,
    primary: &DecisionResult,
    provider: Option<&dyn ShadowDecisionProvider>,
    budget: ShadowBudget,
    cancelled: bool,
    queue: &ShadowQueue,
) -> Result<Option<ShadowObservation>, DecisionError> {
    observe_inner(
        mode,
        request,
        primary,
        provider,
        budget,
        cancelled,
        Some(queue),
    )
}

fn observe_inner(
    mode: ShadowMode,
    request: &DecisionRequest,
    primary: &DecisionResult,
    provider: Option<&dyn ShadowDecisionProvider>,
    budget: ShadowBudget,
    cancelled: bool,
    queue: Option<&ShadowQueue>,
) -> Result<Option<ShadowObservation>, DecisionError> {
    request.validate()?;
    validate_result(request, primary)?;

    if mode == ShadowMode::Disabled {
        return Ok(None);
    }

    let primary_digest = canonical_result_digest(primary);

    if cancelled {
        return Ok(Some(observation(
            request,
            primary_digest,
            None,
            ShadowMatchKind::Cancelled,
            "unknown".to_owned(),
            0,
        )));
    }
    let effective_deadline_ms = request.deadline_ms.min(budget.deadline_ms);
    if effective_deadline_ms == 0 {
        return Ok(Some(observation(
            request,
            primary_digest,
            None,
            ShadowMatchKind::Timeout,
            "unknown".to_owned(),
            0,
        )));
    }
    if budget.queue_slots == 0 {
        return Ok(Some(observation(
            request,
            primary_digest,
            None,
            ShadowMatchKind::QueueFull,
            "unknown".to_owned(),
            0,
        )));
    }

    let _permit = queue.and_then(ShadowQueue::try_acquire);
    if queue.is_some() && _permit.is_none() {
        return Ok(Some(observation(
            request,
            primary_digest,
            None,
            ShadowMatchKind::QueueFull,
            "unknown".to_owned(),
            0,
        )));
    }

    let Some(provider) = provider else {
        return Ok(Some(observation(
            request,
            primary_digest,
            None,
            ShadowMatchKind::Unavailable,
            "unknown".to_owned(),
            0,
        )));
    };

    let context = ShadowCallContext::new(effective_deadline_ms, false);
    match provider.observe_with_context(request, context) {
        Ok(shadow) => {
            let elapsed_ms = shadow.elapsed_ms;
            if elapsed_ms > effective_deadline_ms {
                return Ok(Some(observation(
                    request,
                    primary_digest,
                    None,
                    ShadowMatchKind::Timeout,
                    "unknown".to_owned(),
                    elapsed_ms,
                )));
            }
            if let Err(error) = validate_result(request, &shadow) {
                let match_kind = match error {
                    DecisionError::StaleFingerprint => ShadowMatchKind::StaleFingerprint,
                    _ => ShadowMatchKind::InvalidShadow,
                };
                return Ok(Some(observation(
                    request,
                    primary_digest,
                    None,
                    match_kind,
                    "unknown".to_owned(),
                    elapsed_ms,
                )));
            }

            let provider_id = shadow.provider.clone();
            let shadow_digest = canonical_result_digest(&shadow);
            let match_kind = if is_abstain(primary) && is_abstain(&shadow) {
                ShadowMatchKind::BothAbstain
            } else if primary == &shadow {
                ShadowMatchKind::ExactMatch
            } else if outcomes_match(&primary.outcome, &shadow.outcome) {
                ShadowMatchKind::OutcomeMatch
            } else {
                ShadowMatchKind::OutcomeMismatch
            };
            Ok(Some(observation(
                request,
                primary_digest,
                Some(shadow_digest),
                match_kind,
                provider_id,
                elapsed_ms,
            )))
        }
        Err(error) => Ok(Some(observation(
            request,
            primary_digest,
            None,
            match_kind_for_error(error),
            "unknown".to_owned(),
            0,
        ))),
    }
}

/// Serialize only the observation envelope in a deterministic field order.
/// No request payload or candidate text is reachable from this type.
pub fn observation_replay_bytes(observation: &ShadowObservation) -> Vec<u8> {
    serde_json::to_vec(observation).expect("shadow observation serializes")
}

/// Return a stable digest for [`observation_replay_bytes`].
pub fn observation_replay_digest(observation: &ShadowObservation) -> String {
    let digest = Sha256::digest(observation_replay_bytes(observation));
    format!("sha256:{digest:x}")
}

fn is_abstain(result: &DecisionResult) -> bool {
    result.abstain && matches!(result.outcome, DecisionOutcome::Abstain)
}

fn outcomes_match(primary: &DecisionOutcome, shadow: &DecisionOutcome) -> bool {
    match (primary, shadow) {
        (
            DecisionOutcome::Choice {
                candidate_id: primary_id,
                ..
            },
            DecisionOutcome::Choice {
                candidate_id: shadow_id,
                ..
            },
        ) => primary_id == shadow_id,
        (
            DecisionOutcome::Ranking {
                ordered_ids: primary_ids,
            },
            DecisionOutcome::Ranking {
                ordered_ids: shadow_ids,
            },
        ) => primary_ids == shadow_ids,
        (
            DecisionOutcome::Score {
                score: primary_score,
                min: primary_min,
                max: primary_max,
            },
            DecisionOutcome::Score {
                score: shadow_score,
                min: shadow_min,
                max: shadow_max,
            },
        ) => {
            primary_score == shadow_score && primary_min == shadow_min && primary_max == shadow_max
        }
        (DecisionOutcome::Abstain, DecisionOutcome::Abstain) => true,
        _ => false,
    }
}

fn match_kind_for_error(error: ShadowError) -> ShadowMatchKind {
    match error {
        ShadowError::Unavailable => ShadowMatchKind::Unavailable,
        ShadowError::Timeout => ShadowMatchKind::Timeout,
        ShadowError::Cancelled => ShadowMatchKind::Cancelled,
        ShadowError::PrivacyRejected => ShadowMatchKind::PrivacyRejected,
        ShadowError::QueueFull => ShadowMatchKind::QueueFull,
    }
}

fn observation(
    request: &DecisionRequest,
    primary_digest: String,
    shadow_digest: Option<String>,
    match_kind: ShadowMatchKind,
    provider_id: String,
    elapsed_ms: u64,
) -> ShadowObservation {
    ShadowObservation {
        schema_version: DECISION_SCHEMA_V1.to_owned(),
        task: request.task,
        scope: request.scope,
        input_fingerprint: request.input_fingerprint.clone(),
        primary_digest,
        shadow_digest,
        match_kind,
        provider_id,
        elapsed_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::{
        DecisionCapability, DecisionPort, DecisionReason, DecisionTask, DeterministicDecisionPort,
    };
    use serde_json::json;
    use std::cell::Cell;

    fn request() -> DecisionRequest {
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
            json!({"redacted_text": "hello"}),
        )
        .expect("valid request")
    }

    fn result(
        request: &DecisionRequest,
        outcome: DecisionOutcome,
        provider: &str,
    ) -> DecisionResult {
        DecisionResult {
            schema_version: DECISION_SCHEMA_V1.to_owned(),
            task: request.task,
            input_fingerprint: request.input_fingerprint.clone(),
            abstain: matches!(outcome, DecisionOutcome::Abstain),
            outcome,
            reason_code: DecisionReason::ProviderChoice,
            provider: provider.to_owned(),
            elapsed_ms: 0,
        }
    }

    struct FakeProvider {
        calls: Cell<u32>,
        response: Result<DecisionResult, ShadowError>,
    }

    impl ShadowDecisionProvider for FakeProvider {
        fn observe(&self, _request: &DecisionRequest) -> Result<DecisionResult, ShadowError> {
            self.calls.set(self.calls.get() + 1);
            self.response.clone()
        }
    }

    struct ContextProvider {
        seen: Cell<Option<ShadowCallContext>>,
        response: Result<DecisionResult, ShadowError>,
    }

    impl ShadowDecisionProvider for ContextProvider {
        fn observe(&self, _request: &DecisionRequest) -> Result<DecisionResult, ShadowError> {
            self.response.clone()
        }

        fn observe_with_context(
            &self,
            _request: &DecisionRequest,
            context: ShadowCallContext,
        ) -> Result<DecisionResult, ShadowError> {
            self.seen.set(Some(context));
            self.response.clone()
        }
    }

    fn primary(request: &DecisionRequest) -> DecisionResult {
        DeterministicDecisionPort.decide(request).expect("baseline")
    }

    #[test]
    fn disabled_does_not_call_provider() {
        let req = request();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(primary(&req)),
        };
        assert_eq!(
            observe(ShadowMode::Disabled, &req, &primary(&req), Some(&fake)).unwrap(),
            None
        );
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn shadow_mode_defaults_to_disabled() {
        assert_eq!(ShadowMode::default(), ShadowMode::Disabled);
    }

    #[test]
    fn exact_match() {
        let req = request();
        let baseline = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "keep".into(),
                confidence: 0.5,
            },
            "primary",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::ExactMatch);
        assert!(observation.shadow_digest.is_some());
    }

    #[test]
    fn outcome_match() {
        let req = request();
        let baseline = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "keep".into(),
                confidence: 0.5,
            },
            "primary",
        );
        let shadow = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "keep".into(),
                confidence: 0.7,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(shadow),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMatch);
    }

    #[test]
    fn outcome_mismatch() {
        let req = request();
        let baseline = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "keep".into(),
                confidence: 0.5,
            },
            "primary",
        );
        let shadow = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "drop".into(),
                confidence: 0.5,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(shadow),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMismatch);
    }

    #[test]
    fn both_abstain() {
        let req = request();
        let baseline = primary(&req);
        let shadow = result(&req, DecisionOutcome::Abstain, "shadow");
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(shadow),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::BothAbstain);
    }

    #[test]
    fn invalid_unknown_and_stale_shadow_are_recorded() {
        let req = request();
        let baseline = primary(&req);
        let mut unknown = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "unknown".into(),
                confidence: 0.5,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(unknown.clone()),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::InvalidShadow);
        unknown.input_fingerprint = "sha256:stale".into();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(unknown),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::StaleFingerprint);
    }

    #[test]
    fn provider_failures_are_classified() {
        let req = request();
        let baseline = primary(&req);
        for (error, kind) in [
            (ShadowError::Timeout, ShadowMatchKind::Timeout),
            (ShadowError::Unavailable, ShadowMatchKind::Unavailable),
        ] {
            let fake = FakeProvider {
                calls: Cell::new(0),
                response: Err(error),
            };
            let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
                .unwrap()
                .unwrap();
            assert_eq!(observation.match_kind, kind);
        }
    }

    #[test]
    fn primary_is_unchanged() {
        let req = request();
        let baseline = primary(&req);
        let before = baseline.clone();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Cancelled),
        };
        let _ = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake));
        assert_eq!(baseline, before);
    }

    #[test]
    fn observation_does_not_contain_payload_or_candidate_text() {
        let req = request();
        let baseline = primary(&req);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Unavailable),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        let encoded = serde_json::to_string(&observation).unwrap();
        assert!(!encoded.contains("redacted_text"));
        assert!(!encoded.contains("keep"));
        assert!(!encoded.contains("drop"));
    }

    #[test]
    fn missing_provider_is_unavailable() {
        let req = request();
        let baseline = primary(&req);
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, None)
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::Unavailable);
    }

    #[test]
    fn cancelled_budget_does_not_call_provider() {
        let req = request();
        let baseline = primary(&req);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let observation = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: 20,
                queue_slots: 1,
            },
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::Cancelled);
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn zero_deadline_budget_does_not_call_provider() {
        let req = request();
        let baseline = primary(&req);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let observation = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: 0,
                queue_slots: 1,
            },
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn queue_full_budget_does_not_call_provider() {
        let req = request();
        let baseline = primary(&req);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let observation = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: 20,
                queue_slots: 0,
            },
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::QueueFull);
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn provider_elapsed_over_budget_is_timeout() {
        let req = request();
        let baseline = primary(&req);
        let mut shadow = baseline.clone();
        shadow.provider = "shadow".to_owned();
        shadow.elapsed_ms = 10;
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(shadow),
        };
        let observation = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: 5,
                queue_slots: 1,
            },
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
        assert_eq!(fake.calls.get(), 1);
        assert_eq!(baseline.provider, "deterministic");
    }

    #[test]
    fn disabled_precedes_budget_prechecks() {
        let req = request();
        let baseline = primary(&req);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Unavailable),
        };
        assert_eq!(
            observe_with_budget(
                ShadowMode::Disabled,
                &req,
                &baseline,
                Some(&fake),
                ShadowBudget {
                    deadline_ms: 0,
                    queue_slots: 0,
                },
                true,
            )
            .unwrap(),
            None
        );
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn observation_replay_bytes_and_digest_are_stable() {
        let req = request();
        let baseline = primary(&req);
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, None)
            .unwrap()
            .unwrap();
        let first_bytes = observation_replay_bytes(&observation);
        let second_bytes = observation_replay_bytes(&observation);
        assert_eq!(first_bytes, second_bytes);
        assert_eq!(
            observation_replay_digest(&observation),
            observation_replay_digest(&observation)
        );
        assert!(!String::from_utf8(first_bytes)
            .expect("replay bytes are JSON")
            .contains("redacted_text"));
    }

    #[test]
    fn budget_observation_does_not_change_primary() {
        let req = request();
        let baseline = primary(&req);
        let before = baseline.clone();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Cancelled),
        };
        let _ = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: 1,
                queue_slots: 1,
            },
            true,
        );
        assert_eq!(baseline, before);
    }

    #[test]
    fn effective_deadline_allows_equal_elapsed_and_passes_context() {
        let req = request();
        let baseline = result(
            &req,
            DecisionOutcome::Choice {
                candidate_id: "keep".into(),
                confidence: 0.5,
            },
            "primary",
        );
        let mut shadow = baseline.clone();
        shadow.provider = "shadow".into();
        shadow.elapsed_ms = req.deadline_ms;
        let provider = ContextProvider {
            seen: Cell::new(None),
            response: Ok(shadow),
        };
        let observation = observe_with_budget(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&provider),
            ShadowBudget {
                deadline_ms: req.deadline_ms + 10,
                queue_slots: 1,
            },
            false,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMatch);
        assert_eq!(provider.seen.get().unwrap().deadline_ms(), req.deadline_ms);
        assert!(!provider.seen.get().unwrap().is_cancelled());
        assert_eq!(
            observation.primary_digest,
            canonical_result_digest(&baseline)
        );
    }

    #[test]
    fn queue_capacity_is_reserved_and_released() {
        let req = request();
        let baseline = primary(&req);
        let queue = ShadowQueue::new(1);
        let held = queue.try_acquire().expect("first permit");
        assert_eq!(queue.in_flight(), 1);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let full = observe_with_queue(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: req.deadline_ms,
                queue_slots: 1,
            },
            false,
            &queue,
        )
        .unwrap()
        .unwrap();
        assert_eq!(full.match_kind, ShadowMatchKind::QueueFull);
        assert_eq!(fake.calls.get(), 0);
        drop(held);
        assert_eq!(queue.in_flight(), 0);
        let completed = observe_with_queue(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: req.deadline_ms,
                queue_slots: 1,
            },
            false,
            &queue,
        )
        .unwrap()
        .unwrap();
        assert_eq!(completed.match_kind, ShadowMatchKind::BothAbstain);
        assert_eq!(fake.calls.get(), 1);
        assert_eq!(queue.in_flight(), 0);
    }

    #[test]
    fn provider_error_classes_are_all_preserved() {
        let req = request();
        let baseline = primary(&req);
        for (error, kind) in [
            (ShadowError::Unavailable, ShadowMatchKind::Unavailable),
            (ShadowError::Timeout, ShadowMatchKind::Timeout),
            (ShadowError::Cancelled, ShadowMatchKind::Cancelled),
            (
                ShadowError::PrivacyRejected,
                ShadowMatchKind::PrivacyRejected,
            ),
            (ShadowError::QueueFull, ShadowMatchKind::QueueFull),
        ] {
            let fake = FakeProvider {
                calls: Cell::new(0),
                response: Err(error),
            };
            let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
                .unwrap()
                .unwrap();
            assert_eq!(observation.match_kind, kind);
            assert_eq!(
                observation.primary_digest,
                canonical_result_digest(&baseline)
            );
        }
    }

    #[test]
    fn invalid_provider_identity_is_redacted() {
        let req = request();
        let baseline = primary(&req);
        let mut shadow = baseline.clone();
        shadow.provider = "bad/provider".into();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(shadow),
        };
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake))
            .unwrap()
            .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::InvalidShadow);
        assert_eq!(observation.provider_id, "unknown");
    }

    #[test]
    fn privacy_rejected_request_never_calls_provider() {
        let mut req = request();
        req.payload = json!({"api_key": "not-recorded"});
        let baseline = primary(&request());
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        assert!(observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake)).is_err());
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn replay_golden_bytes_and_digest_are_stable() {
        let observation = ShadowObservation {
            schema_version: DECISION_SCHEMA_V1.into(),
            task: DecisionTask::MemoryAdmission,
            scope: DecisionScope::Memory,
            input_fingerprint: "sha256:input".into(),
            primary_digest: "sha256:primary".into(),
            shadow_digest: None,
            match_kind: ShadowMatchKind::Unavailable,
            provider_id: "unknown".into(),
            elapsed_ms: 0,
        };
        let bytes = observation_replay_bytes(&observation);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "{\"schema_version\":\"yunxi.decision.v1\",\"task\":\"memory_admission\",\"scope\":\"memory\",\"input_fingerprint\":\"sha256:input\",\"primary_digest\":\"sha256:primary\",\"shadow_digest\":null,\"match_kind\":\"unavailable\",\"provider_id\":\"unknown\",\"elapsed_ms\":0}"
        );
        assert_eq!(
            observation_replay_digest(&observation),
            "sha256:472dfde10227d2825f0157139d7dfcfb7dd56791e053e0a05708ac5d2d82f09f"
        );
    }

    #[test]
    fn privacy_rejection_is_classified_before_provider() {
        let mut req = request();
        req.payload = json!({"token": "not-recorded"});
        let baseline = primary(&request());
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        assert!(matches!(
            observe(ShadowMode::RecordOnly, &req, &baseline, Some(&fake)),
            Err(DecisionError::PrivacyRejected)
        ));
        assert_eq!(fake.calls.get(), 0);
    }

    #[test]
    fn precheck_observations_preserve_primary_digest() {
        let req = request();
        let baseline = primary(&req);
        let expected = canonical_result_digest(&baseline);
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        for (budget, cancelled, kind) in [
            (
                ShadowBudget {
                    deadline_ms: req.deadline_ms,
                    queue_slots: 1,
                },
                true,
                ShadowMatchKind::Cancelled,
            ),
            (
                ShadowBudget {
                    deadline_ms: 0,
                    queue_slots: 1,
                },
                false,
                ShadowMatchKind::Timeout,
            ),
            (
                ShadowBudget {
                    deadline_ms: req.deadline_ms,
                    queue_slots: 0,
                },
                false,
                ShadowMatchKind::QueueFull,
            ),
        ] {
            let observation = observe_with_budget(
                ShadowMode::RecordOnly,
                &req,
                &baseline,
                Some(&fake),
                budget,
                cancelled,
            )
            .unwrap()
            .unwrap();
            assert_eq!(observation.match_kind, kind);
            assert_eq!(observation.primary_digest, expected);
            assert_eq!(baseline, primary(&req));
        }
        assert_eq!(fake.calls.get(), 0);

        let queue = ShadowQueue::new(1);
        let held = queue.try_acquire().unwrap();
        let observation = observe_with_queue(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&fake),
            ShadowBudget {
                deadline_ms: req.deadline_ms,
                queue_slots: 1,
            },
            false,
            &queue,
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::QueueFull);
        assert_eq!(observation.primary_digest, expected);
        drop(held);
        assert_eq!(queue.in_flight(), 0);
    }

    #[test]
    fn queue_permit_releases_on_provider_error_invalid_and_timeout() {
        let req = request();
        let baseline = primary(&req);
        let queue = ShadowQueue::new(1);
        let mut invalid = baseline.clone();
        invalid.provider = "bad/provider".into();
        invalid.elapsed_ms = 0;
        let mut over_budget = baseline.clone();
        over_budget.provider = "shadow".into();
        over_budget.elapsed_ms = req.deadline_ms + 1;
        for response in [Err(ShadowError::Unavailable), Ok(invalid), Ok(over_budget)] {
            let fake = FakeProvider {
                calls: Cell::new(0),
                response,
            };
            let observation = observe_with_queue(
                ShadowMode::RecordOnly,
                &req,
                &baseline,
                Some(&fake),
                ShadowBudget {
                    deadline_ms: req.deadline_ms,
                    queue_slots: 1,
                },
                false,
                &queue,
            )
            .unwrap()
            .unwrap();
            assert!(matches!(
                observation.match_kind,
                ShadowMatchKind::Unavailable
                    | ShadowMatchKind::InvalidShadow
                    | ShadowMatchKind::Timeout
            ));
            assert_eq!(queue.in_flight(), 0);
        }
    }

    #[test]
    fn ranking_and_score_outcomes_are_compared_and_invalidated() {
        let req = request();
        let ranking = result(
            &req,
            DecisionOutcome::Ranking {
                ordered_ids: vec!["keep".into(), "drop".into()],
            },
            "primary",
        );
        let same_ranking = result(
            &req,
            DecisionOutcome::Ranking {
                ordered_ids: vec!["keep".into(), "drop".into()],
            },
            "shadow",
        );
        let reversed_ranking = result(
            &req,
            DecisionOutcome::Ranking {
                ordered_ids: vec!["drop".into(), "keep".into()],
            },
            "shadow",
        );
        for (shadow, kind) in [
            (same_ranking, ShadowMatchKind::OutcomeMatch),
            (reversed_ranking, ShadowMatchKind::OutcomeMismatch),
        ] {
            let fake = FakeProvider {
                calls: Cell::new(0),
                response: Ok(shadow),
            };
            assert_eq!(
                observe(ShadowMode::RecordOnly, &req, &ranking, Some(&fake))
                    .unwrap()
                    .unwrap()
                    .match_kind,
                kind
            );
        }

        let score = result(
            &req,
            DecisionOutcome::Score {
                score: 0.5,
                min: 0.0,
                max: 1.0,
            },
            "primary",
        );
        let score_mismatch = result(
            &req,
            DecisionOutcome::Score {
                score: 0.9,
                min: 0.0,
                max: 1.0,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(score_mismatch),
        };
        assert_eq!(
            observe(ShadowMode::RecordOnly, &req, &score, Some(&fake))
                .unwrap()
                .unwrap()
                .match_kind,
            ShadowMatchKind::OutcomeMismatch
        );

        let score_same = result(
            &req,
            DecisionOutcome::Score {
                score: 0.5,
                min: 0.0,
                max: 1.0,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(score_same),
        };
        assert_eq!(
            observe(ShadowMode::RecordOnly, &req, &score, Some(&fake))
                .unwrap()
                .unwrap()
                .match_kind,
            ShadowMatchKind::OutcomeMatch
        );

        let mut duplicate = ranking.clone();
        duplicate.outcome = DecisionOutcome::Ranking {
            ordered_ids: vec!["keep".into(), "keep".into()],
        };
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(duplicate),
        };
        assert_eq!(
            observe(ShadowMode::RecordOnly, &req, &ranking, Some(&fake))
                .unwrap()
                .unwrap()
                .match_kind,
            ShadowMatchKind::InvalidShadow
        );

        let out_of_range = result(
            &req,
            DecisionOutcome::Score {
                score: 2.0,
                min: 0.0,
                max: 1.0,
            },
            "shadow",
        );
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(out_of_range),
        };
        assert_eq!(
            observe(ShadowMode::RecordOnly, &req, &score, Some(&fake))
                .unwrap()
                .unwrap()
                .match_kind,
            ShadowMatchKind::InvalidShadow
        );
    }
}
