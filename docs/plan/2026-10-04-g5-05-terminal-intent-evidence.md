# G5-05-04 terminal intent abstain evidence

日期：2026-10-04  
阶段：G5-05-04  
功能提交：`72d801df`

## 结果

在 `crates/yunxi-core/src/decision.rs` 的 `#[cfg(test)]` 区域增加
`terminal_intent_baseline_abstains_instead_of_executing_or_clarifying`。它构造一个脱敏的
`TerminalIntent`/`TerminalTurn` 请求，使用 deterministic port 验证：

- baseline 只返回 `Abstain` 和 `DeterministicBaseline`；
- 不执行命令、不创建澄清副作用、不调用 provider；
- request task、fingerprint、deadline 和 result 校验保持一致。

## 验证

WSL Ubuntu-24.04 ext4 disposable checkout：

```text
cargo test -p yunxi-core --lib decision::tests --locked -- --nocapture --test-threads=1
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 769 filtered out
```

Windows fmt、locked metadata、架构依赖、隐私扫描和 diff check 均通过；临时
`/tmp/g5-05-04-src`、`/tmp/g5-05-04-target` 与构建进程已清理。

## 边界

该测试证明的是 DecisionPort deterministic baseline 的 terminal-intent fail-closed 语义，
不是自然语言终端解析器、fish hook、权限执行或澄清 UI 的实现。真正 intent consumer、
confusion matrix、RAM/p50/p95 和 provider 对比仍属于 G5-05 后续切片。

