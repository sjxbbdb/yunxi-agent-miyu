# G5-04-05 consumer-facing harness evidence

## Result

G5-04-05 的 test-only memory admission consumer harness 已完成并推送，代码提交为
`7a7afb3bff1dfaec64e9d277f8808dac0b0d7b57`。

夹具只存在于 `crates/yunxi-core/src/memory/tests/admission.rs` 的测试代码中，生产
`memory/write.rs`、`memory/admission.rs`、DecisionPort、shadow runtime、schema 和
数据库均未修改。

## 证明的顺序与边界

- pending response 由 `VecDeque` 保存，`pop_front` 后只处理一次；不提供跨重启或持久化
  replay 幂等保证。
- drain 顺序固定为 `validate_result(request, response)`，再调用
  `admit_admission_shadow_response(token, current)`，最后才增加内存 `applied` 计数。
- 合法 fake provider response 被应用一次；provider、schema、candidate、confidence、
  elapsed 等非法响应在 freshness gate 前丢弃。
- task、scope、fingerprint、diary、database、generation、epoch、mode 任一变化均被
  分类为 stale 丢弃；已有 G5-04-04 直接 gate 测试同时证明 exact `StaleFingerprint`。
- 测试只保存固定 allowlist request metadata 和 DecisionResult，不访问原文、MemoryStore、
  SQLite、organizer、scheduler、权限、KB 或外部进程。
- cancelled、queue-full、timeout、unavailable 的 primary 等价沿用既有 observer fault
  回归；本批不把它们宣称为异步 runtime 证据。

## 验证证据

### WSL Ubuntu-24.04 / ext4 disposable checkout

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

测试使用 `/tmp/g5-04-05-src` 与 `/tmp/g5-04-05-target`，完成后已删除；未留下
cargo/rustc/yunxi/miyu 进程。

### Windows 静态门禁

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

## 非目标与残余风险

本批不宣称真实异步 provider、Laya 权重、跨线程调度、跨重启防 replay、生产 memory
consumer 或 KB consumer 已实现。下一阶段必须重新设计并审查是否需要受控的异步边界，
仍不得绕过 deterministic primary、lifecycle owner 或权限真相。

远端 `origin/codex/yunxi-product-rename` 已指向
`7a7afb3bff1dfaec64e9d277f8808dac0b0d7b57`。
