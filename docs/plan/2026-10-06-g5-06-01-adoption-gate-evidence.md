# G5-06-01 consumer adoption gate 证据

日期：2026-10-06  
阶段：G5-06-DESIGN（test-only slice，未进入真实 provider 或生产采纳）  
代码提交：`87253d2d`

## 施工范围

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域增加
consumer adoption harness。它以既有 `DecisionRequest`、deterministic primary 和
`observe` seam 为基础，覆盖五类未来消费者：

- `context_salience`
- `memory_admission`
- `recall_rerank`
- `terminal_intent`
- `proactive_ranking`

每个消费者独立回放同一组八类情况：`disabled`、`match`、`mismatch`、`invalid`、
`timeout`、`privacy`、`stale`、`closed`。harness 只产生脱敏元数据行，不把 payload、
候选文本、profile 或 provider 内容写入结果，也不调用数据库、工具、scheduler、网络
或模型 runtime。

## WSL 权威验证

在 Ubuntu-24.04 ext4 disposable target 上执行：

```text
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd /mnt/d/YunXi-Miyu && rm -rf /tmp/g5-06-01-target && mkdir -p /tmp/g5-06-01-target && CARGO_TARGET_DIR=/tmp/g5-06-01-target cargo test -p yunxi-core --lib decision_shadow --locked -- --nocapture --test-threads=1'
```

结果：`33 passed; 0 failed; 0 ignored; 748 filtered out`。新增测试
`g5_06_consumer_adoption_gate_is_independent_and_fails_closed` 通过；其余
`decision_shadow` 回归也全部通过。

回放摘要断言：

- 5 consumers × 8 cases = 40 metadata rows；
- `disabled = 5`，`fallback = 30`；
- 五个 match 行均为 deterministic baseline 与 shadow 同为 abstain 的
  `both_abstain`，不是伪造的模型命中；
- `provider_calls = 0` 仅出现在 disabled/privacy/closed 等要求不调用 provider 的
  路径，privacy gate 在 provider 前拒绝；
- `primary_unchanged = true`，`side_effects = 0`；
- 序列化 replay bytes 稳定。

覆盖的失败分类由真实 `observe` seam 产生：`OutcomeMismatch`、`InvalidShadow`、
`Timeout`、`PrivacyRejected`、`StaleFingerprint` 和 `Unavailable`。因此这批证据验证
的是独立闸门与 fail-closed 记录，不是 provider 质量或业务收益。

## Windows 静态门禁与隐私

通过：

```text
cargo fmt --all -- --check
git diff --check
python testkit/privacy/g0_scan.py --repo .
```

隐私扫描：`status=passed`，tracked files `1924`，credential/private path/private key
均为 0；仅命中既有 fixture/public allowlist。

## 清理与边界

- `/tmp/g5-06-01-target` 已在测试完成后删除并确认 `target-clean`。
- 未下载或加载 Laya 权重；未新增 provider、网络客户端、数据库迁移、生产 scheduler
  或 consumer 调用。
- 未修改 deterministic primary 或任何生产写入语义。
- 当前阶段仍为 G5-06-DESIGN；真实 provider、质量/延迟/RAM 评测以及生产 consumer
  采纳必须等待新的施工单和独立审计，不能由本 slice 推进。

