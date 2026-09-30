//! 同一个人的新消息并进他正在跑的那一轮。
//!
//! 私聊:回合在跑时,新消息一律并进去,与终端、网页一致。
//!
//! 群聊:先照常过回复判断(覆盖窗口、判官都不变)。判了要回、而这个人还有一轮
//! 在跑,就并进那一轮,不另起一轮——否则前一轮刚把答案发出去,后一轮又把同一件
//! 事答一遍(09-26 用户:回得慢,人家催一句「??」,不该换来两条回复)。只认同一个
//! 人,别人的消息照常排队,所以 A、B、A 交错时,第二条 A 仍并进第一条 A 那一轮。

use crate::platforms::onebot::*;
use yunxi_engine::agent::QueueIngressReservation;

/// 这个人在这场对话里正在跑的那一轮。
pub(in crate::platforms::onebot) struct ActiveSenderTurn {
    pub(in crate::platforms::onebot) run_id: String,
    pub(in crate::platforms::onebot) turn_id: String,
    pub(in crate::platforms::onebot) followup: Arc<PlatformFollowupRun>,
    /// 工具正在跑时才拿得到。占住它,这条消息保证在下一步开始前被读到。
    pub(in crate::platforms::onebot) reservation: Option<QueueIngressReservation>,
}

impl ActiveSenderTurn {
    pub(in crate::platforms::onebot) fn update_mode(&self) -> TurnUpdateMode {
        active_turn_update_mode(self.reservation.is_some())
    }
}

pub(in crate::platforms::onebot) fn active_sender_turn(
    state: &DaemonState,
    session_id: &str,
    conversation: &PlatformConversation,
    sender_id: &str,
) -> Option<ActiveSenderTurn> {
    let (run_id, turn_id, followup) =
        platform_update_target(state, session_id, conversation, sender_id)?;
    let reservation = followup.try_reserve();
    Some(ActiveSenderTurn {
        run_id,
        turn_id,
        followup,
        reservation,
    })
}

/// 并进去时是排队还是取代当前生成。
///
/// **工具正在跑**说明她在真干活,排队别打断,下一步开始前读到;否则她只是在写
/// 回复,新消息该取代它,带上新消息重写。
///
/// 08-29 取证:QQ 里一句话拆成几条发是常态。用户先发"这是什么鱼"、三秒后补图,
/// 回合已经带着"没有图"开跑并写出"你没发图我怎么知道",这句被中间消息通道投递
/// 了出去,随后消费队列才答对——用户看到的是先装瞎再答题。
///
/// Supersede 与 Followup 走同一条入队通道,只多一个 `supersede.trigger()`
/// (runtime/turn_update.rs),agent 收到后丢弃当前生成的正文,那句半成品就不会
/// 被 flush 出去。
pub(in crate::platforms::onebot) fn active_turn_update_mode(
    tool_executing: bool,
) -> TurnUpdateMode {
    if tool_executing {
        TurnUpdateMode::Followup
    } else {
        TurnUpdateMode::Supersede
    }
}

pub(in crate::platforms::onebot) enum MergeOutcome {
    Merged,
    /// 限流挡下了,这条不回。
    RateLimited,
    /// 那一轮恰好收尾,没并进去。
    Missed,
}

/// 记一次限流账,再把消息并进那一轮。调用方负责先让插件看过这条消息
/// (`observe_inbound`)。
#[allow(clippy::too_many_arguments)]
pub(in crate::platforms::onebot) async fn merge_into_active_turn(
    state: &DaemonState,
    conn: &ConnectionHandle,
    target: Target,
    event: &Value,
    parsed: InboundMessage,
    inbound_event: &PlatformInboundEvent,
    context: &PlatformTurnContext,
    admission: &Admission,
    session_id: &str,
    turn: ActiveSenderTurn,
) -> MergeOutcome {
    let mode = turn.update_mode();
    let _ingress_reservation = turn.reservation;
    let _enqueue_order = turn.followup.lock_enqueue().await;
    let decision = charge_rate(state, admission);
    if decision != RateDecision::Allow {
        notify_rate_limited(context, target, decision).await;
        return MergeOutcome::RateLimited;
    }
    match enqueue_tool_followup(
        state,
        conn,
        target,
        event,
        parsed,
        inbound_event,
        &turn.followup,
        session_id,
        &turn.run_id,
        &turn.turn_id,
        mode,
    )
    .await
    {
        Ok(()) => {
            turn.followup.context.adopt_followup(inbound_event).await;
            tracing::info!(
                target: "yunxi::qq",
                session_id,
                sender_id = %inbound_event.sender_id,
                message_id = %inbound_event.message_id,
                mode = match mode {
                    TurnUpdateMode::Supersede => "supersede",
                    TurnUpdateMode::Followup => "followup",
                },
                "{}",
                t("OneBot message queued as a follow-up to the active turn", "OneBot 消息已加入当前回合的后续队列")
            );
            MergeOutcome::Merged
        }
        Err(error) => {
            tracing::warn!(
                target: "yunxi::qq",
                session_id,
                sender_id = %inbound_event.sender_id,
                error = %error,
                "{}",
                t("OneBot follow-up could not be queued", "OneBot 后续消息无法入队")
            );
            MergeOutcome::Missed
        }
    }
}
