use super::shared::*;
use crate::state::*;

fn claim(scope: &str, key: &str, certainty: ProfileClaimCertainty) -> NewProfileClaim {
    NewProfileClaim {
        owner_scope: scope.to_string(),
        key: key.to_string(),
        value: "tea".to_string(),
        certainty,
        source_kind: "user_edit".to_string(),
        source_ref: "profile-form:1".to_string(),
        observed_at: "2026-10-02T00:00:00Z".to_string(),
        updated_at: "2026-10-02T00:00:00Z".to_string(),
        revision: 1,
    }
}

#[test]
fn profile_claims_are_scope_isolated_and_revoke_is_hidden_by_default() {
    let (_temp, store) = test_store();
    let global = store
        .insert_profile_claim(&claim("", "drink", ProfileClaimCertainty::Confirmed))
        .unwrap();
    let persona = store
        .insert_profile_claim(&claim(
            "persona-a",
            "drink",
            ProfileClaimCertainty::Inferred,
        ))
        .unwrap();
    assert_eq!(
        store.list_profile_claims("", false).unwrap(),
        vec![global.clone()]
    );
    assert_eq!(
        store.list_profile_claims("persona-a", false).unwrap(),
        vec![persona.clone()]
    );
    assert!(store
        .list_profile_claims("persona-b", false)
        .unwrap()
        .is_empty());
    assert!(store
        .revoke_profile_claim(&global.claim_id, "2026-10-02T01:00:00Z")
        .unwrap());
    assert!(store.list_profile_claims("", false).unwrap().is_empty());
    assert_eq!(store.list_profile_claims("", true).unwrap().len(), 1);
}

#[test]
fn relationship_events_require_persona_and_bound_source_payload() {
    let (_temp, store) = test_store();
    let mut input = NewRelationshipEvent {
        persona_scope: "persona-a".into(),
        event_kind: "preference_shared".into(),
        summary: "likes tea".into(),
        payload: Some("{\"kind\":\"drink\"}".into()),
        observed_at: "2026-10-02T00:00:00Z".into(),
        source_kind: "conversation".into(),
        source_ref: "turn:abc".into(),
    };
    let event = store.insert_relationship_event(&input).unwrap();
    assert_eq!(
        store.list_relationship_events("persona-a", false).unwrap(),
        vec![event.clone()]
    );
    assert!(store.revoke_relationship_event(&event.event_id).unwrap());
    assert!(store
        .list_relationship_events("persona-a", false)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .list_relationship_events("persona-a", true)
            .unwrap()
            .len(),
        1
    );

    input.persona_scope.clear();
    assert!(store.insert_relationship_event(&input).is_err());
    input.persona_scope = "persona-a".into();
    input.payload = Some("x".repeat(MAX_RELATIONSHIP_PAYLOAD_CHARS + 1));
    assert!(store.insert_relationship_event(&input).is_err());
    input.payload = Some("small".into());
    input.source_ref = "file:///absolute/path".into();
    assert!(store.insert_relationship_event(&input).is_err());
}

#[test]
fn profile_claims_do_not_create_memory_rows_and_duplicate_revision_is_atomic() {
    let (_temp, store) = test_store();
    let first = store
        .insert_profile_claim(&claim("", "drink", ProfileClaimCertainty::Confirmed))
        .unwrap();
    let duplicate = claim("", "drink", ProfileClaimCertainty::Confirmed);
    assert!(store.insert_profile_claim(&duplicate).is_err());
    assert_eq!(store.list_profile_claims("", true).unwrap().len(), 1);
    let conn = rusqlite::Connection::open(store.state_dir().join("conversation.db")).unwrap();
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%embedding%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(
        tables.is_empty(),
        "profile claims must not add embedding tables"
    );
    assert_eq!(
        store
            .conv_db()
            .profile_claim_by_id(&first.claim_id)
            .unwrap(),
        Some(first)
    );
}

#[test]
fn profile_claim_owner_scope_rejects_whitespace_but_allows_global_empty_scope() {
    let (_temp, store) = test_store();
    let mut input = claim("   \t", "drink", ProfileClaimCertainty::Confirmed);
    assert!(store.insert_profile_claim(&input).is_err());
    assert!(store.list_profile_claims("   \t", false).is_err());

    input.owner_scope = "persona-a".into();
    let mut inserted = store.insert_profile_claim(&input).unwrap();
    inserted.owner_scope = " \n".into();
    assert!(store.upsert_profile_claim(&inserted).is_err());

    let global = store
        .insert_profile_claim(&claim("", "global-key", ProfileClaimCertainty::Confirmed))
        .unwrap();
    assert_eq!(global.owner_scope, "");
    assert_eq!(store.list_profile_claims("", false).unwrap(), vec![global]);
}

#[test]
fn confirmed_claims_require_explicit_user_or_manual_provenance() {
    let (_temp, store) = test_store();
    for source_kind in ["conversation", "model", "inference"] {
        let mut input = claim("persona-a", source_kind, ProfileClaimCertainty::Confirmed);
        input.source_kind = source_kind.to_string();
        assert!(store.insert_profile_claim(&input).is_err());
    }

    for source_kind in ["user_edit", "user_confirmation", "manual_import"] {
        let mut input = claim("persona-a", source_kind, ProfileClaimCertainty::Confirmed);
        input.source_kind = source_kind.to_string();
        assert!(store.insert_profile_claim(&input).is_ok());
    }

    let mut inferred = claim("persona-a", "conversation", ProfileClaimCertainty::Inferred);
    inferred.key = "inferred".into();
    inferred.source_kind = "conversation".into();
    let inserted = store.insert_profile_claim(&inferred).unwrap();
    let mut forged = inserted.clone();
    forged.certainty = ProfileClaimCertainty::Confirmed;
    assert!(store.upsert_profile_claim(&forged).is_err());
    assert_eq!(
        store
            .profile_claim_by_id_in_scope("persona-a", &inserted.claim_id)
            .unwrap()
            .unwrap()
            .certainty,
        ProfileClaimCertainty::Inferred
    );
}

#[test]
fn scope_aware_reads_and_revokes_never_cross_persona_boundaries() {
    let (temp, store) = test_store();
    let global = store
        .insert_profile_claim(&claim("", "global", ProfileClaimCertainty::Confirmed))
        .unwrap();
    let claim_a = store
        .insert_profile_claim(&claim("persona-a", "a", ProfileClaimCertainty::Inferred))
        .unwrap();
    let claim_b = store
        .insert_profile_claim(&claim("persona-b", "b", ProfileClaimCertainty::Inferred))
        .unwrap();
    let event_a = store
        .insert_relationship_event(&NewRelationshipEvent {
            persona_scope: "persona-a".into(),
            event_kind: "stage".into(),
            summary: "a".into(),
            payload: None,
            observed_at: "2026-10-02T00:00:00Z".into(),
            source_kind: "conversation".into(),
            source_ref: "turn:a".into(),
        })
        .unwrap();
    let event_b = store
        .insert_relationship_event(&NewRelationshipEvent {
            persona_scope: "persona-b".into(),
            event_kind: "stage".into(),
            summary: "b".into(),
            payload: None,
            observed_at: "2026-10-02T00:00:01Z".into(),
            source_kind: "conversation".into(),
            source_ref: "turn:b".into(),
        })
        .unwrap();

    assert!(store
        .profile_claim_by_id_in_scope("persona-b", &claim_a.claim_id)
        .unwrap()
        .is_none());
    assert!(store
        .relationship_event_by_id_in_scope("persona-b", &event_a.event_id)
        .unwrap()
        .is_none());
    assert!(store
        .profile_claim_by_id_in_scope("persona-a", &global.claim_id)
        .unwrap()
        .is_none());
    assert_eq!(
        store
            .profile_claim_by_id_in_scope("", &global.claim_id)
            .unwrap(),
        Some(global.clone())
    );

    assert!(!store
        .revoke_profile_claim_in_scope("persona-b", &claim_a.claim_id, "2026-10-02T01:00:00Z")
        .unwrap());
    assert!(!store
        .revoke_relationship_event_in_scope("persona-b", &event_a.event_id)
        .unwrap());
    assert_eq!(
        store.list_profile_claims("persona-a", false).unwrap(),
        vec![claim_a.clone()]
    );
    assert_eq!(
        store.list_relationship_events("persona-a", false).unwrap(),
        vec![event_a.clone()]
    );

    assert!(store
        .revoke_profile_claim_in_scope("persona-a", &claim_a.claim_id, "2026-10-02T01:00:00Z")
        .unwrap());
    assert!(store
        .revoke_relationship_event_in_scope("persona-a", &event_a.event_id)
        .unwrap());
    assert!(store
        .list_profile_claims("persona-a", false)
        .unwrap()
        .is_empty());
    assert!(store
        .list_relationship_events("persona-a", false)
        .unwrap()
        .is_empty());
    assert_eq!(
        store.list_profile_claims("persona-a", true).unwrap().len(),
        1
    );
    assert_eq!(
        store
            .list_relationship_events("persona-a", true)
            .unwrap()
            .len(),
        1
    );

    let reopened = StateStore::new(&test_paths(temp.path())).unwrap();
    assert_eq!(
        reopened
            .profile_claim_by_id_in_scope("persona-a", &claim_a.claim_id)
            .unwrap()
            .unwrap()
            .status,
        ProfileClaimStatus::Revoked
    );
    assert_eq!(
        reopened
            .relationship_event_by_id_in_scope("persona-a", &event_a.event_id)
            .unwrap()
            .unwrap()
            .status,
        RelationshipEventStatus::Revoked
    );
    assert_eq!(
        reopened
            .profile_claim_by_id_in_scope("persona-b", &claim_b.claim_id)
            .unwrap(),
        Some(claim_b)
    );
    assert_eq!(
        reopened
            .relationship_event_by_id_in_scope("persona-b", &event_b.event_id)
            .unwrap(),
        Some(event_b)
    );
}
