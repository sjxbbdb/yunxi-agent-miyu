use super::shared::*;
use crate::decision::{validate_result, DecisionError, DecisionOutcome};
use crate::memory::*;
use serde_json::json;
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
