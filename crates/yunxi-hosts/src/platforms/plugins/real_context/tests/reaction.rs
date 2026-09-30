//! 判断通过后贴在对方消息上的「在看了」表情。
//!
//! 09-24 用户拍板:抽样与刚说过话这两路是她自己凑过去插话,没人在等她,不贴。
//! 按主触发归类——同时被 @、在续聊窗口里的照贴;补救消息顶替时沿用原始触发。

use super::shared::*;
use crate::platforms::plugins::real_context::*;

type Recorded = Arc<Mutex<Vec<(String, String, bool)>>>;

fn reaction_context() -> (tempfile::TempDir, PlatformTurnContext, Recorded) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let (temp, context) = test_context(Arc::new(ReactionAdapter {
        reactions: recorded.clone(),
    }));
    (temp, context, recorded)
}

fn marked(recorded: &Recorded, message_id: &str) -> bool {
    recorded
        .lock()
        .unwrap()
        .iter()
        .any(|(id, _, active)| id == message_id && *active)
}

/// 已承诺的回复被同一发送者的补救消息顶替:这是判官放行后唯一不需要判官模型
/// 就能走到贴表情那一步的路,测试环境里用它代表「判断通过」。
async fn take_over(trigger: TriggerKind) -> (bool, bool) {
    let (_temp, context, recorded) = reaction_context();
    let plugin = RealContextPlugin::new();
    let first = inbound_event();
    plugin.register_committed_pending(
        &context,
        trigger,
        Vec::new(),
        vec![active_reply_target(&first)],
        false,
    );
    let mut correction = inbound_event();
    correction.message_id = "message-2".to_string();
    correction.text = "接着刚才那句再补一条".to_string();
    let mut decision = TriggerDecision {
        should_reply: false,
        content: correction.text.clone(),
        response_target: None,
    };
    plugin
        .decide_group_trigger(
            &context,
            &correction,
            &mut decision,
            &RealContextPluginSettings::default(),
        )
        .await
        .unwrap();
    (decision.should_reply, marked(&recorded, "message-2"))
}

#[tokio::test]
async fn sampled_and_after_speaking_replies_leave_the_message_unmarked() {
    for trigger in [TriggerKind::Probability, TriggerKind::AfterSpeaking] {
        let (replied, reacted) = take_over(trigger).await;
        assert!(replied, "{trigger:?}: 承诺沿用,补救消息应直接回复");
        assert!(!reacted, "{trigger:?}: 自己凑过去插话不该先贴表情");
    }
}

#[tokio::test]
async fn called_or_continuing_replies_still_mark_the_message() {
    for trigger in [TriggerKind::Direct, TriggerKind::Continuation] {
        let (replied, reacted) = take_over(trigger).await;
        assert!(replied, "{trigger:?}: 承诺沿用,补救消息应直接回复");
        assert!(reacted, "{trigger:?}: 被叫到或正聊着的照贴表情");
    }
}

/// 回合已在跑时的补救(`confirm_supersede`)同样沿用原始触发。
#[tokio::test]
async fn a_running_sampled_turn_does_not_mark_the_correction() {
    for (trigger, wants) in [
        (TriggerKind::Probability, false),
        (TriggerKind::AfterSpeaking, false),
        (TriggerKind::Direct, true),
    ] {
        let (_temp, context, recorded) = reaction_context();
        let plugin = RealContextPlugin::new();
        let first = inbound_event();
        plugin.register_committed_pending(
            &context,
            trigger,
            Vec::new(),
            vec![active_reply_target(&first)],
            false,
        );
        let mut correction = inbound_event();
        correction.message_id = "message-2".to_string();
        plugin.confirm_supersede(&context, &correction).await;
        assert_eq!(
            marked(&recorded, "message-2"),
            wants,
            "{trigger:?} 的补救消息贴表情与否不符预期"
        );
    }
}

#[test]
fn only_self_initiated_triggers_skip_the_reaction() {
    for (trigger, wants) in [
        (TriggerKind::Probability, false),
        (TriggerKind::AfterSpeaking, false),
        (TriggerKind::Direct, true),
        (TriggerKind::Continuation, true),
        (TriggerKind::Supersede, true),
        (TriggerKind::Moderation, true),
    ] {
        assert_eq!(trigger.marks_with_reaction(), wants, "{trigger:?}");
    }
}

/// 判过要回的新消息并进了同一个人正在跑的那一轮(09-26):表情从那一轮原先的
/// 消息换贴到新消息上,新消息的待回复改记在那一轮名下——新消息自己的上下文随
/// 并入就散了,回复发出时只有那一轮能把它清掉。回复没引用新消息(引用开关关着,
/// 落回那一轮自己那条)时,新消息上的表情也得摘。
#[tokio::test]
async fn a_merged_followup_moves_the_reaction_and_hands_its_pending_to_the_running_turn() {
    let (_temp, running, recorded) = reaction_context();
    let plugin = RealContextPlugin::new();
    let mut followup = inbound_event();
    followup.message_id = "message-2".to_string();
    let newcomer = PlatformTurnContext::new(
        running.conversation.clone(),
        running.sender_id.clone(),
        running.sender_display_name.clone(),
        false,
        running.config.clone(),
        running.paths.clone(),
        running.state_store.clone(),
        running.adapter.clone(),
        running.plugins.clone(),
    )
    .with_inbound_event(followup.clone());
    plugin.register_committed_pending(
        &newcomer,
        TriggerKind::Direct,
        vec![("message-2".to_string(), "289".to_string())],
        vec![active_reply_target(&followup)],
        false,
    );

    PlatformPlugin::adopt_followup(&plugin, &running, &followup).await;

    assert!(
        recorded
            .lock()
            .unwrap()
            .contains(&("message-1".to_string(), "289".to_string(), false)),
        "那一轮原先的消息应摘掉表情"
    );
    assert!(
        !recorded
            .lock()
            .unwrap()
            .contains(&("message-2".to_string(), "289".to_string(), false)),
        "新消息的表情留着,等回复发出"
    );
    {
        let runtime = plugin.runtime.lock().unwrap();
        let pending = runtime
            .sessions
            .get(&runtime_session_key(&running))
            .and_then(|session| session.pending.get(&followup.sender_id))
            .expect("待回复应还在");
        assert!(
            pending.owner.same_turn(&running.ownership),
            "待回复应改记在那一轮名下"
        );
    }

    let reply = OutboundMessage::text(OutboundOrigin::FinalReply, "好");
    plugin
        .finish_reply(&running, &reply, &RealContextPluginSettings::default())
        .await;
    assert!(
        recorded
            .lock()
            .unwrap()
            .contains(&("message-2".to_string(), "289".to_string(), false)),
        "回复发出后新消息上的表情应摘掉"
    );
    assert!(
        plugin
            .runtime
            .lock()
            .unwrap()
            .sessions
            .get(&runtime_session_key(&running))
            .and_then(|session| session.pending.get(&followup.sender_id))
            .is_none(),
        "回复发出后待回复应由那一轮清掉"
    );
}
