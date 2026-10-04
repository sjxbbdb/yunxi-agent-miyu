# G5-05-03 memory/KB 隔离与重复召回评测证据

日期：2026-10-04  
阶段：G5-05-03  
代码变更：无（复用既有生产边界与测试）  
当前功能基线：`19dfc883`

## 结果

本批不新增生产逻辑，直接对当前 HEAD 的 memory/KB 隔离、重复收敛和 semantic fallback
做 WSL ext4 回归，避免为已有覆盖重复堆测试。

KB `tools::knowledge_base`：30 passed、2 ignored、0 failed，覆盖 namespace capability、
default/user provenance、删除/回滚、重索引 revision、memory reset 不触碰 KB、KB remove
不触碰 memory 及隐藏 namespace 向量保留。

memory `dedup`：6 passed、0 failed；两项需要 ONNX Runtime 的语义 dedup 被测试自身
安全跳过。memory `semantic`：5 passed、1 ignored、0 failed；两项需要 ONNX Runtime 的
语义路径安全跳过，keyword-only、disabled semantic、短期 episode 不进入 semantic
coverage 与缺 runtime fallback 均通过。

## 证据命令

```text
cargo test -p yunxi-engine --lib tools::knowledge_base --locked -- --nocapture --test-threads=1
cargo test -p yunxi-core --lib memory::tests::dedup --locked -- --nocapture --test-threads=1
cargo test -p yunxi-core --lib memory::tests::semantic --locked -- --nocapture --test-threads=1
```

WSL 使用 `/tmp/g5-05-03-src` 与 `/tmp/g5-05-03-target`，三条命令完成后已删除目录，
未留下 cargo/rustc/yunxi/miyu 进程。该回归不冒充 ONNX Runtime、真实 embedding provider
或 Arch 实机证据。

## 边界

已证明当前实现的 namespace/database/delete/recovery 隔离和 deterministic fallback；尚
未完成独立的 G5-05 指标 JSON、terminal intent clarification、proactive offline ranking、
RAM 采样或真实 Laya/provider 对比。因此 G5-05 仍保持 active，不进入 G5-06。

