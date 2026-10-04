# G5-04-07 无模型 fault/replay 边界设计合同

日期：2026-10-04  
阶段：G5-04-07-DESIGN  
前置：G5-04-06 async-boundary evidence `af0306f2`。  
性质：只补 test-only fault、metrics 和 replay 证据；不授权生产 runtime 或模型接入。

## 事实锚点

- `decision_shadow.rs` 已定义 `ShadowError` 的 unavailable/timeout/cancelled/privacy/queue
  分类和 `ShadowMatchKind` 的完整观测分类；`observe_inner` 在同步调用栈内完成校验、
  precheck、provider 结果映射，primary 不被修改。
- `observation_replay_bytes`/`observation_replay_digest` 只编码无原文的
  `ShadowObservation`；`AdmissionShadowMetrics` 是 caller-owned 的内存原子计数。
- G5-04-06 的 bounded channel 夹具目前只证明正常关闭后可 drain；本阶段补充显式
  disconnected/closed 路径，但不向生产 `ShadowError` 增加 `Closed`。

## 允许范围

只允许修改：

- `crates/yunxi-core/src/memory/tests/admission.rs`：测试用 fault 状态、metrics/replay
  断言和 mpsc disconnected 夹具；
- `docs/plan/*` 与 `next-release-note.md`：施工与证据。

禁止修改 `decision_shadow.rs`、生产 `admission.rs`/`memory/write.rs`、DecisionPort、
DB/schema、KB、runtime、Laya/provider、TUI/Web、fish、daemon、scheduler 和 voice。

## 必须覆盖

1. invalid、stale、cancelled、timeout、unavailable、queue-full、closed 各一条 test-only
   路径；每条断言 `applied=0` 或既有 fresh 结果不变，primary envelope 等价。
2. closed 通过 mpsc `RecvError`/`TryRecvError::Disconnected` 表示，不伪造新的生产错误类。
3. 所有产生 observation 的 fault 记录一次 `AdmissionShadowMetrics`，replay bytes/digest
   稳定、带 `sha256:` 形状且不含原文、token id、database/generation/epoch/mode。
4. closed 不产生 observation/replay；token 仍不可序列化。
5. 现有 decision_shadow fault、golden replay、privacy 和 RAII queue 回归继续通过。

## 不宣称与验收

本阶段不宣称真实异步 runtime、数据库、生产 consumer、Laya/模型、跨重启持久化 replay、
强制取消或 Arch 实机。WSL Ubuntu-24.04 ext4 跑 admission 定向测试，Windows 跑 fmt、
locked metadata、架构依赖、隐私扫描和 diff check；显式删除 `/tmp/g5-04-07-src`、
`/tmp/g5-04-07-target` 并确认无 cargo/rustc/yunxi/miyu 进程后才能提交推送。
