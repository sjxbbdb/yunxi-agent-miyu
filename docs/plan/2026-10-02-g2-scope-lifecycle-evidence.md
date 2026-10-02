# G2-02 scope lifecycle evidence

> 状态：实现完成，等待主代理验收与提交。本文档记录当前工作树的验证证据，不代表已提交或已推送。

## 施工单

- 基线：`507fdaa6`（G2-01 profile metadata schema）。
- 范围：persona scope rename/delete 与 `profile_claims.owner_scope`、`relationship_events.persona_scope` 的 SQLite Immediate transaction 一致性。
- 不在范围内：prompt、memory、Laya/DecisionPort、TUI、向量库及新迁移。
- 允许文件：`crates/yunxi-core/src/state/conversation_db/sessions.rs`、`crates/yunxi-core/src/state/tests/sessions.rs`、本证据文档；`profile.rs` 未需修改。

## 实现要点

`rename_persona_scope` 现在在同一 `TransactionBehavior::Immediate` 中：

1. 拒绝目标 scope 已存在的 session、current/repl pointer、affection state、profile claim 或 relationship event。
2. 同时迁移 `platform_session_bindings`、`sessions`、current/repl 两类 `app_state` pointer、affection state、profile claims 和 relationship events。
3. 任一目标冲突在写入前返回错误，SQLite transaction 回滚，旧 scope 数据保持不变。

`delete_persona_scope` 在同一 Immediate transaction 中删除 scope session、current/repl pointer、affection state、profile claims 和 relationship events；`owner_scope=''` 的 global profile 不受影响。数据库事务提交后仍沿用既有 session artifact 清理边界；该边界不属于本切片的数据库原子性承诺。

## 测试证据

测试环境为 WSL Ubuntu-24.04，源码复制到 ext4 临时目录 `/tmp/yunxi-g2-scope-src`，编译产物使用 `/tmp/yunxi-g2-target`，避免污染仓库工作树。

| 命令 | 结果 |
| --- | --- |
| `cargo test -p yunxi-core --lib state::tests::sessions --locked -- --test-threads=1` | 16 passed, 0 failed |
| `cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1` | 3 passed, 0 failed |
| `cargo test -p yunxi-core --lib state::migrations --locked -- --test-threads=1` | 18 passed, 0 failed |
| `cargo fmt --all -- --check` | passed |
| `git diff --check` | passed |
| `python test_scripts/arch_dep_check.py` | passed（输出编码在 Windows 控制台乱码，但退出码为 0） |
| `python testkit/privacy/g0_scan.py --repo .` | passed；credential/private path/private key 均为 0 |

新增/扩展的 sessions tests 覆盖：rename 同时迁移 current/repl pointer、profile claim 与 relationship event；目标 profile/event 冲突拒绝并验证旧数据未被写入；delete 清理 scoped metadata 且保留 global profile。

## 未验证与残余风险

- 当前只验证 WSL Ubuntu-24.04；原生 Arch、macOS 及 Windows SQLite 构建未在本切片复跑。
- 未进行进程崩溃注入、磁盘满或跨进程锁竞争矩阵；这些属于后续 G9 硬化。
- scope rename/delete 的数据库写入是单个 Immediate transaction；提交后的文件/artifact 清理仍由既有外层流程负责，若外层清理失败需由后续恢复/重试设计覆盖。
- 本切片没有引入向量化、自动总结或决策模型调用，profile 仍保持非向量化存储。

## 可复核运行记录

本切片已绑定提交 `f1eaf65ff22e263935d83b4ba0de4f246b9e78db`，并由 worker 在
WSL Ubuntu-24.04 的 ext4 临时 target 上完成定向复跑。临时源码、target 与
日志已清理；以下稳定摘要是唯一保留的测试证据。

```text
run_id: g2-02-20261002-ext4-01
stage/task: G2-02 persona scope lifecycle
commit_sha: f1eaf65ff22e263935d83b4ba0de4f246b9e78db
recorded_at_utc: 2026-10-02 06:03:08 UTC (record update; worker execution window not retained)
evidence_owner: /root/g1_exit_audit (worker; requested_model=gpt-6.1-sol; actual_model=未暴露)
environment: WSL Ubuntu-24.04; source/target on ext4 (/tmp/yunxi-g2-target)
commands:
  cargo test -p yunxi-core --lib state::tests::sessions --locked -- --test-threads=1
  cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1
  cargo test -p yunxi-core --lib state::migrations --locked -- --test-threads=1
  cargo fmt --all -- --check
  git diff --check
  python test_scripts/arch_dep_check.py
  python testkit/privacy/g0_scan.py --repo .
exit_codes: 0, 0, 0, 0, 0, 0, 0
stable_counts: sessions 16 passed/0 failed; profile 3 passed/0 failed; migrations 18 passed/0 failed; credential/path/key/private-key findings 0
failure_reason: none
unverified: Arch/macOS/Windows native build; crash/disk-full/lock contention; post-commit artifact cleanup failure
cleanup: /tmp/yunxi-g2-target removed; no cargo/rustc remained; no raw DB/path/secret artifact committed
```

The exact worker execution minute was not retained; `recorded_at_utc` is the
time this evidence record was corrected for audit. This is a worker-owned
stable summary, not a retained raw log.
