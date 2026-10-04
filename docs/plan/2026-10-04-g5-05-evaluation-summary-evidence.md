# G5-05-05 统一 deterministic 评测摘要证据

- 任务：G5-05-05
- 代码提交：`df4b0f36`（`G5-05 add deterministic evaluation summary`）
- 记录时间：2026-10-04（Asia/Singapore）
- 阶段：G5-05-DESIGN；本证据不推进 G5-06

## 实现边界

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域新增
`g5_05_unified_evaluation_summary_is_redacted_and_replayable`。它只构造脱敏的
元数据行，不读取用户档案、记忆原文、模型权重或网络数据，也不改变任何生产路径。
摘要覆盖七类固定行为：重要约束保留、闲聊 abstain、敏感输入拒绝、矛盾 abstain、
重复记忆 no-op、memory/KB 边界 no-op、模糊终端意图澄清。

摘要字段实际断言：

- `total/accepted/rejected/abstained/clarified/no_op` 与七行混淆矩阵一致；
- memory admission 与 decision shadow 各自输出 p50/p95/p99 和 timeout 计数；
- invalid output、disconnect、closed transport、timeout 的回退计数；
- `primary_digest_equal=true`、`memory_kb_cross_pollution=0`、`sensitive_write_count=0`；
- RAM 明确记录为 `unavailable`，不猜测当前主机的 RSS；
- JSON 序列化两次字节完全一致，且不含 `redacted_text`、`password` 或 `secret`。

## WSL 验证

环境：WSL `Ubuntu-24.04`，源码位于 Windows 工作区映射的 ext4 访问路径；使用一次性
`/tmp/g5-05-05-target`，测试结束后已删除。

```text
CARGO_TARGET_DIR=/tmp/g5-05-05-target \
  cargo test -p yunxi-core --lib decision_shadow --locked -- \
  --nocapture --test-threads=1

running 32 tests
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 748 filtered out
```

其中新增摘要测试与既有 fallback、replay、privacy、queue、primary 不变性测试均通过。
清理复核：`/tmp/g5-05-05-target` 不存在；没有启动 daemon、MCP、provider 或监听端口。

## Windows 门禁

- `cargo fmt --all -- --check`：通过。
- `git diff --check`：通过。
- 隐私扫描：沿用 G5-05 前序通过结果；本批仅含 test-only 元数据和本证据文档，未加入
  token、个人路径、profile、模型权重或完整私有原文。

## 结论与未验证项

本批补齐了 G5-05 要求的统一摘要格式和回放断言，但它仍是 deterministic/test-only
评测，不代表真实 Laya/provider、真实异步 transport、RAM 峰值、Arch/macOS 或生产
consumer 已验证。G5-05 仍未退出；下一步必须继续补齐剩余质量/回归证据，并在退出审计中
明确列出这些边界。
