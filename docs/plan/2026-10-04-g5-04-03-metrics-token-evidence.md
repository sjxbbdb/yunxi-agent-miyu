# G5-04-03 Admission Shadow Metrics 与异步 Token 证据

日期：2026-10-04  
提交：`b683d750`  
阶段：G5-04-03（无模型、无异步 runtime、无真实 provider）

## 实现边界

本切片只扩展 memory admission 的只读观测 seam，deterministic admission 仍是唯一
primary。没有下载或加载 Laya 权重，没有新增线程、网络、scheduler、数据库写入、
embedding 或 KB 路径。

### Metrics

`crates/yunxi-core/src/memory/admission.rs` 的 `AdmissionShadowMetrics` 是调用方持有的
内存对象，不进入配置、数据库、日志或 replay payload。它提供：

- `shadow_started`、`shadow_completed`；
- 11 个固定 `ShadowMatchKind` 计数；
- 有界 latency bucket，并暴露 p50/p95/p99 上界；
- `record_completed` 的 `MemoryAdmission + Memory` task/scope 隔离门，跨 consumer 观测
  会被拒绝且不污染计数。

各字段通过 `AtomicU64` 记录；snapshot 的各字段读取是非事务性的，文档明确不能被解释
为同一瞬间的全局一致快照。计数失败不会改变 primary。

`observe_admission_shadow_with_metrics` 是未来调用方的显式 wrapper。只有
`RecordOnly + metrics` 才开始计数；`provider=None` 仍会记录 `Unavailable`，而默认
`Disabled` 完全不计数。当前 `apply_organized_batch` 仍使用既有
`Default + provider=None` 调用，不接入 metrics，也不改变生产写入语义。

### Async token

`AdmissionShadowToken` 不实现 `Serialize`，只能通过
`from_envelope(AdmissionDecisionEnvelope, diary_id, batch_database_id, batch_generation,
consumer_epoch, mode)` 构造。构造时重新校验 request/primary、`sha256:` + 64 位十六进制
fingerprint、非负 diary/generation 和受限 ASCII database id。

回调匹配必须同时满足 task、scope、fingerprint、diary、database、batch generation、
consumer epoch 和 mode；任一不匹配即返回 false。`matches_envelope` 会再次校验 request
与 primary。token 字段不会出现在 `ShadowObservation`、回放字节或回放摘要中，因此旧
响应不能凭 token 自行获得写入资格。

## 验收证据

| 环境 | 命令 | 结果 |
| --- | --- | --- |
| WSL Ubuntu-24.04 ext4 disposable checkout | `CARGO_TARGET_DIR=/tmp/g5-04-03-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1` | exit 0；17 passed，0 failed，752 filtered；耗时约 3m27s（首次编译） |
| Windows | `cargo fmt --all -- --check` | exit 0 |
| Windows | `cargo metadata --no-deps --format-version 1 --locked` | exit 0 |
| Windows | `python test_scripts/arch_dep_check.py` | exit 0 |
| Windows | `PYTHONDONTWRITEBYTECODE=1 python testkit/privacy/g0_scan.py --self-test` | passed |
| Windows | `PYTHONDONTWRITEBYTECODE=1 python testkit/privacy/g0_scan.py --repo .` | passed；credential/personal/private/unreadable 均为 0 |
| Windows | `git diff --check` | exit 0 |

测试覆盖全 11 类观测、三分位延迟、跨 task/scope 拒绝、provider 缺失/取消/队列满、
默认关闭、primary 等价、非法 fingerprint/database id、负生命周期字段和 token 不进入
observation payload。WSL 临时 source、target、日志与 cargo/rustc 进程均已清理。

## 未实现与下一边界

- 尚未创建 async runtime/task，也没有真实迟到回调；当前 token 只证明构造和 stale
  mismatch 规则，不能宣称已经完成异步响应丢弃的端到端证据。
- 尚未接入 Laya、真实 DecisionPort provider、memory admission 写入资格、recall/rerank
  或 KB。production provider 仍为 `None`，主路径仍由 deterministic 规则决定。
- 下一施工单为 G5-04-04：在不引入 runtime/model/DB 写权限的前提下，用无模型 harness
  验证 token 的旧 batch/旧 generation/旧 epoch 响应只被丢弃并分类为 stale；完成后再
  重新评审是否允许进入下一个 memory consumer slice。

