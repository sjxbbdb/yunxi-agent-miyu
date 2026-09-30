//! 带附件的消息在后台发（09-26）：工具不等上传；传完之前同一张图再发算重复；传失败了经回报口
//! 交回这个对话，之后还能重发。

use super::shared::*;
use crate::platforms::*;
use futures_util::future::BoxFuture;
use serde_json::json;
use std::sync::atomic::Ordering as AtomicOrdering;

/// 放行一次才返回一次，每次都报上传失败：模拟 NapCat 迟迟不应、最后超时。
struct GatedAdapter {
    gate: tokio::sync::Semaphore,
    calls: AtomicUsize,
}

impl PlatformAdapter for GatedAdapter {
    fn send<'a>(&'a self, _message: OutboundMessage) -> BoxFuture<'a, Result<SendReceipt>> {
        Box::pin(async move {
            self.calls.fetch_add(1, AtomicOrdering::Relaxed);
            self.gate.acquire().await.unwrap().forget();
            anyhow::bail!("OneBot API send_group_msg timed out")
        })
    }

    fn bot_display_name<'a>(&'a self) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("YunXi".to_string()) })
    }
}

async fn send_picture(
    registry: &yunxi_engine::tools::ToolRegistry,
    path: &std::path::Path,
) -> String {
    tokio::time::timeout(
        Duration::from_secs(2),
        registry.call(
            "send_message_to_user",
            &json!({ "images": [{ "path": path }] }).to_string(),
        ),
    )
    .await
    .expect("工具不该等上传")
    .expect("发送该被受理")
}

#[tokio::test]
async fn an_attachment_uploads_in_the_background_and_a_failure_comes_back_as_a_notice() {
    let temp = tempfile::tempdir().unwrap();
    let paths = test_paths(temp.path());
    let adapter = Arc::new(GatedAdapter {
        gate: tokio::sync::Semaphore::new(0),
        calls: AtomicUsize::new(0),
    });
    let notices = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = notices.clone();
    let context = Arc::new(
        PlatformTurnContext::new(
            PlatformConversation {
                platform: "onebot".to_string(),
                account_id: "10000".to_string(),
                kind: ConversationKind::Group,
                conversation_id: "background-send".to_string(),
            },
            "20000".to_string(),
            "tester".to_string(),
            true,
            AppConfig::default(),
            paths.clone(),
            StateStore::new(&paths).unwrap(),
            adapter.clone(),
            Arc::new(plugins::PlatformPluginRegistry::default()),
        )
        .with_undelivered_hook(Arc::new(move |_context, notice| {
            recorded.lock().unwrap().push(notice);
        })),
    );
    let mut registry = yunxi_engine::tools::ToolRegistry::new();
    register_platform_tools(&mut registry, context.clone());
    let path = temp.path().join("pic.png");
    image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 200, 10, 255]))
        .save(&path)
        .unwrap();

    let first = send_picture(&registry, &path).await;
    assert!(first.contains("\"uploading\":true"), "{first}");
    let again = send_picture(&registry, &path).await;
    assert!(
        again.contains("\"deduplicated\":true"),
        "传完之前同一张图再发算重复：{again}"
    );

    adapter.gate.add_permits(1);
    let notice = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(notice) = notices.lock().unwrap().first().cloned() {
                return notice;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("上传失败该经回报口交回对话");
    assert!(notice.starts_with("<upload-failed>"), "{notice}");
    assert!(notice.contains("pic.png"), "{notice}");
    assert!(notice.contains("timed out"), "{notice}");

    let retry = send_picture(&registry, &path).await;
    assert!(
        retry.contains("\"uploading\":true"),
        "传失败的图该能重发：{retry}"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while adapter.calls.load(AtomicOrdering::Relaxed) < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("重发的那条该真的交给平台");
    adapter.gate.add_permits(1);
}
