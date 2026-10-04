# G5-04-08 无模型 metrics/replay 收敛设计合同

日期：2026-10-04  
阶段：G5-04-08-DESIGN  
前置：G5-04-07 fault/replay evidence `5daf238b`。  
性质：只设计 test-only 观测收敛夹具；不授权生产 metrics API、数据库或模型接入。

## 事实锚点

- `AdmissionShadowMetrics` 是 caller-owned、非持久化的原子计数；当前 `record`/`record_completed`
  对每次合法 observation 计数，不承担跨批次去重或 replay ledger 职责。
- `observation_replay_bytes`/`observation_replay_digest` 由固定字段生成无原文 replay；
  digest 包含 observation 的稳定字段和 elapsed，不包含 token/lifecycle 私有字段。
- G5-04-05/06 的 consumer harness 只在测试内存中 pop pending；生产 `apply_organized_batch`
  仍由 deterministic admission 独占。

## 允许范围

只允许修改 `crates/yunxi-core/src/memory/tests/admission.rs` 的 test-only helper/test，
以及本目录施工/证据文档和 `next-release-note.md`。禁止修改生产 metrics API、
`decision_shadow.rs`、`admission.rs`、`memory/write.rs`、DB/schema、KB、runtime、Laya、
provider、TUI/Web、fish、daemon、scheduler 和 voice。

## 施工契约

在测试夹具内建立 `HashSet<replay_digest>` 收敛模型，不把它伪装成生产持久化 ledger：

1. 同一 observation 重复投递只在夹具的 unique 集合中记一次；原始
   `AdmissionShadowMetrics` 仍按调用次数计数，证明职责分离。
2. 跨两个模拟 batch，重复 digest 不增加 unique 数，新 digest 增加一次；snapshot 的
   p50/p95/p99 只由 latency bucket 决定，primary envelope 前后相等。
3. replay bytes/digest 必须字节稳定、`sha256:` + 64 hex、无 raw/private/token 字段。
4. empty/closed transport 不产生 observation、replay 或 metrics completed；invalid/stale
   先在既有顺序中丢弃。

## 验收与不宣称

WSL Ubuntu-24.04 ext4 运行 admission tests；Windows 运行 fmt、locked metadata、架构依赖、
隐私扫描和 diff check；清理 `/tmp/g5-04-08-src`、`/tmp/g5-04-08-target` 与进程。
本阶段不宣称跨重启持久化去重、分布式 exactly-once、生产 metrics 聚合、真实异步 runtime、
Laya/provider、DB/KB/consumer 写入或 Arch 实机证据。
