# G5-05-02 DecisionPort fallback evaluation evidence

日期：2026-10-04  
阶段：G5-05-02  
功能提交：`19dfc883`  
前置：G5-05-01 admission matrix `29262756`

## 结果

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域增加
`fallback_matrix_preserves_primary_and_fails_closed`。它只保存脱敏的 case id、match kind、
primary digest 和 provider call count，覆盖：invalid result、provider 超 deadline、
provider unavailable、closed transport 映射为 unavailable，以及 privacy rejection。

每个 observation 的 `primary_digest` 都必须与 deterministic baseline 相等，baseline 前后
结构相等；privacy rejection 在 request gate 处失败且 provider call 为 0；closed transport
只用 test-only `sync_channel` 证明断连分类，不新增生产 transport。

## 验证

WSL Ubuntu-24.04 ext4 disposable checkout：

```text
cargo test -p yunxi-core --lib decision_shadow --locked -- --nocapture --test-threads=1
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 747 filtered out
```

Windows `cargo fmt --all -- --check`、locked metadata、架构依赖检查、隐私仓库扫描与
`git diff --check` 均通过；credential/private path 为 0。`/tmp/g5-05-02-src`、
`/tmp/g5-05-02-target` 和测试进程均已清理。

## 边界

本切片不创建真实 provider、Laya 权重、异步 runtime、生产 metrics ledger、生产 consumer
或权限/工具执行路径；它只把已有 fallback 语义汇总成可回放测试证据。G5-05 仍需补齐
recall/rerank、terminal intent、memory/KB 隔离与完整指标摘要后才能退出。

