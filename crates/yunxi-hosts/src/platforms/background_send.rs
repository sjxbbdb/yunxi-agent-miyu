//! 带图、带文件的消息在后台发（09-26 用户：「真人发图是发了就会挂着那自己上传」）。
//!
//! `send_message_to_user` 带附件时立刻返回，不等上传：线上 53 次成功的带图发送中位 11 秒，
//! 十次超过一分钟，最慢 178.8 秒（09-21..26 日志），这段时间她原来只能干等。现在她接着说话，
//! 图传完自己出现在对话里，先后不保证。
//!
//! 传失败了照后台任务的样子回报给她（[`UndeliveredHook`]，平台那边装）：她还在这个对话里回话就
//! 并进那一轮，闲着就替这个对话起一轮。传完之前，同样的图、同样的话再发一遍也算重复
//! （[`UploadsInFlight`]）：传完了由投递回执记进已投递，传失败就放掉，她还能重发。

use crate::platforms::*;
use serde_json::json;

/// 平台那边装进来的回报口：后台发的附件没传上去时，把一段说明交给这个对话。
pub(crate) type UndeliveredHook = Arc<dyn Fn(Arc<PlatformTurnContext>, String) + Send + Sync>;

/// 还在传的附件消息。
#[derive(Default)]
pub(crate) struct UploadsInFlight {
    next: AtomicU64,
    entries: Mutex<Vec<InFlight>>,
}

struct InFlight {
    id: u64,
    digests: Vec<blake3::Hash>,
    text: Option<DeliveredReplyText>,
}

impl UploadsInFlight {
    fn begin(&self, digests: Vec<blake3::Hash>, text: Option<&str>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let text = text.map(|text| DeliveredReplyText {
            normalized: reply_text_normalized(text),
            grams: reply_text_bigrams(text),
        });
        self.entries
            .lock()
            .unwrap()
            .push(InFlight { id, digests, text });
        id
    }

    fn finish(&self, id: u64) {
        self.entries.lock().unwrap().retain(|entry| entry.id != id);
    }

    pub(crate) fn digests(&self) -> Vec<blake3::Hash> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .flat_map(|entry| entry.digests.iter().copied())
            .collect()
    }

    /// 这段话是不是正在跟着某条附件一起传（判重规则与已投递的一样）。
    pub(crate) fn carries_text(&self, normalized: &str, grams: &HashSet<(char, char)>) -> bool {
        self.entries.lock().unwrap().iter().any(|entry| {
            entry.text.as_ref().is_some_and(|text| {
                text.normalized == normalized
                    || (grams.len() >= 16 && bigram_jaccard(grams, &text.grams) >= 0.66)
            })
        })
    }
}

/// 一条要在后台发的附件消息。
pub(crate) struct BackgroundSend {
    pub(crate) segments: Vec<OutboundSegment>,
    /// 跟着发的那段话（判重、成功后记账用）。
    pub(crate) text: Option<String>,
    /// 这条里新图的内容摘要（传完之前判重用）。
    pub(crate) image_digests: Vec<blake3::Hash>,
    /// 附件的文件名，失败时写进回报。
    pub(crate) names: Vec<String>,
}

/// 交给后台去发，立刻给出工具结果。
pub(crate) fn send_in_background(
    context: Arc<PlatformTurnContext>,
    send: BackgroundSend,
) -> String {
    let upload = context
        .uploads
        .begin(send.image_digests, send.text.as_deref());
    let task_context = context.clone();
    tokio::spawn(async move {
        let context = task_context;
        let result = context
            .send(OutboundMessage::segments(
                OutboundOrigin::Tool,
                send.segments,
            ))
            .await;
        match result {
            Ok(_) => {
                if let Some(text) = send.text.as_deref() {
                    context.record_delivered_reply_text(text);
                }
                context.uploads.finish(upload);
            }
            Err(error) => {
                context.uploads.finish(upload);
                tracing::warn!(
                    target: "yunxi::qq",
                    error = %error,
                    conversation_id = %context.conversation.conversation_id,
                    "{}",
                    yunxi_base::i18n::text(
                        "a background attachment send failed",
                        "后台发送的附件没有发出去"
                    )
                );
                if let Some(hook) = context.undelivered_hook.clone() {
                    let notice = undelivered_notice(&send.names, &error);
                    hook(context, notice);
                }
            }
        }
    });
    json!({
        "ok": true,
        "uploading": true,
        "message": "Uploading in the background, like a photo that keeps uploading after you hit send. It shows up in the chat once the upload finishes. Carry on without waiting, and do not mention sending or uploading it. A notice arrives if the upload fails."
    })
    .to_string()
}

/// 传失败时交给她的那段话（模型可见，英文短句，只陈述发生了什么）。
fn undelivered_notice(names: &[String], error: &anyhow::Error) -> String {
    let mut reason = format!("{error:#}");
    if reason.chars().count() > 200 {
        reason = reason.chars().take(200).collect::<String>() + "…";
    }
    let what = if names.is_empty() {
        "attachment".to_string()
    } else {
        names.join(", ")
    };
    format!(
        "<upload-failed>The message you sent with {what} did not get through: {reason}. The chat may not have seen it.</upload-failed>"
    )
}
