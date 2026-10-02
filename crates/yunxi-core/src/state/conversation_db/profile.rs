//! Structured profile claims and relationship events.
//!
//! This module is deliberately metadata-only. It does not read or write the
//! legacy `profile.md`, prompt assembly, or any memory/embedding table.

use super::ConversationDb;
use anyhow::{bail, Result};
use rand::random;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub const MAX_PROFILE_KEY_CHARS: usize = 128;
pub const MAX_PROFILE_VALUE_CHARS: usize = 4_096;
pub const MAX_RELATIONSHIP_SUMMARY_CHARS: usize = 4_096;
pub const MAX_RELATIONSHIP_PAYLOAD_CHARS: usize = 8_192;
pub const MAX_SOURCE_KIND_CHARS: usize = 64;
pub const MAX_SOURCE_REF_CHARS: usize = 256;
pub const MAX_TIMESTAMP_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileClaimCertainty {
    Confirmed,
    Inferred,
}

impl ProfileClaimCertainty {
    fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Inferred => "inferred",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "confirmed" => Ok(Self::Confirmed),
            "inferred" => Ok(Self::Inferred),
            _ => Err(invalid_enum("certainty", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileClaimStatus {
    Active,
    Revoked,
}

impl ProfileClaimStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(invalid_enum("status", value)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipEventStatus {
    Active,
    Revoked,
}

impl RelationshipEventStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Revoked => "revoked",
        }
    }

    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "active" => Ok(Self::Active),
            "revoked" => Ok(Self::Revoked),
            _ => Err(invalid_enum("status", value)),
        }
    }
}

/// A structured claim about the owner profile. An empty `owner_scope` is the
/// global profile; persona scopes must be explicit non-empty strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileClaim {
    pub claim_id: String,
    pub owner_scope: String,
    pub key: String,
    pub value: String,
    pub certainty: ProfileClaimCertainty,
    pub source_kind: String,
    pub source_ref: String,
    pub observed_at: String,
    pub updated_at: String,
    pub status: ProfileClaimStatus,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProfileClaim {
    pub owner_scope: String,
    pub key: String,
    pub value: String,
    pub certainty: ProfileClaimCertainty,
    pub source_kind: String,
    pub source_ref: String,
    pub observed_at: String,
    pub updated_at: String,
    pub revision: i64,
}

/// An append-only, non-prompt relationship fact for one explicit persona.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipEvent {
    pub event_id: String,
    pub persona_scope: String,
    pub event_kind: String,
    pub summary: String,
    pub payload: Option<String>,
    pub observed_at: String,
    pub source_kind: String,
    pub source_ref: String,
    pub status: RelationshipEventStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRelationshipEvent {
    pub persona_scope: String,
    pub event_kind: String,
    pub summary: String,
    pub payload: Option<String>,
    pub observed_at: String,
    pub source_kind: String,
    pub source_ref: String,
}

const PROFILE_CLAIM_COLUMNS: &str =
    "claim_id, owner_scope, key, value, certainty, source_kind, source_ref, observed_at, updated_at, status, revision";
const RELATIONSHIP_EVENT_COLUMNS: &str =
    "event_id, persona_scope, event_kind, summary, payload, observed_at, source_kind, source_ref, status";

fn invalid_enum(field: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid {field} value: {value}"),
        )),
    )
}

fn validate_text(field: &str, value: &str, max_chars: usize, required: bool) -> Result<()> {
    if required && value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    if value.chars().count() > max_chars {
        bail!("{field} exceeds the {max_chars}-character limit");
    }
    if value.contains('\0') {
        bail!("{field} must not contain NUL bytes");
    }
    Ok(())
}

fn validate_source(kind: &str, reference: &str) -> Result<()> {
    validate_text("source_kind", kind, MAX_SOURCE_KIND_CHARS, true)?;
    validate_text("source_ref", reference, MAX_SOURCE_REF_CHARS, true)?;
    let lower = reference.to_ascii_lowercase();
    let absolute_path = reference.starts_with('/')
        || reference.starts_with('\\')
        || (reference.len() >= 3
            && reference.as_bytes()[0].is_ascii_alphabetic()
            && reference.as_bytes()[1] == b':'
            && matches!(reference.as_bytes()[2], b'/' | b'\\'))
        || lower.starts_with("file://");
    if absolute_path {
        bail!("source_ref must not contain an absolute local path");
    }
    for marker in [
        "api_key=",
        "apikey=",
        "access_token=",
        "secret=",
        "token=",
        "authorization: bearer ",
    ] {
        if lower.contains(marker) {
            bail!("source_ref must not contain secrets");
        }
    }
    if lower.starts_with("sk-") || lower.starts_with("ghp_") || lower.starts_with("xoxb-") {
        bail!("source_ref must not contain secrets");
    }
    Ok(())
}

fn validate_timestamp(field: &str, value: &str) -> Result<()> {
    validate_text(field, value, MAX_TIMESTAMP_CHARS, true)
}

fn validate_owner_scope(value: &str) -> Result<()> {
    validate_text("owner_scope", value, 128, false)?;
    if !value.is_empty() && value.trim().is_empty() {
        bail!("owner_scope must be empty or contain non-whitespace characters");
    }
    Ok(())
}

fn validate_profile_claim(claim: &ProfileClaim) -> Result<()> {
    validate_text("claim_id", &claim.claim_id, 128, true)?;
    validate_owner_scope(&claim.owner_scope)?;
    validate_text("key", &claim.key, MAX_PROFILE_KEY_CHARS, true)?;
    validate_text("value", &claim.value, MAX_PROFILE_VALUE_CHARS, true)?;
    validate_source(&claim.source_kind, &claim.source_ref)?;
    validate_timestamp("observed_at", &claim.observed_at)?;
    validate_timestamp("updated_at", &claim.updated_at)?;
    if claim.revision < 1 {
        bail!("revision must be positive");
    }
    Ok(())
}

fn validate_new_profile_claim(claim: &NewProfileClaim) -> Result<()> {
    validate_owner_scope(&claim.owner_scope)?;
    validate_text("key", &claim.key, MAX_PROFILE_KEY_CHARS, true)?;
    validate_text("value", &claim.value, MAX_PROFILE_VALUE_CHARS, true)?;
    validate_source(&claim.source_kind, &claim.source_ref)?;
    validate_timestamp("observed_at", &claim.observed_at)?;
    validate_timestamp("updated_at", &claim.updated_at)?;
    if claim.revision < 1 {
        bail!("revision must be positive");
    }
    Ok(())
}

fn validate_relationship_event(event: &NewRelationshipEvent) -> Result<()> {
    validate_text("persona_scope", &event.persona_scope, 128, true)?;
    validate_text("event_kind", &event.event_kind, 128, true)?;
    validate_text(
        "summary",
        &event.summary,
        MAX_RELATIONSHIP_SUMMARY_CHARS,
        true,
    )?;
    if let Some(payload) = &event.payload {
        validate_text("payload", payload, MAX_RELATIONSHIP_PAYLOAD_CHARS, true)?;
    }
    validate_timestamp("observed_at", &event.observed_at)?;
    validate_source(&event.source_kind, &event.source_ref)?;
    Ok(())
}

fn map_profile_claim(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProfileClaim> {
    Ok(ProfileClaim {
        claim_id: row.get(0)?,
        owner_scope: row.get(1)?,
        key: row.get(2)?,
        value: row.get(3)?,
        certainty: ProfileClaimCertainty::parse(&row.get::<_, String>(4)?)?,
        source_kind: row.get(5)?,
        source_ref: row.get(6)?,
        observed_at: row.get(7)?,
        updated_at: row.get(8)?,
        status: ProfileClaimStatus::parse(&row.get::<_, String>(9)?)?,
        revision: row.get(10)?,
    })
}

fn map_relationship_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelationshipEvent> {
    Ok(RelationshipEvent {
        event_id: row.get(0)?,
        persona_scope: row.get(1)?,
        event_kind: row.get(2)?,
        summary: row.get(3)?,
        payload: row.get(4)?,
        observed_at: row.get(5)?,
        source_kind: row.get(6)?,
        source_ref: row.get(7)?,
        status: RelationshipEventStatus::parse(&row.get::<_, String>(8)?)?,
    })
}

impl ConversationDb {
    pub fn insert_profile_claim(&self, input: &NewProfileClaim) -> Result<ProfileClaim> {
        validate_new_profile_claim(input)?;
        let claim_id = format!("profile_claim_{:032x}", random::<u128>());
        let claim = ProfileClaim {
            claim_id,
            owner_scope: input.owner_scope.clone(),
            key: input.key.clone(),
            value: input.value.clone(),
            certainty: input.certainty,
            source_kind: input.source_kind.clone(),
            source_ref: input.source_ref.clone(),
            observed_at: input.observed_at.clone(),
            updated_at: input.updated_at.clone(),
            status: ProfileClaimStatus::Active,
            revision: input.revision,
        };
        self.upsert_profile_claim(&claim)?;
        Ok(claim)
    }

    pub fn upsert_profile_claim(&self, claim: &ProfileClaim) -> Result<()> {
        validate_profile_claim(claim)?;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO profile_claims
                 (claim_id, owner_scope, key, value, certainty, source_kind, source_ref,
                  observed_at, updated_at, status, revision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(claim_id) DO UPDATE SET
                 owner_scope = excluded.owner_scope, key = excluded.key,
                 value = excluded.value, certainty = excluded.certainty,
                 source_kind = excluded.source_kind, source_ref = excluded.source_ref,
                 observed_at = excluded.observed_at, updated_at = excluded.updated_at,
                 status = excluded.status, revision = excluded.revision",
            params![
                &claim.claim_id,
                &claim.owner_scope,
                &claim.key,
                &claim.value,
                claim.certainty.as_str(),
                &claim.source_kind,
                &claim.source_ref,
                &claim.observed_at,
                &claim.updated_at,
                claim.status.as_str(),
                claim.revision,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn profile_claim_by_id(&self, claim_id: &str) -> Result<Option<ProfileClaim>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!("SELECT {PROFILE_CLAIM_COLUMNS} FROM profile_claims WHERE claim_id = ?1"),
                params![claim_id],
                map_profile_claim,
            )
            .optional()?)
    }

    pub fn list_profile_claims(
        &self,
        owner_scope: &str,
        include_revoked: bool,
    ) -> Result<Vec<ProfileClaim>> {
        validate_owner_scope(owner_scope)?;
        let conn = self.conn.lock().unwrap();
        let sql = if include_revoked {
            format!(
                "SELECT {PROFILE_CLAIM_COLUMNS} FROM profile_claims
                 WHERE owner_scope = ?1 ORDER BY key ASC, revision ASC, claim_id ASC"
            )
        } else {
            format!(
                "SELECT {PROFILE_CLAIM_COLUMNS} FROM profile_claims
                 WHERE owner_scope = ?1 AND status = 'active'
                 ORDER BY key ASC, revision ASC, claim_id ASC"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![owner_scope], map_profile_claim)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn revoke_profile_claim(&self, claim_id: &str, updated_at: &str) -> Result<bool> {
        validate_text("claim_id", claim_id, 128, true)?;
        validate_timestamp("updated_at", updated_at)?;
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE profile_claims SET status = 'revoked', updated_at = ?2
             WHERE claim_id = ?1 AND status != 'revoked'",
            params![claim_id, updated_at],
        )? == 1)
    }

    pub fn insert_relationship_event(
        &self,
        input: &NewRelationshipEvent,
    ) -> Result<RelationshipEvent> {
        validate_relationship_event(input)?;
        let event = RelationshipEvent {
            event_id: format!("relationship_event_{:032x}", random::<u128>()),
            persona_scope: input.persona_scope.clone(),
            event_kind: input.event_kind.clone(),
            summary: input.summary.clone(),
            payload: input.payload.clone(),
            observed_at: input.observed_at.clone(),
            source_kind: input.source_kind.clone(),
            source_ref: input.source_ref.clone(),
            status: RelationshipEventStatus::Active,
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO relationship_events
                 (event_id, persona_scope, event_kind, summary, payload, observed_at,
                  source_kind, source_ref, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active')",
            params![
                &event.event_id,
                &event.persona_scope,
                &event.event_kind,
                &event.summary,
                event.payload.as_deref(),
                &event.observed_at,
                &event.source_kind,
                &event.source_ref,
            ],
        )?;
        Ok(event)
    }

    pub fn relationship_event_by_id(&self, event_id: &str) -> Result<Option<RelationshipEvent>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                &format!(
                    "SELECT {RELATIONSHIP_EVENT_COLUMNS} FROM relationship_events WHERE event_id = ?1"
                ),
                params![event_id],
                map_relationship_event,
            )
            .optional()?)
    }

    pub fn list_relationship_events(
        &self,
        persona_scope: &str,
        include_revoked: bool,
    ) -> Result<Vec<RelationshipEvent>> {
        validate_text("persona_scope", persona_scope, 128, true)?;
        let conn = self.conn.lock().unwrap();
        let sql = if include_revoked {
            format!(
                "SELECT {RELATIONSHIP_EVENT_COLUMNS} FROM relationship_events
                 WHERE persona_scope = ?1 ORDER BY observed_at ASC, event_id ASC"
            )
        } else {
            format!(
                "SELECT {RELATIONSHIP_EVENT_COLUMNS} FROM relationship_events
                 WHERE persona_scope = ?1 AND status = 'active'
                 ORDER BY observed_at ASC, event_id ASC"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![persona_scope], map_relationship_event)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn revoke_relationship_event(&self, event_id: &str) -> Result<bool> {
        validate_text("event_id", event_id, 128, true)?;
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE relationship_events SET status = 'revoked'
             WHERE event_id = ?1 AND status != 'revoked'",
            params![event_id],
        )? == 1)
    }
}
