# G5-06-03 consumer adoption measurement protocol 设计

阶段：G5-06-DESIGN  
状态：设计合同，未执行真实 provider

## 目的

G5-06-01/02 已证明五个 consumer 的 gate、fallback、隐私和回滚边界可以独立回放，
但这些 test-only 夹具不能证明真实模型质量、延迟或资源占用。本协议先固定将来
provider 评测的输入、输出和统计方法，避免用合成数字填充真实门槛。

## 每次评测的固定身份

每个 run 必须生成一个脱敏 manifest，字段只允许：

```text
run_id, stage, consumer, provider_id, provider_revision, decision_schema,
fixture_revision, sample_count, seed, started_at_utc, environment,
budget_ms, queue_capacity, mode, cleanup_id
```

禁止写入 payload、profile、raw/display/context 原文、候选文本、凭据、模型输入输出
正文。`provider_revision` 在 provider 尚未存在时为 `unavailable`，不能用占位 SHA
伪装已测。

## 五类 consumer 的输入与 oracle

| consumer | 允许的输入形状 | deterministic oracle | 不能改变的边界 |
| --- | --- | --- | --- |
| context salience | 脱敏候选 ID、scope、结构化 salience 标签 | primary 只排序候选 | 不返回原文、不改 prompt/profile |
| memory admission | 脱敏 memory class、confidence、source、force 标记 | admit/reject/abstain 规则 | 不直接写 memory/embedding |
| recall rerank | candidate ID、truth/confidence、tombstone 元数据 | 现有排序规则 | 不返回候选正文、不跨 KB |
| terminal intent | 脱敏 intent class、scope、候选动作 ID | choice/clarify/abstain | 不执行命令、不提权、不改 fish |
| proactive ranking | 脱敏 event class、cooldown、relationship scope | queue/abstain | 不主动发消息、不创建 scheduler |

每个样本必须同时跑 deterministic primary 与 record-only provider；provider 结果只有
在 schema、fingerprint、scope、capability、deadline、隐私 gate 全部通过后才可计入
比较，其余统一记为 fallback reason。

## 统计方法

- 质量：按 consumer 分开输出 `exact_match`、`outcome_match`、`outcome_mismatch`、
  `abstain`、`invalid`、`privacy_rejected`、`stale`、`timeout`、`unavailable`；保留
  样本分母和 fixture revision。
- 延迟：只用单调时钟测量 provider seam，从调用前到返回后；分别报告 p50/p95/p99、
  sample_count 和 timeout_count。没有实际样本时写 `unavailable`，不填 0 或合成值。
- 资源：WSL 可用时用 disposable process 的 peak RSS 采集；工具缺失、进程被限制或
  测试没有真实 provider 时写 `unavailable`。不把编译内存、宿主机总内存或估算值当作
  provider RAM。
- 影响：每个 mismatch 必须关联稳定 reason、primary digest、fallback、side_effects
  和人工可审计的结构化影响类别；不得保存文本正文。
- 重放：同一 `run_id + fixture_revision + seed + provider_revision` 的 canonical
  metadata bytes 必须幂等；不同 revision 必须产生不同 run 身份，不能覆盖旧证据。

## 故障矩阵

每个 consumer 必须至少有：disabled、deadline=0、queue full、disconnect、provider
timeout、invalid schema、stale fingerprint、privacy rejection、primary mismatch、
audit-off。每种情况都要求 provider calls、fallback reason、primary digest、side effects
和 cleanup 结果可验证。非目标 consumer 同批回放，证明故障不串扰。

## 运行与清理

权威环境为 WSL Ubuntu-24.04 ext4 disposable checkout；Windows 只跑 fmt、diff、metadata
和隐私扫描。一次只运行一个 cargo；每个 run 使用唯一 `CARGO_TARGET_DIR`、隔离 home、
临时数据库和输出目录。结束后删除 target/数据库/manifest 原文/子进程，并保留仅脱敏的
计数摘要和 cleanup_id。

## 当前边界

本协议不授权下载 Laya 权重、创建网络 provider、改生产 DecisionPort 调用者、接入
memory/KB/profile/terminal scheduler 或提出质量阈值。只有新的施工单明确 provider 来源、
许可证、revision、样本和人工审计方式后，才可执行真实评测；在此之前 G5-06 保持
DESIGN，长期 goal 保持 active。

