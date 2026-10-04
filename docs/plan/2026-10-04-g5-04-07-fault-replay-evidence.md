# G5-04-07 fault/replay boundary evidence

## Result

G5-04-07 的无模型 fault/replay 边界测试已完成并推送，代码提交为
`5daf238bf5b5fb4c70f86bd1a6e79aabc9f721d6`。

本批只修改 `memory/tests/admission.rs`。生产 admission、DecisionPort、shadow runtime、
write path、数据库和 schema 均未修改。

## 证据分层

- 新增 `admission_fault_replay_is_stable_and_closed_transport_has_no_observation` 使用
  已脱敏的合成 `ShadowObservation` 验证 metrics/replay：6 类 fault/stale 分类均只计一次，
  replay bytes/digest 稳定、`sha256:` + 64 hex，且不含 raw、diary/database/generation/
  epoch/mode 等私有字段。它不是实际 provider fault 的运行时调用证明。
- 新增 closed transport 断言：空 channel 返回 `TryRecvError::Disconnected`，receiver 已关时
  `try_send` 返回 `TrySendError::Disconnected`；closed 不产生 observation、replay 或 apply。
- 实际 observer fault 路径由既有 `decision_shadow` 回归覆盖：invalid shadow、同步 stale、
  cancelled、timeout、unavailable、privacy rejection、queue-full、RAII permit 和 primary
  digest 不变性。privacy rejection 在 request gate 处返回错误，不会伪造 observation。
- 既有 `admission_shadow_metrics_count_every_match_kind_in_stable_snapshot` 覆盖全部 11 类
  `ShadowMatchKind` 及 p50/p95/p99；本批不重复声明新的生产指标 adapter。

## 验证证据

### WSL Ubuntu-24.04 / ext4 disposable checkout

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 23 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

使用的 `/tmp/g5-04-07-src` 与 `/tmp/g5-04-07-target` 已删除，未留下 cargo/rustc/yunxi/miyu
进程。

### Windows 静态门禁

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

## 非目标与残余风险

本批不宣称真实异步 provider、跨线程生产 runtime、Laya 权重、生产 consumer、数据库/KB
写入、跨重启 replay/幂等、强制取消、Arch 实机或 scheduler/权限接入已实现。

远端 `origin/codex/yunxi-product-rename` 已指向
`5daf238bf5b5fb4c70f86bd1a6e79aabc9f721d6`。
