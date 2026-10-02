//! 记忆生命周期的窄审计接缝。
//!
//! 这个模块不拥有 facts/episodes 的当前状态，也不把候选内容保存下来。它只描述
//! 允许的状态边界，并为持久化的 append-only 审计事件提供稳定的值对象。候选内容
//! 永远只以摘要和来源 id 进入事件，原文仍由既有记忆表按原有访问规则管理。

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum MemoryLifecycleState {
    #[serde(rename = "transient")]
    Transient,
    #[serde(rename = "short")]
    Short,
    #[serde(rename = "candidate")]
    Candidate,
    #[serde(rename = "committed")]
    Committed,
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "expired")]
    Expired,
}

impl MemoryLifecycleState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Short => "short",
            Self::Candidate => "candidate",
            Self::Committed => "committed",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "transient" => Some(Self::Transient),
            "short" => Some(Self::Short),
            "candidate" => Some(Self::Candidate),
            "committed" => Some(Self::Committed),
            "rejected" => Some(Self::Rejected),
            "expired" => Some(Self::Expired),
            _ => None,
        }
    }
}

impl fmt::Display for MemoryLifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub enum MemoryLifecycleOwner {
    #[serde(rename = "turn_loop")]
    TurnLoop,
    #[serde(rename = "memory_organizer")]
    MemoryOrganizer,
    #[serde(rename = "admission_rules")]
    AdmissionRules,
    #[serde(rename = "decision_port")]
    DecisionPort,
    #[serde(rename = "memory_gc")]
    MemoryGc,
    #[serde(rename = "user")]
    User,
    #[serde(rename = "migration_repair")]
    MigrationRepair,
}

impl MemoryLifecycleOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TurnLoop => "turn_loop",
            Self::MemoryOrganizer => "memory_organizer",
            Self::AdmissionRules => "admission_rules",
            Self::DecisionPort => "decision_port",
            Self::MemoryGc => "memory_gc",
            Self::User => "user",
            Self::MigrationRepair => "migration_repair",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "turn_loop" => Some(Self::TurnLoop),
            "memory_organizer" => Some(Self::MemoryOrganizer),
            "admission_rules" => Some(Self::AdmissionRules),
            "decision_port" => Some(Self::DecisionPort),
            "memory_gc" => Some(Self::MemoryGc),
            "user" => Some(Self::User),
            "migration_repair" => Some(Self::MigrationRepair),
            _ => None,
        }
    }
}

impl fmt::Display for MemoryLifecycleOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validate the only transitions currently supported by the memory pipeline.
///
/// There is intentionally no implicit self-transition and no path that skips
/// `candidate` for an automatically written short diary. Manual facts are the
/// one explicit `transient -> committed` path.
pub fn validate_transition(from: MemoryLifecycleState, to: MemoryLifecycleState) -> Result<()> {
    let valid = matches!(
        (from, to),
        (MemoryLifecycleState::Transient, MemoryLifecycleState::Short)
            | (
                MemoryLifecycleState::Transient,
                MemoryLifecycleState::Committed
            )
            | (MemoryLifecycleState::Short, MemoryLifecycleState::Candidate)
            | (MemoryLifecycleState::Short, MemoryLifecycleState::Expired)
            | (
                MemoryLifecycleState::Candidate,
                MemoryLifecycleState::Committed
            )
            | (
                MemoryLifecycleState::Candidate,
                MemoryLifecycleState::Rejected
            )
    );
    if !valid {
        bail!("invalid memory lifecycle transition: {from} -> {to}");
    }
    Ok(())
}

/// Validate that the actor named on an event is allowed to make the edge.
///
/// `DecisionPort` is deliberately not a terminal writer: future decision
/// models may advise admission, but the durable transition remains owned by
/// the organizer/rules boundary.
pub fn validate_owned_transition(
    from: MemoryLifecycleState,
    to: MemoryLifecycleState,
    owner: MemoryLifecycleOwner,
) -> Result<()> {
    validate_transition(from, to)?;
    let valid_owner = matches!(
        (from, to, owner),
        (
            MemoryLifecycleState::Transient,
            MemoryLifecycleState::Short,
            MemoryLifecycleOwner::TurnLoop
        ) | (
            MemoryLifecycleState::Transient,
            MemoryLifecycleState::Committed,
            MemoryLifecycleOwner::User,
        ) | (
            MemoryLifecycleState::Short,
            MemoryLifecycleState::Candidate,
            MemoryLifecycleOwner::MemoryOrganizer,
        ) | (
            MemoryLifecycleState::Short,
            MemoryLifecycleState::Expired,
            MemoryLifecycleOwner::MemoryGc,
        ) | (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Committed,
            MemoryLifecycleOwner::MemoryOrganizer,
        ) | (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Committed,
            MemoryLifecycleOwner::AdmissionRules,
        ) | (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Rejected,
            MemoryLifecycleOwner::MemoryOrganizer,
        ) | (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Rejected,
            MemoryLifecycleOwner::AdmissionRules,
        ) | (
            MemoryLifecycleState::Candidate,
            MemoryLifecycleState::Rejected,
            MemoryLifecycleOwner::MemoryGc,
        )
    );
    if !valid_owner {
        bail!("memory lifecycle owner is not allowed to make {from} -> {to}: {owner}");
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MemoryLifecycleEvent {
    pub memory_kind: String,
    pub memory_id: Option<i64>,
    pub from_state: MemoryLifecycleState,
    pub to_state: MemoryLifecycleState,
    pub owner: MemoryLifecycleOwner,
    pub owner_scope: String,
    pub reason_code: String,
    pub source_episode_ids: Vec<i64>,
    pub content_digest: String,
    pub generation: i64,
    pub transition_key: String,
    pub created_at: String,
}

impl MemoryLifecycleEvent {
    pub fn validate(&self) -> Result<()> {
        validate_owned_transition(self.from_state, self.to_state, self.owner)?;
        let kind = self.memory_kind.trim();
        if kind.is_empty() || kind.len() > 64 || kind.chars().any(char::is_control) {
            bail!("memory lifecycle kind is invalid");
        }
        if self.memory_id.is_some_and(|id| id <= 0) {
            bail!("memory lifecycle memory id must be positive");
        }
        let scope = self.owner_scope.trim();
        if scope.is_empty() || scope.len() > 256 || scope.chars().any(char::is_control) {
            bail!("memory lifecycle owner scope is invalid");
        }
        let reason = self.reason_code.trim();
        if reason.is_empty() || reason.len() > 96 || reason.chars().any(char::is_control) {
            bail!("memory lifecycle reason code is invalid");
        }
        if self.source_episode_ids.iter().any(|id| *id <= 0) {
            bail!("memory lifecycle source episode ids must be positive");
        }
        if self
            .source_episode_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            bail!("memory lifecycle source episode ids must be sorted and unique");
        }
        if self.content_digest.len() > 128
            || self.transition_key.trim().is_empty()
            || self.transition_key.len() > 512
            || self.transition_key.chars().any(char::is_control)
        {
            bail!("memory lifecycle digest or transition key is invalid");
        }
        if self.generation < 0 {
            bail!("memory lifecycle generation must not be negative");
        }
        if self.created_at.trim().is_empty() || self.created_at.chars().any(char::is_control) {
            bail!("memory lifecycle created_at is invalid");
        }
        Ok(())
    }
}

pub fn normalized_episode_ids(ids: &[i64]) -> Result<Vec<i64>> {
    let mut normalized = ids.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    if normalized.iter().any(|id| *id <= 0) {
        bail!("memory lifecycle source episode ids must be positive");
    }
    Ok(normalized)
}

pub fn content_digest(content: &str) -> String {
    blake3::hash(content.as_bytes()).to_hex().to_string()
}

pub fn transition_key(
    generation: i64,
    memory_kind: &str,
    memory_id: Option<i64>,
    from_state: MemoryLifecycleState,
    to_state: MemoryLifecycleState,
    source_episode_ids: &[i64],
) -> String {
    let ids = source_episode_ids
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "g{generation}:{memory_kind}:{}:{}>{}:{}",
        memory_id.map_or_else(|| "none".to_string(), |id| id.to_string()),
        from_state,
        to_state,
        ids
    )
}
