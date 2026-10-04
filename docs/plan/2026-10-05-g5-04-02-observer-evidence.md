# G5-04-02 Admission Observer 接入证据

日期：2026-10-05  
提交：`3c4aed3a`  
阶段：G5-04-02（默认关闭、同步、record-only；无真实 provider）

## 实现范围

- `AdmissionShadowConfig` 是 memory-local、非持久化配置；`Default` 固定为
  `ShadowMode::Disabled`，budget 仅使用有界常量。
- `observe_admission_shadow` 只转发到既有 `decision_shadow::observe_with_budget`/
  `observe_with_queue`，借用 envelope 的 primary，不修改 primary、不持有写句柄。
- `apply_organized_batch` 在构造 `admission_by_id` 后只有一个 observer seam：每条 diary
  构造一次 raw-free envelope，调用 `Default + provider=None + queue=None`，完全忽略
  observation/error；后续 admitted/promoted、lifecycle、DB、embedding 和 KB 路径保持原样。
- RecordOnly fake provider、timeout、cancelled、queue-full 测试只验证 observation 分类和
  provider 调用次数，不把 fake 建议写入 memory。

## 验收证据

| 环境 | 命令 | 结果 |
| --- | --- | --- |
| WSL Ubuntu-24.04 ext4 | `CARGO_TARGET_DIR=/tmp/g5-04-observer-verifier CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p yunxi-core --lib memory --locked -- --nocapture --test-threads=1` | exit 0；101 running，100 passed，0 failed，1 ignored，662 filtered；耗时约 5.45s |
| Windows | `cargo fmt --all -- --check` | exit 0 |
| Windows | `cargo metadata --no-deps --format-version 1 --locked` | exit 0 |
| Windows | `python test_scripts/arch_dep_check.py` | exit 0 |
| Windows | `PYTHONDONTWRITEBYTECODE=1 python testkit/privacy/g0_scan.py --self-test` | passed |
| Windows | `PYTHONDONTWRITEBYTECODE=1 python testkit/privacy/g0_scan.py --repo .` | passed；credential/personal/private/unreadable 均为 0 |
| Windows | `git diff --check` | exit 0 |

WSL verifier 删除并确认 `/tmp/g5-04-observer-verifier` 不存在；没有 cargo/rustc 残留。
ONNX runtime 缺失导致的 3 个既有测试按仓库逻辑 skip，不是本切片失败。G5-03 的
decision/shadow 定向基线仍为 decision 39/39、shadow 30/30。

## 边界与下一步

- 生产仍没有真实 DecisionPort provider、Laya 权重、网络、模型 runtime 或 async task；
  `provider=None` 是当前唯一生产调用。
- 当前 observer 不等待、不重试、不改变 deterministic admission，不提供 memory/KB 写入
  资格或权限凭证。
- `deadline_ms`/queue/cancel 只在显式内存测试配置下验证；未来任何可能阻塞的 provider
  必须另立 async adapter 合同，绑定 fingerprint、batch generation 和 consumer epoch，
  迟到响应只能丢弃。
- 下一施工单 G5-04-03：独立设计无模型 consumer metrics/replay 与异步 token harness，
  先补 unknown-field/stale/allowlist fault coverage，再讨论真实 provider；不加载 Laya。
