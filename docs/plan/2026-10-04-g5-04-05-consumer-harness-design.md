# G5-04-05 consumer-facing harness 设计合同

日期：2026-10-04  
阶段：G5-04-05-DESIGN  
前置：G5-04-04 stale-response gate `0e820f54` 及证据文档。  
性质：仅测试夹具与内存内消费契约；不授权 Laya、真实 provider、异步 runtime 或生产写入改造。

## 目标

验证未来 memory admission consumer 在“验证响应 → 校验新鲜度 → 应用结果”的顺序中，
不会把 shadow 建议变成生产写入资格。所有动作都发生在 `cfg(test)` 内存夹具中：

1. 接收已构造的 `AdmissionShadowToken`、当前 `AdmissionShadowContext`、request 和
   provider `DecisionResult`。
2. 先调用 `validate_result`，再调用 `admit_admission_shadow_response`。
3. 只有两道门都通过时，增加本地 `applied` 计数；不访问 `MemoryStore`、SQLite、
   organizer、scheduler、权限、KB 或任何外部进程。
4. invalid、stale、cancelled、queue-full、timeout 和 provider unavailable 只增加
   对应的内存计数并丢弃，不能改变 primary envelope。

## 允许修改范围

- `crates/yunxi-core/src/memory/tests/admission.rs`：新增 test-only harness、fake result
  构造器和回归测试。
- `docs/plan/*`：记录施工单、命令、计数、清理和未验证边界。

禁止修改 `memory/write.rs`、`memory/admission.rs` 生产逻辑、`decision.rs`、
`decision_shadow.rs`、schema/migration、KB、TUI/Web、fish、daemon、scheduler 和 voice。

## 必须覆盖的测试

- fresh + 合法 choice：恰好应用一次，primary 不变；
- invalid result：provider、schema、candidate、confidence 或 elapsed 越界时丢弃；
- stale response：task、scope、fingerprint、diary、database、generation、epoch、mode
  任一变化时返回 `StaleFingerprint`，不应用；
- cancelled/queue-full/timeout/unavailable 的 primary 等价；
- response 验证顺序固定为 `validate_result` 后 freshness gate；
- 测试夹具不产生序列化 token，不保存原文，不触碰数据库或外部 runtime。

## 验收与清理

WSL Ubuntu-24.04 ext4 disposable checkout 运行：

```bash
CARGO_TARGET_DIR=/tmp/g5-04-05-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
  cargo test -p yunxi-core --lib memory::tests::admission --locked -- \
  --nocapture --test-threads=1
```

Windows 运行 `cargo fmt --all -- --check`、locked metadata、架构依赖检查、隐私扫描和
`git diff --check`。测试后删除 disposable source/target、日志和 fixture，确认无
cargo/rustc/yunxi/miyu 进程；通过后立即提交并推送。此阶段不宣称真实异步响应或生产
consumer 已实现。
