# G5-04-08 metrics/replay convergence evidence

## Result

G5-04-08 的 test-only metrics/replay 收敛夹具已完成并推送，代码提交为
`d52bc35752fabb494589ba433cedc34187924725`。

本批只修改 `crates/yunxi-core/src/memory/tests/admission.rs` 与测试文档；生产
`AdmissionShadowMetrics`、DecisionPort、shadow runtime、DB、schema 和 write path 未改。

## 证明的边界

- `HashSet<observation_replay_digest>` 只在测试夹具中去重完全相同的 canonical replay；
  生产 metrics 仍按调用计数，未新增生产幂等语义。
- 重复的 OutcomeMatch/Timeout observation 只向夹具 metrics 贡献一次，elapsed 变化会
  改变 digest，故不会被错误合并。
- 夹具断言 `shadow_started=2`、`shadow_completed=2`、两类各 1，latency p50=50、
  p95/p99=100，primary envelope 前后相等。
- replay bytes/digest 稳定、`sha256:` + 64 hex，且不含 raw/private/token/database/
  generation/epoch/mode 字段；并发原子快照、持久化或跨重启 replay 不在本批范围。
- closed/fault 的 transport 与实际 observer fault 仍由 G5-04-07 和既有 decision_shadow
  回归负责，本测试不把合成 observation 伪装成 provider 运行时路径。

## 验证证据

### WSL Ubuntu-24.04 / ext4 disposable checkout

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

`/tmp/g5-04-08-src` 与 `/tmp/g5-04-08-target` 已删除，未留下 cargo/rustc/yunxi/miyu
进程。

### Windows 静态门禁

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

## 非目标与残余风险

本批不宣称生产 metrics 去重、跨线程一致性、持久化/跨重启幂等、真实异步 runtime、
Laya/provider、生产 memory/KB consumer、DB/schema 写入、权限/scheduler 接入或 Arch
实机证据已完成。

远端 `origin/codex/yunxi-product-rename` 已指向
`d52bc35752fabb494589ba433cedc34187924725`。
