//! 后台发的附件没传上去（见 `platforms::background_send`）：照后台任务的样子回报给她。
//!
//! 她还在这个对话里回话，就并进那一轮（发图的那一轮优先）；闲着就替这个对话起一轮。

use crate::platforms::onebot::*;

/// 装进每个 QQ 回合上下文的回报口。
pub(in crate::platforms::onebot) fn undelivered_hook(
    state: &DaemonState,
) -> crate::platforms::UndeliveredHook {
    let state = state.clone();
    Arc::new(move |context, notice| {
        let state = state.clone();
        tokio::spawn(async move { report_undelivered(&state, &context, notice).await });
    })
}

async fn report_undelivered(
    state: &DaemonState,
    context: &Arc<PlatformTurnContext>,
    notice: String,
) {
    if let Some((run_id, turn_id, session_id)) = running_turn_in_conversation(state, context) {
        let request = TurnUpdateRequest {
            run_id,
            turn_id,
            session_id: Some(session_id),
            audience: yunxi_base::config::PromptAudience::External,
            content: notice.clone(),
            display_content: notice.clone(),
            attachments: Vec::new(),
            uploaded_attachment_ids: Vec::new(),
            mode: TurnUpdateMode::Followup,
        };
        match enqueue_turn_update(state, request) {
            Ok(_) => return,
            // 那一轮正在收尾：替这个对话另起一轮。
            Err(error) => {
                tracing::debug!(error = %error, "upload failure notice could not join the running turn")
            }
        }
    }
    let conversation = &context.conversation;
    if let Err(error) = wake_conversation_for_undelivered(
        state,
        &conversation.account_id,
        conversation.kind.as_str(),
        &conversation.conversation_id,
        Some(&context.sender_id),
        notice,
    )
    .await
    {
        tracing::warn!(
            target: "yunxi::qq",
            error = %error,
            conversation_id = %conversation.conversation_id,
            "{}",
            t("the upload failure notice could not be delivered", "附件没发出去的回报没能交给她")
        );
    }
}

/// 这个对话里正在跑的一轮：发图的那一轮还在就是它，不然取最近开始的那一轮。
fn running_turn_in_conversation(
    state: &DaemonState,
    context: &Arc<PlatformTurnContext>,
) -> Option<(String, String, Arc<str>)> {
    let manager = state.manager.lock().unwrap();
    manager
        .active_runs
        .iter()
        .filter_map(|(run_id, run)| {
            let followup = run.platform_followup.as_ref()?;
            if followup.conversation != context.conversation {
                return None;
            }
            let own = Arc::ptr_eq(&followup.context, context);
            Some((
                own,
                followup.started(),
                run_id.clone(),
                run.turn_id.clone()?,
                run.session_id.clone(),
            ))
        })
        .max_by_key(|(own, started, ..)| (*own, *started))
        .map(|(_, _, run_id, turn_id, session_id)| (run_id, turn_id, session_id))
}
