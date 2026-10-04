# G5-02 deterministic DecisionPort 证据

日期：2026-10-04  
阶段：G5-02  
提交：`2b458d39`  
状态：已完成并推送；长期 Goal 继续进入 G5-03，不代表 G5–G9 完成。

## 1. 本切片落地内容

- 新增 `crates/yunxi-core/src/decision.rs`，并在
  `crates/yunxi-core/src/lib.rs` 注册 `decision` 模块。
- 固化 `yunxi.decision.v1` 的 `DecisionRequest`、`DecisionResult`、task/scope/
  capability/reason 枚举和窄 `DecisionPort` trait。
- 请求校验包含：候选 ID 去重/数量/字节长度、能力集合去重与非空、payload 必须为
  object、64 KiB/16 层/1024 集合项上限、敏感键递归拒绝，以及 canonical JSON
  SHA-256 fingerprint。`deadline_ms` 已进入 fingerprint。
- 结果校验包含：schema/task/fingerprint/provider 边界、`elapsed_ms` 不得超过
  request deadline、choice/ranking/score 的 capability 与候选/数值边界、空 ranking
  拒绝、abstain 一致性；未知候选错误不回显调用方 ID。
- request/result/error 的 `Debug`/`Display` 都采用脱敏输出，不打印 payload、候选 ID、
  schema 原文或错误细节；capability 集合按稳定顺序参与 fingerprint，结果 digest
  使用 canonical JSON SHA-256。
- `DeterministicDecisionPort` 只返回 `abstain=true`、`reason_code=deterministic_baseline`
  的零副作用结果，且必须显式声明 `abstain` capability；没有模型、网络、存储、工具
  或调度依赖。
- `test_scripts/arch_dep_check.py` 将 `decision` 注册到存储与协议层，保持依赖方向
  门禁可见。

## 2. 验收证据

### Windows 只读/门禁

以下命令在提交前通过：

```text
cargo fmt --all -- --check
cargo metadata --no-deps --format-version 1
git diff --check
python test_scripts/arch_dep_check.py
python testkit/privacy/g0_scan.py --self-test
python testkit/privacy/g0_scan.py --repo .
```

隐私扫描结果：`credential_shape=0`、`personal_path=0`、`private_key=0`、
`unreadable=0`，`tracked_files=1896`。扫描中的 fixture/public allowlist 是既有
受控路径，不是本切片新增秘密。

### WSL Ubuntu-24.04 ext4 权威运行

从当前工作树创建 disposable ext4 checkout（排除 `.git`、`target`、`.tmp`），运行：

```text
cargo fmt --all -- --check
cargo test -p yunxi-core decision --lib --locked -- --nocapture --test-threads=1
```

结果：**8 passed、0 failed、719 filtered out**。覆盖稳定 fingerprint、deadline 变化与
零 deadline、deterministic abstain、capability 缺失、候选/隐私边界、payload 限制、
provider/elapsed/ranking 结果边界和 serde round-trip。测试后 disposable checkout
已删除，确认 `/tmp/yunxi-g502-final` 不存在。

此前同一 WSL ext4 基线的全量 `cargo test -p yunxi-core --lib --locked` 为
711 passed、8 ignored、5 failed；5 个失败均位于既有
`llm::openai_compatible` endpoint/error-message 测试：

- `endpoint_retry::invalid_request_does_not_fail_over_to_another_endpoint`
- `error_text::a_rate_limit_says_it_is_a_rate_limit`
- `error_text::a_relay_failure_never_claims_an_http_status`
- `error_text::a_single_endpoint_pool_does_not_say_every_endpoint`
- `error_text::the_all_failed_message_lists_every_endpoint_and_nothing_else`

这些失败不触及 `decision` 模块；本切片不把全量结果伪装成全绿，也没有修改这些既有
测试或 LLM 逻辑。

## 3. 明确边界与残余风险

- 尚无 Laya/shadow provider、模型权重、sidecar、Python/Node/ONNX 依赖。
- 尚无任何 memory admission、recall/rerank、terminal intent、proactive ranking
  消费者接入；既有 deterministic 业务规则仍是唯一行为来源。
- `DecisionPort` 不拥有 memory/KB/profile、host grant、MCP、fish executor 或
  scheduler 写句柄；结果不能直接执行动作。
- 本切片未证明真实模型质量、中文/Linux 术语质量、p50/p95/RAM、Arch 实机运行或
  全量 core 的既有 LLM 测试问题；这些保持在后续阶段或既有风险清单中。

## 4. 下一施工单

进入 G5-03：只设计并测试默认关闭的 shadow provider 观测支路。deterministic baseline
永远权威；shadow 只能读取已脱敏 request、记录最小摘要，超时/断连/非法结果/隐私
失败不得等待、重试、写入或改变主结果。G5-03 仍禁止下载/加载 Laya 权重和消费者接入。
