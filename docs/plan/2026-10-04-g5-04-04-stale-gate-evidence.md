# G5-04-04 stale-response gate evidence

## Result

G5-04-04 的无模型 stale-response harness 已完成并推送，代码提交为
`0e820f54736e4d0ecd1af36180338d95552c1392`。

本批只增加 memory admission 的同步响应门禁。它不加载 Laya，不创建异步
runtime，不接入真实 provider，也不改变生产写入语义。

## 实现边界

- `admit_admission_shadow_response` 只比较已校验 token 与当前
  `AdmissionShadowContext`。
- task、scope、input fingerprint、diary id、batch database id、batch
  generation、consumer epoch、shadow mode 任一不一致，都返回
  `DecisionError::StaleFingerprint`。
- 相等时只返回 `Ok(())`；函数没有 callback、数据库写入、organizer、scheduler
  或权限副作用。
- 调用方必须在应用响应前调用门禁；stale 响应只能被丢弃或记录，不能获得写权限。
- 测试用本地 `Cell` 计数证明当前响应只应用一次，八类上下文漂移均不会再次应用，
  primary envelope 保持不变。

## 验证证据

### WSL Ubuntu-24.04 / ext4 disposable checkout

```text
cargo test -p yunxi-core --lib memory::tests::admission --locked -- --nocapture --test-threads=1
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 752 filtered out
```

测试从 `/tmp/g5-04-04-src` 构建，target 使用 `/tmp/g5-04-04-target`；测试后两个
目录已删除，未留下 cargo/rustc/yunxi/miyu 进程。

### Windows 静态门禁

- `cargo fmt --all -- --check`：通过
- `cargo metadata --no-deps --format-version 1 --locked`：通过
- `python test_scripts/arch_dep_check.py`：通过
- `python testkit/privacy/g0_scan.py --self-test`：通过
- `python testkit/privacy/g0_scan.py --repo .`：通过，credential/private path 均为 0
- `git diff --check`：通过

### 推送

远端 `origin/codex/yunxi-product-rename` 已指向
`0e820f54736e4d0ecd1af36180338d95552c1392`。

## 非目标与残余风险

本批不宣称真实异步 provider 的网络迟到响应已经接入，也不宣称 Laya 权重、真实
memory consumer、KB consumer 或 runtime scheduler 已接入。下一阶段仍须先设计并
验证有界的 consumer-facing harness，再由独立施工单决定是否扩大到真实异步边界。
