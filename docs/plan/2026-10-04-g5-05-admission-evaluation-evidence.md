# G5-05-01 deterministic admission evaluation evidence

日期：2026-10-04  
阶段：G5-05-01  
功能提交：`1af476ca`  
设计合同：`2026-10-04-g5-05-evaluation-design.md`

## 结果

在 `crates/yunxi-core/src/memory/tests/admission.rs` 增加了一个 test-only、可重复回放的
admission evaluation matrix。六个脱敏样本覆盖：项目约束保留、闲聊不长期收录、password
占位符拒绝、secret 拒绝、稳定偏好保留、force-long-term 仍拒绝敏感内容。每个 case 同时
断言 `AdmissionVerdict` 与 `AdmissionClass`，第二次运行的 `(case_id, verdict, class)`
序列必须完全相同；随后校验 admission envelope，确认 primary 在校验前后不变。

## 验证

WSL Ubuntu-24.04 ext4 disposable checkout：

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

Windows 静态门禁：

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

测试使用 `/tmp/g5-05-src` 与 `/tmp/g5-05-target`，运行后已删除；未留下 cargo/rustc/
yunxi/miyu 进程、WSL target、用户 home、真实 secret 或生成索引。

## 边界

本切片只证明 deterministic admission 的固定样本可重复、敏感输入不被 force 提升、
primary envelope 不变。它还没有完成 G5-05 的 recall/rerank、terminal intent 澄清、
memory/KB 交叉删除、重复 memory no-op、provider invalid/timeout/disconnect fallback、
confusion matrix、p50/p95/RAM 统计或真实 Laya/provider 评测；不改变生产写入与 shadow
observer。下一施工单继续补齐这些 test-only 评测，不推进 G5-06。

