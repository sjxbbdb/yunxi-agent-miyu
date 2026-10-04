# G5-04-06 async-boundary harness evidence

## Result

G5-04-06 的无模型异步边界测试夹具已完成并推送，代码提交为
`af0306f2dbb6be7283163ebecc8290bf4148873f`。

本批只在 `crates/yunxi-core/src/memory/tests/admission.rs` 增加 test-only
`std::sync::mpsc::sync_channel(1)`、`Barrier` 和短命线程；没有修改生产写入路径、
DecisionPort、shadow runtime、schema 或数据库。

## 验证的异步形状

- producer→bounded channel→consumer 的 fresh response 跨线程应用一次；
- context 在 response 到达前漂移时，task/scope/fingerprint/diary/database/generation/
  epoch/mode 任一变化均 stale drop；
- consumer 的顺序仍为 fault/cancel 分类、`validate_result`、stale gate、内存计数；
- `sync_channel(1)` 第二项用 `try_send` 确定性得到 `QueueFull`，不阻塞首项；
- cancel/timeout/unavailable 只计数和丢弃，不调用 apply；
- producer/consumer 均 join，sender/receiver 关闭后没有泄漏；
- `VecDeque::pop_front` 证明进程内 single-pop/no replay；不宣称跨重启或持久化幂等。

测试夹具只携带已脱敏 request/response 和不可序列化 token；没有原文、MemoryStore、
SQLite、KB、organizer、scheduler、权限或外部进程。

## 验证证据

### WSL Ubuntu-24.04 / ext4 disposable checkout

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

测试使用 `/tmp/g5-04-06-src` 与 `/tmp/g5-04-06-target`，已在测试后删除；未留下
cargo/rustc/yunxi/miyu 进程。

### Windows 静态门禁

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

## 非目标与残余风险

本批不宣称真实异步 provider、tokio/第二 runtime、强制取消阻塞任务、Laya 权重、生产
memory/KB consumer、跨重启 replay/幂等、Arch 实机或 scheduler/organizer/权限接入已实现。

远端 `origin/codex/yunxi-product-rename` 已指向
`af0306f2dbb6be7283163ebecc8290bf4148873f`。
