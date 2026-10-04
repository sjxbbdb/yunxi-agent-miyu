# G5-05-06 recall/rerank deterministic 回归证据

- 任务：G5-05-06
- 记录时间：2026-10-04（Asia/Singapore）
- 阶段：G5-05-DESIGN；本批无生产代码变更

## 验证范围

本批复用现有 `MemoryStore` 的 recall/rerank 生产实现和 test-only fixture，不引入
第二套排序器、模型或 provider。覆盖四个稳定性不变量：

- 过度召回的 fact 会因 recall fatigue 下沉，较新的 fact 可以被召回；
- `accepted` truth status 优先于 `uncertain`；
- 高 confidence fact 优先于 shaky fact；
- 少量召回不会错误压低事实排序。

同时复跑 store 级生命周期/召回回归，确认 keyword/semantic recall、tombstone、重启/进程
恢复、principal 过滤、reset 与 profile prompt-only 边界没有被 G5-05 评测改动破坏。

## WSL 验证

环境：WSL `Ubuntu-24.04`，一次性 target `/tmp/g5-05-recall-target`，测试后已删除。

```text
cargo test -p yunxi-core --lib memory::tests::ranking --locked -- \
  --nocapture --test-threads=1

running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 776 filtered out

cargo test -p yunxi-core --lib memory::tests::store --locked -- \
  --nocapture --test-threads=1

running 19 tests
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 761 filtered out
```

前序 G5-05-03 证据已记录 KB 30 passed/2 ignored、dedup 6 passed 和 semantic
5 passed/1 ignored；本批不把 ONNX Runtime 未安装时的安全 skip 误报成语义模型通过。

## 结论与边界

recall/rerank 的确定性排序和生命周期边界有当前 HEAD 的可复现证据。该证据不代表
真实 Laya/provider 已接入，也不提供 RAM 峰值、Arch/macOS 或生产 shadow consumer 的
质量结论；这些边界仍需在 G5-05 退出审计中单独列出。
