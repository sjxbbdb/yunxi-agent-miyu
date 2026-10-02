//! Deterministic admission for automatically organized short diaries.
//!
//! This module deliberately has no model/provider dependency.  It classifies the
//! source record before an organizer result can be written, and exposes only a
//! metadata value for a future read-only decision consumer.  Neither decisions
//! nor metadata retain the diary text.

use super::{content_digest, ShortDiaryRecord};
use serde::{Deserialize, Serialize};

pub(crate) const ADMISSION_SCHEMA_VERSION: u16 = 1;

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
