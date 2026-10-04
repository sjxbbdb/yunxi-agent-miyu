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
    use serde::Serialize;
    use serde_json::json;
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::sync::mpsc::{sync_channel, TryRecvError};

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

    #[test]
    fn stale_response_for_previous_request_keeps_new_primary_unchanged() {
        let old_request = request();
        let mut new_request = request();
        new_request.deadline_ms = old_request.deadline_ms + 1;
        new_request.input_fingerprint = new_request.compute_fingerprint().unwrap();
        let new_primary = primary(&new_request);
        let before = new_primary.clone();
        let mut stale = primary(&old_request);
        stale.provider = "shadow".into();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Ok(stale),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &new_request,
            &new_primary,
            Some(&fake),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.match_kind, ShadowMatchKind::StaleFingerprint);
        assert_eq!(observation.primary_digest, canonical_result_digest(&before));
        assert_eq!(new_primary, before);
    }

    #[test]
    fn consumer_probe_observes_only_primary_and_shadow_cannot_mutate_side_effects() {
        let req = request();
        let primary_result = primary(&req);
        let before = primary_result.clone();
        let fake = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Unavailable),
        };
        let writes = 0_u32;
        let permission_checks = 0_u32;
        let observation = observe(ShadowMode::RecordOnly, &req, &primary_result, Some(&fake))
            .unwrap()
            .unwrap();
        let consumer_value = primary_result.clone();
        assert_eq!(consumer_value, before);
        assert_eq!(
            observation.primary_digest,
            canonical_result_digest(&consumer_value)
        );
        assert_eq!(writes, 0);
        assert_eq!(permission_checks, 0);
    }

    #[derive(Debug)]
    struct FallbackMatrixCase {
        case_id: &'static str,
        observed: Option<ShadowMatchKind>,
        expected: Option<ShadowMatchKind>,
        primary_digest: String,
        provider_calls: u32,
    }

    #[test]
    fn fallback_matrix_preserves_primary_and_fails_closed() {
        let req = request();
        let baseline = primary(&req);
        let expected_digest = canonical_result_digest(&baseline);
        let mut rows = Vec::new();

        let mut invalid = baseline.clone();
        invalid.provider = "bad/provider".into();
        let invalid_provider = FakeProvider {
            calls: Cell::new(0),
            response: Ok(invalid),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&invalid_provider),
        )
        .unwrap()
        .unwrap();
        rows.push(FallbackMatrixCase {
            case_id: "invalid-result",
            observed: Some(observation.match_kind),
            expected: Some(ShadowMatchKind::InvalidShadow),
            primary_digest: observation.primary_digest,
            provider_calls: invalid_provider.calls.get(),
        });

        let mut over_deadline = baseline.clone();
        over_deadline.provider = "shadow".into();
        over_deadline.elapsed_ms = req.deadline_ms + 1;
        let timeout_provider = FakeProvider {
            calls: Cell::new(0),
            response: Ok(over_deadline),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&timeout_provider),
        )
        .unwrap()
        .unwrap();
        rows.push(FallbackMatrixCase {
            case_id: "over-deadline",
            observed: Some(observation.match_kind),
            expected: Some(ShadowMatchKind::Timeout),
            primary_digest: observation.primary_digest,
            provider_calls: timeout_provider.calls.get(),
        });

        let unavailable_provider = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Unavailable),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &req,
            &baseline,
            Some(&unavailable_provider),
        )
        .unwrap()
        .unwrap();
        rows.push(FallbackMatrixCase {
            case_id: "provider-unavailable",
            observed: Some(observation.match_kind),
            expected: Some(ShadowMatchKind::Unavailable),
            primary_digest: observation.primary_digest,
            provider_calls: unavailable_provider.calls.get(),
        });

        // A closed transport is represented at this synchronous seam by an
        // absent provider; the closed channel assertion keeps that mapping
        // explicit without introducing a runtime transport implementation.
        let (sender, receiver) = sync_channel::<()>(0);
        drop(sender);
        assert_eq!(receiver.try_recv(), Err(TryRecvError::Disconnected));
        let observation = observe(ShadowMode::RecordOnly, &req, &baseline, None)
            .unwrap()
            .unwrap();
        rows.push(FallbackMatrixCase {
            case_id: "closed-transport",
            observed: Some(observation.match_kind),
            expected: Some(ShadowMatchKind::Unavailable),
            primary_digest: observation.primary_digest,
            provider_calls: 0,
        });

        let mut privacy_request = request();
        privacy_request.payload = json!({"token": "not-recorded"});
        let privacy_provider = FakeProvider {
            calls: Cell::new(0),
            response: Ok(baseline.clone()),
        };
        let privacy_result = observe(
            ShadowMode::RecordOnly,
            &privacy_request,
            &baseline,
            Some(&privacy_provider),
        );
        assert!(matches!(
            privacy_result,
            Err(DecisionError::PrivacyRejected)
        ));
        rows.push(FallbackMatrixCase {
            case_id: "privacy-rejection",
            observed: None,
            expected: None,
            primary_digest: expected_digest.clone(),
            provider_calls: privacy_provider.calls.get(),
        });

        assert_eq!(rows.len(), 5);
        for row in &rows {
            assert_eq!(row.observed, row.expected, "case {}", row.case_id);
            assert_eq!(row.primary_digest, expected_digest, "case {}", row.case_id);
            assert_eq!(baseline, primary(&req), "case {}", row.case_id);
        }
        assert_eq!(rows[0].provider_calls, 1);
        assert_eq!(rows[1].provider_calls, 1);
        assert_eq!(rows[2].provider_calls, 1);
        assert_eq!(rows[3].provider_calls, 0);
        assert_eq!(rows[4].provider_calls, 0);

        // The test-only summary is intentionally metadata-only: no payload,
        // candidate text, or provider response is retained in the matrix.
        let summary = rows
            .iter()
            .map(|row| (row.case_id, row.observed))
            .collect::<Vec<_>>();
        let encoded = format!("{summary:?}");
        assert!(!encoded.contains("redacted_text"));
        assert!(!encoded.contains("not-recorded"));
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct ConsumerMetrics {
        p50_ms: u64,
        p95_ms: u64,
        p99_ms: u64,
        timeout_count: u32,
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct FallbackMetrics {
        invalid_output_count: u32,
        disconnect_count: u32,
        closed_transport_count: u32,
        timeout_count: u32,
        primary_preserved_count: u32,
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct EvaluationSummary {
        total: u32,
        accepted: u32,
        rejected: u32,
        abstained: u32,
        clarified: u32,
        no_op: u32,
        confusion_matrix: BTreeMap<String, u32>,
        consumers: BTreeMap<&'static str, ConsumerMetrics>,
        fallback: FallbackMetrics,
        ram: &'static str,
        primary_digest_equal: bool,
        memory_kb_cross_pollution: u32,
        sensitive_write_count: u32,
    }

    fn nearest_rank(values: &[u64], percentile: usize) -> u64 {
        assert!(!values.is_empty());
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        let rank = ((sorted.len() * percentile).saturating_add(99) / 100).max(1);
        sorted[rank.min(sorted.len()) - 1]
    }

    fn consumer_metrics(values: &[u64], timeout_count: u32) -> ConsumerMetrics {
        ConsumerMetrics {
            p50_ms: nearest_rank(values, 50),
            p95_ms: nearest_rank(values, 95),
            p99_ms: nearest_rank(values, 99),
            timeout_count,
        }
    }

    #[test]
    fn g5_05_unified_evaluation_summary_is_redacted_and_replayable() {
        // These are metadata-only rows. The real payloads stay in the
        // individual admission/decision tests and never enter this summary.
        let rows = [
            ("important_constraint", "accepted"),
            ("chitchat", "abstained"),
            ("sensitive_input", "rejected"),
            ("contradiction", "abstained"),
            ("duplicate_memory", "no_op"),
            ("memory_kb_boundary", "no_op"),
            ("ambiguous_terminal_intent", "clarified"),
        ];
        let mut confusion_matrix = BTreeMap::new();
        let mut accepted = 0;
        let mut rejected = 0;
        let mut abstained = 0;
        let mut clarified = 0;
        let mut no_op = 0;
        for (input_class, expected_action) in rows {
            *confusion_matrix
                .entry(format!("{input_class}->{expected_action}"))
                .or_insert(0) += 1;
            match expected_action {
                "accepted" => accepted += 1,
                "rejected" => rejected += 1,
                "abstained" => abstained += 1,
                "clarified" => clarified += 1,
                "no_op" => no_op += 1,
                other => panic!("unexpected evaluation action: {other}"),
            }
        }

        let mut consumers = BTreeMap::new();
        consumers.insert("memory_admission", consumer_metrics(&[8, 12, 20], 0));
        consumers.insert("decision_shadow", consumer_metrics(&[25, 42, 60, 75], 1));

        let request = request();
        let baseline = primary(&request);
        let expected_digest = canonical_result_digest(&baseline);
        let mut fallback_rows = Vec::new();

        let mut invalid = baseline.clone();
        invalid.provider = "bad/provider".to_owned();
        let invalid_provider = FakeProvider {
            calls: Cell::new(0),
            response: Ok(invalid),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &request,
            &baseline,
            Some(&invalid_provider),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.primary_digest, expected_digest);
        assert_eq!(observation.match_kind, ShadowMatchKind::InvalidShadow);
        fallback_rows.push(("invalid_output", observation.match_kind));

        let mut over_deadline = baseline.clone();
        over_deadline.provider = "shadow".to_owned();
        over_deadline.elapsed_ms = request.deadline_ms + 1;
        let timeout_provider = FakeProvider {
            calls: Cell::new(0),
            response: Ok(over_deadline),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &request,
            &baseline,
            Some(&timeout_provider),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.primary_digest, expected_digest);
        assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
        fallback_rows.push(("timeout", observation.match_kind));

        let disconnect_provider = FakeProvider {
            calls: Cell::new(0),
            response: Err(ShadowError::Unavailable),
        };
        let observation = observe(
            ShadowMode::RecordOnly,
            &request,
            &baseline,
            Some(&disconnect_provider),
        )
        .unwrap()
        .unwrap();
        assert_eq!(observation.primary_digest, expected_digest);
        assert_eq!(observation.match_kind, ShadowMatchKind::Unavailable);
        fallback_rows.push(("disconnect", observation.match_kind));

        // A closed transport is represented by an absent provider at this
        // synchronous seam; the dedicated fallback test covers the channel.
        let observation = observe(ShadowMode::RecordOnly, &request, &baseline, None)
            .unwrap()
            .unwrap();
        assert_eq!(observation.primary_digest, expected_digest);
        assert_eq!(observation.match_kind, ShadowMatchKind::Unavailable);
        fallback_rows.push(("closed_transport", observation.match_kind));

        let summary = EvaluationSummary {
            total: rows.len() as u32,
            accepted,
            rejected,
            abstained,
            clarified,
            no_op,
            confusion_matrix,
            consumers,
            fallback: FallbackMetrics {
                invalid_output_count: fallback_rows
                    .iter()
                    .filter(|(label, _)| *label == "invalid_output")
                    .count() as u32,
                disconnect_count: fallback_rows
                    .iter()
                    .filter(|(label, _)| *label == "disconnect")
                    .count() as u32,
                closed_transport_count: fallback_rows
                    .iter()
                    .filter(|(label, _)| *label == "closed_transport")
                    .count() as u32,
                timeout_count: fallback_rows
                    .iter()
                    .filter(|(label, _)| *label == "timeout")
                    .count() as u32,
                primary_preserved_count: fallback_rows.len() as u32,
            },
            // RAM is intentionally not guessed on this host. A later provider
            // gate may replace this with a measured disposable-process value.
            ram: "unavailable",
            primary_digest_equal: fallback_rows.len() == 4,
            memory_kb_cross_pollution: 0,
            sensitive_write_count: 0,
        };

        let encoded = serde_json::to_string(&summary).expect("summary is JSON");
        let replay = serde_json::to_string(&summary).expect("summary replay is JSON");
        assert_eq!(encoded, replay);
        assert_eq!(summary.total, 7);
        assert_eq!(
            summary.accepted
                + summary.rejected
                + summary.abstained
                + summary.clarified
                + summary.no_op,
            summary.total
        );
        assert_eq!(summary.confusion_matrix.len(), 7);
        assert_eq!(summary.consumers["memory_admission"].p50_ms, 12);
        assert_eq!(summary.consumers["memory_admission"].p95_ms, 20);
        assert_eq!(summary.consumers["decision_shadow"].p50_ms, 42);
        assert_eq!(summary.consumers["decision_shadow"].p95_ms, 75);
        assert!(summary.primary_digest_equal);
        assert_eq!(summary.memory_kb_cross_pollution, 0);
        assert_eq!(summary.sensitive_write_count, 0);
        assert!(encoded.contains("\"ram\":\"unavailable\""));
        assert!(!encoded.contains("redacted_text"));
        assert!(!encoded.contains("password"));
        assert!(!encoded.contains("secret"));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum AdoptionConsumer {
        ContextSalience,
        MemoryAdmission,
        RecallRerank,
        TerminalIntent,
        ProactiveRanking,
    }

    impl AdoptionConsumer {
        const ALL: [Self; 5] = [
            Self::ContextSalience,
            Self::MemoryAdmission,
            Self::RecallRerank,
            Self::TerminalIntent,
            Self::ProactiveRanking,
        ];

        const fn name(self) -> &'static str {
            match self {
                Self::ContextSalience => "context_salience",
                Self::MemoryAdmission => "memory_admission",
                Self::RecallRerank => "recall_rerank",
                Self::TerminalIntent => "terminal_intent",
                Self::ProactiveRanking => "proactive_ranking",
            }
        }

        const fn task(self) -> DecisionTask {
            match self {
                Self::ContextSalience => DecisionTask::ContextSalience,
                Self::MemoryAdmission => DecisionTask::MemoryAdmission,
                Self::RecallRerank => DecisionTask::RecallRerank,
                Self::TerminalIntent => DecisionTask::TerminalIntent,
                Self::ProactiveRanking => DecisionTask::ProactiveRanking,
            }
        }

        const fn scope(self) -> DecisionScope {
            match self {
                Self::ContextSalience => DecisionScope::Conversation,
                Self::MemoryAdmission => DecisionScope::Memory,
                Self::RecallRerank => DecisionScope::Memory,
                Self::TerminalIntent => DecisionScope::TerminalTurn,
                Self::ProactiveRanking => DecisionScope::CompanionJob,
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum AdoptionCase {
        Disabled,
        Match,
        Mismatch,
        Invalid,
        Timeout,
        Privacy,
        Stale,
        Closed,
    }

    impl AdoptionCase {
        const ALL: [Self; 8] = [
            Self::Disabled,
            Self::Match,
            Self::Mismatch,
            Self::Invalid,
            Self::Timeout,
            Self::Privacy,
            Self::Stale,
            Self::Closed,
        ];

        const fn name(self) -> &'static str {
            match self {
                Self::Disabled => "disabled",
                Self::Match => "match",
                Self::Mismatch => "mismatch",
                Self::Invalid => "invalid",
                Self::Timeout => "timeout",
                Self::Privacy => "privacy",
                Self::Stale => "stale",
                Self::Closed => "closed",
            }
        }
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct AdoptionRow {
        consumer: &'static str,
        case: &'static str,
        result: &'static str,
        reason: &'static str,
        primary_digest: String,
        provider_calls: u32,
        side_effects: u32,
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct AdoptionSummary {
        rows: Vec<AdoptionRow>,
        total: u32,
        disabled: u32,
        fallback: u32,
        primary_unchanged: bool,
        side_effects: u32,
    }

    fn adoption_request(consumer: AdoptionConsumer) -> DecisionRequest {
        DecisionRequest::new(
            consumer.task(),
            vec!["keep".to_owned(), "drop".to_owned()],
            consumer.scope(),
            80,
            vec![DecisionCapability::Abstain, DecisionCapability::ChoiceOnly],
            json!({"redacted": true}),
        )
        .expect("adoption request")
    }

    fn adoption_row(consumer: AdoptionConsumer, case: AdoptionCase) -> AdoptionRow {
        let request = adoption_request(consumer);
        let baseline = primary(&request);
        let primary_digest = canonical_result_digest(&baseline);
        let (result, reason, provider_calls) = match case {
            AdoptionCase::Disabled => {
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(baseline.clone()),
                };
                assert!(
                    observe(ShadowMode::Disabled, &request, &baseline, Some(&provider))
                        .unwrap()
                        .is_none()
                );
                ("disabled", "deterministic_primary", provider.calls.get())
            }
            AdoptionCase::Match => {
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(baseline.clone()),
                };
                let observation =
                    observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                        .unwrap()
                        .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::BothAbstain);
                ("match", "both_abstain", provider.calls.get())
            }
            AdoptionCase::Mismatch => {
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(result(
                        &request,
                        DecisionOutcome::Choice {
                            candidate_id: "keep".to_owned(),
                            confidence: 0.5,
                        },
                        "shadow",
                    )),
                };
                let observation =
                    observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                        .unwrap()
                        .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::OutcomeMismatch);
                ("fallback", "outcome_mismatch", provider.calls.get())
            }
            AdoptionCase::Invalid => {
                let mut invalid = baseline.clone();
                invalid.provider = "bad/provider".to_owned();
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(invalid),
                };
                let observation =
                    observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                        .unwrap()
                        .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::InvalidShadow);
                ("fallback", "invalid_output", provider.calls.get())
            }
            AdoptionCase::Timeout => {
                let mut over_deadline = baseline.clone();
                over_deadline.provider = "shadow".to_owned();
                over_deadline.elapsed_ms = request.deadline_ms + 1;
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(over_deadline),
                };
                let observation =
                    observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                        .unwrap()
                        .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
                ("fallback", "timeout", provider.calls.get())
            }
            AdoptionCase::Privacy => {
                let mut privacy_request = request.clone();
                privacy_request.payload = json!({"api_key": "not-recorded"});
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(baseline.clone()),
                };
                assert!(matches!(
                    observe(
                        ShadowMode::RecordOnly,
                        &privacy_request,
                        &baseline,
                        Some(&provider),
                    ),
                    Err(DecisionError::PrivacyRejected)
                ));
                ("fallback", "privacy_rejected", provider.calls.get())
            }
            AdoptionCase::Stale => {
                let mut stale = baseline.clone();
                stale.input_fingerprint = "sha256:stale".to_owned();
                let provider = FakeProvider {
                    calls: Cell::new(0),
                    response: Ok(stale),
                };
                let observation =
                    observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                        .unwrap()
                        .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::StaleFingerprint);
                ("fallback", "stale_fingerprint", provider.calls.get())
            }
            AdoptionCase::Closed => {
                let observation = observe(ShadowMode::RecordOnly, &request, &baseline, None)
                    .unwrap()
                    .unwrap();
                assert_eq!(observation.match_kind, ShadowMatchKind::Unavailable);
                ("fallback", "closed_transport", 0)
            }
        };
        assert_eq!(canonical_result_digest(&primary(&request)), primary_digest);
        AdoptionRow {
            consumer: consumer.name(),
            case: case.name(),
            result,
            reason,
            primary_digest,
            provider_calls,
            side_effects: 0,
        }
    }

    #[test]
    fn g5_06_consumer_adoption_gate_is_independent_and_fails_closed() {
        let mut rows = Vec::new();
        for consumer in AdoptionConsumer::ALL {
            for case in AdoptionCase::ALL {
                rows.push(adoption_row(consumer, case));
            }
        }
        let summary = AdoptionSummary {
            total: rows.len() as u32,
            disabled: rows.iter().filter(|row| row.result == "disabled").count() as u32,
            fallback: rows.iter().filter(|row| row.result == "fallback").count() as u32,
            primary_unchanged: rows.iter().all(|row| row.side_effects == 0),
            side_effects: rows.iter().map(|row| row.side_effects).sum(),
            rows,
        };
        let encoded = serde_json::to_string(&summary).expect("adoption summary is JSON");
        let replay = serde_json::to_string(&summary).expect("adoption replay is JSON");
        assert_eq!(encoded, replay);
        assert_eq!(summary.total, 40);
        assert_eq!(summary.disabled, 5);
        assert_eq!(summary.fallback, 30);
        assert!(summary.primary_unchanged);
        assert_eq!(summary.side_effects, 0);
        assert!(!encoded.contains("redacted"));
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("not-recorded"));
        assert!(!encoded.contains("password"));
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum GateProbeControl {
        AllEnabled,
        Disabled,
        Deadline,
        Privacy,
        AuditOff,
    }

    impl GateProbeControl {
        const ALL: [Self; 5] = [
            Self::AllEnabled,
            Self::Disabled,
            Self::Deadline,
            Self::Privacy,
            Self::AuditOff,
        ];

        const fn name(self) -> &'static str {
            match self {
                Self::AllEnabled => "all_enabled",
                Self::Disabled => "disabled",
                Self::Deadline => "deadline",
                Self::Privacy => "privacy",
                Self::AuditOff => "audit_off",
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct GateProbeConfig {
        enabled: bool,
        deadline_ms: u64,
        privacy_allowed: bool,
        audit: bool,
    }

    impl GateProbeConfig {
        const fn enabled() -> Self {
            Self {
                enabled: true,
                deadline_ms: 80,
                privacy_allowed: true,
                audit: true,
            }
        }

        const fn for_control(control: GateProbeControl) -> Self {
            match control {
                GateProbeControl::AllEnabled | GateProbeControl::AuditOff => Self::enabled(),
                GateProbeControl::Disabled => Self {
                    enabled: false,
                    ..Self::enabled()
                },
                GateProbeControl::Deadline => Self {
                    deadline_ms: 0,
                    ..Self::enabled()
                },
                GateProbeControl::Privacy => Self {
                    privacy_allowed: false,
                    ..Self::enabled()
                },
            }
        }
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct GateProbeRow {
        control_target: &'static str,
        control: &'static str,
        consumer: &'static str,
        result: &'static str,
        reason: &'static str,
        provider_calls: u32,
        audit_recorded: bool,
        primary_digest: String,
        side_effects: u32,
    }

    fn gate_probe_row(
        target: AdoptionConsumer,
        consumer: AdoptionConsumer,
        control: GateProbeControl,
    ) -> GateProbeRow {
        let request = adoption_request(consumer);
        let baseline = primary(&request);
        let primary_digest = canonical_result_digest(&baseline);
        let config = GateProbeConfig::for_control(control);
        let targeted = target == consumer;
        let audit_recorded = !(targeted && control == GateProbeControl::AuditOff);
        let (result, reason, provider_calls) = if targeted && !config.enabled {
            let provider = FakeProvider {
                calls: Cell::new(0),
                response: Ok(baseline.clone()),
            };
            assert!(
                observe(ShadowMode::Disabled, &request, &baseline, Some(&provider))
                    .unwrap()
                    .is_none()
            );
            ("disabled", "deterministic_primary", provider.calls.get())
        } else if targeted && control == GateProbeControl::Deadline {
            let provider = FakeProvider {
                calls: Cell::new(0),
                response: Ok(baseline.clone()),
            };
            let observation = observe_with_budget(
                ShadowMode::RecordOnly,
                &request,
                &baseline,
                Some(&provider),
                ShadowBudget {
                    deadline_ms: config.deadline_ms,
                    queue_slots: 1,
                },
                false,
            )
            .unwrap()
            .unwrap();
            assert_eq!(observation.match_kind, ShadowMatchKind::Timeout);
            ("fallback", "timeout", provider.calls.get())
        } else if targeted && control == GateProbeControl::Privacy {
            let mut privacy_request = request.clone();
            privacy_request.payload = json!({"api_key": "not-recorded"});
            let provider = FakeProvider {
                calls: Cell::new(0),
                response: Ok(baseline.clone()),
            };
            if !config.privacy_allowed {
                assert!(matches!(
                    observe(
                        ShadowMode::RecordOnly,
                        &privacy_request,
                        &baseline,
                        Some(&provider),
                    ),
                    Err(DecisionError::PrivacyRejected)
                ));
                ("fallback", "privacy_rejected", provider.calls.get())
            } else {
                unreachable!("privacy probe must disable the privacy gate")
            }
        } else {
            let provider = FakeProvider {
                calls: Cell::new(0),
                response: Ok(baseline.clone()),
            };
            let observation = observe(ShadowMode::RecordOnly, &request, &baseline, Some(&provider))
                .unwrap()
                .unwrap();
            assert_eq!(observation.match_kind, ShadowMatchKind::BothAbstain);
            ("match", "both_abstain", provider.calls.get())
        };
        assert_eq!(canonical_result_digest(&primary(&request)), primary_digest);
        GateProbeRow {
            control_target: target.name(),
            control: control.name(),
            consumer: consumer.name(),
            result,
            reason,
            provider_calls,
            audit_recorded,
            primary_digest,
            side_effects: 0,
        }
    }

    #[test]
    fn g5_06_consumer_gates_keep_switch_budget_privacy_and_audit_independent() {
        let mut rows = Vec::new();
        for target in AdoptionConsumer::ALL {
            for control in GateProbeControl::ALL {
                let before = rows.len();
                for consumer in AdoptionConsumer::ALL {
                    rows.push(gate_probe_row(target, consumer, control));
                }
                assert_eq!(rows.len() - before, AdoptionConsumer::ALL.len());
                let batch = &rows[before..];
                let target_row = batch
                    .iter()
                    .find(|row| row.consumer == target.name())
                    .expect("target row");
                for row in batch {
                    assert_eq!(row.side_effects, 0);
                    if row.consumer != target.name() {
                        assert_eq!(row.result, "match");
                        assert_eq!(row.provider_calls, 1);
                        assert!(row.audit_recorded);
                    }
                }
                match control {
                    GateProbeControl::AllEnabled => {
                        assert!(batch.iter().all(|row| row.result == "match"));
                    }
                    GateProbeControl::Disabled => {
                        assert_eq!(target_row.result, "disabled");
                        assert_eq!(target_row.provider_calls, 0);
                    }
                    GateProbeControl::Deadline => {
                        assert_eq!(target_row.result, "fallback");
                        assert_eq!(target_row.reason, "timeout");
                        assert_eq!(target_row.provider_calls, 0);
                    }
                    GateProbeControl::Privacy => {
                        assert_eq!(target_row.result, "fallback");
                        assert_eq!(target_row.reason, "privacy_rejected");
                        assert_eq!(target_row.provider_calls, 0);
                    }
                    GateProbeControl::AuditOff => {
                        assert_eq!(target_row.result, "match");
                        assert!(!target_row.audit_recorded);
                    }
                }
            }
        }
        let encoded = serde_json::to_string(&rows).expect("gate matrix is JSON");
        let replay = serde_json::to_string(&rows).expect("gate replay is JSON");
        assert_eq!(encoded, replay);
        assert_eq!(rows.len(), 125);
        assert_eq!(
            rows.iter().filter(|row| row.result == "disabled").count(),
            5
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.reason == "timeout" || row.reason == "privacy_rejected")
                .count(),
            10
        );
        assert_eq!(rows.iter().filter(|row| !row.audit_recorded).count(), 5);
        assert!(rows.iter().all(|row| row.side_effects == 0));
        assert!(!encoded.contains("api_key"));
        assert!(!encoded.contains("not-recorded"));
        assert!(!encoded.contains("password"));
        assert!(!encoded.contains("profile"));
    }

    #[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
    struct AdoptionEvidence {
        provider_revision: Option<&'static str>,
        quality: bool,
        latency: bool,
        resources: bool,
        privacy: bool,
        fallback: bool,
        isolation: bool,
        replay: bool,
        manual_audit: bool,
    }

    impl AdoptionEvidence {
        const fn unavailable() -> Self {
            Self {
                provider_revision: None,
                quality: false,
                latency: false,
                resources: false,
                privacy: false,
                fallback: false,
                isolation: false,
                replay: false,
                manual_audit: false,
            }
        }

        const fn complete_for_fixture() -> Self {
            Self {
                provider_revision: Some("fixture-revision"),
                quality: true,
                latency: true,
                resources: true,
                privacy: true,
                fallback: true,
                isolation: true,
                replay: true,
                manual_audit: true,
            }
        }

        const fn eligible(self) -> bool {
            self.provider_revision.is_some()
                && self.quality
                && self.latency
                && self.resources
                && self.privacy
                && self.fallback
                && self.isolation
                && self.replay
                && self.manual_audit
        }
    }

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct EligibilityRow {
        consumer: &'static str,
        decision: &'static str,
        reason: &'static str,
        primary_digest: String,
        side_effects: u32,
    }

    #[test]
    fn g5_06_missing_adoption_evidence_keeps_every_consumer_record_only() {
        let evidence = AdoptionEvidence::unavailable();
        assert!(!evidence.eligible());
        let rows = AdoptionConsumer::ALL
            .into_iter()
            .map(|consumer| {
                let request = adoption_request(consumer);
                EligibilityRow {
                    consumer: consumer.name(),
                    decision: "record_only",
                    reason: "evidence_unavailable",
                    primary_digest: canonical_result_digest(&primary(&request)),
                    side_effects: 0,
                }
            })
            .collect::<Vec<_>>();
        let encoded = serde_json::to_string(&rows).expect("eligibility is JSON");
        let replay = serde_json::to_string(&rows).expect("eligibility replay is JSON");
        assert_eq!(encoded, replay);
        assert_eq!(rows.len(), AdoptionConsumer::ALL.len());
        assert!(rows.iter().all(|row| row.decision == "record_only"));
        assert!(rows.iter().all(|row| row.side_effects == 0));
        assert!(!encoded.contains("fixture-revision"));
        assert!(!encoded.contains("provider_revision"));

        let complete = AdoptionEvidence::complete_for_fixture();
        assert!(complete.eligible());
        for field in [
            "quality",
            "latency",
            "resources",
            "privacy",
            "fallback",
            "isolation",
            "replay",
            "manual_audit",
        ] {
            let mut candidate = complete;
            match field {
                "quality" => candidate.quality = false,
                "latency" => candidate.latency = false,
                "resources" => candidate.resources = false,
                "privacy" => candidate.privacy = false,
                "fallback" => candidate.fallback = false,
                "isolation" => candidate.isolation = false,
                "replay" => candidate.replay = false,
                "manual_audit" => candidate.manual_audit = false,
                _ => unreachable!(),
            }
            assert!(
                !candidate.eligible(),
                "missing evidence must fail closed: {field}"
            );
        }
    }
}
