# G5-06-02 consumer gate matrix 证据

日期：2026-10-06  
阶段：G5-06-DESIGN（test-only slice，未进入真实 provider 或生产采纳）  
代码提交：`b7bc6df9`

## 施工范围

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域增加
`GateProbeConfig`、控制场景和矩阵回放。五类未来消费者分别作为独立目标，依次验证：

- `all_enabled`：正常 record-only 比较；
- `disabled`：该消费者关闭，provider 不得被调用；
- `deadline`：该消费者预算为零，必须在 provider 前回退 timeout；
- `privacy`：该消费者隐私 gate 拒绝敏感 request，provider calls 必须为零；
- `audit_off`：只关闭该消费者的审计记录标记，不改变其他消费者结果。

每次只改变一个目标消费者，其他四个消费者都必须保持 deterministic primary、正常
record-only match、单次 provider call 和审计开启。夹具没有把配置类型暴露到生产 API，
也没有实现真正的开关存储或 scheduler。

## WSL 权威验证

在 Ubuntu-24.04 ext4 disposable target 上执行：

```text
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd <disposable-checkout> && rm -rf /tmp/g5-06-02-target && mkdir -p /tmp/g5-06-02-target && CARGO_TARGET_DIR=/tmp/g5-06-02-target cargo test -p yunxi-core --lib decision_shadow --locked -- --nocapture --test-threads=1'
```

结果：`34 passed; 0 failed; 0 ignored; 748 filtered out`。新增测试
`g5_06_consumer_gates_keep_switch_budget_privacy_and_audit_independent` 通过，且
G5-06-01 的 33 项回归仍全部通过。

矩阵断言为 5 个目标消费者 × 5 个控制场景 × 5 个消费者行 = 125 行：

- 5 个 disabled 行、10 个 timeout/privacy fallback 行、5 个 audit-off 行；
- 每个非目标消费者在目标故障时仍为 `match`、`provider_calls=1`、`audit_recorded=true`；
- 每个目标行的 deterministic primary digest 在 gate 变化前后保持不变；
- 所有 `side_effects=0`，序列化回放 bytes 稳定。

隐私摘要不包含 `api_key`、敏感 payload、password 或 profile 字段；privacy case 通过
实际 `observe` request validation seam 得到 `PrivacyRejected`，不是字符串模拟。

## Windows 静态门禁与清理

通过：

```text
cargo fmt --all -- --check
git diff --check
python testkit/privacy/g0_scan.py --repo .
```

隐私扫描：`status=passed`，personal path/credential/private key 均为 0；仅命中既有
fixture/public allowlist。`/tmp/g5-06-02-target` 已在测试结束后删除并确认不存在。

## 边界

本 slice 仍然是 deterministic/test-only adoption evidence。没有下载或加载 Laya 权重，
没有真实 provider、网络客户端、数据库迁移、生产 scheduler、memory/KB 写入或
terminal command execution。G5-06 仍停留在 DESIGN，后续必须另立施工单完成脱敏质量、
延迟、资源和人工影响审计后，才可讨论任何生产 consumer 采纳。

