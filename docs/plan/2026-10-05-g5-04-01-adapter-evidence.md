# G5-04-01 Memory Admission Adapter 证据

日期：2026-10-05  
提交：`14c627e6`  
阶段：G5-04-01（adapter-only，无生产 consumer/provider 接入）  
环境：Windows 门禁 + WSL Ubuntu-24.04 ext4 定向测试

## 已完成

- `crates/yunxi-core/src/memory/admission.rs` 新增 memory-local
  `AdmissionDecisionRequestBuilder`、`AdmissionDecisionEnvelope` 和
  `validate_admission_request`。
- request 固定为 `MemoryAdmission` / `Memory`，候选集合只有 `admit`、`reject`、`abstain`，
  capability 只有 `ChoiceOnly` + `Abstain`。
- payload 只包含六个固定字段：schema、candidate lifecycle、source class、sensitivity、
  force flag、规则版本；首版故意不携带 source digest，避免可链接性。
- deterministic `AdmissionDecision` 仍是唯一 primary；adapter 只映射为协议 envelope 并
  通过 `validate_result`，不调用 provider、不写 DB、不修改 lifecycle/embedding/KB。
- force metadata 与 primary reason 做一致性 guard，避免 builder 参数和 deterministic
  verdict 脱节。
- `memory/tests/admission.rs` 覆盖 Admit/Reject/Abstain、稳定 fingerprint、raw-free payload、
  固定候选、privacy/规则版本拒绝、未知 candidate/越界 confidence 和 force mismatch。

## 验收证据

| 环境 | 命令 | 结果 |
| --- | --- | --- |
| WSL Ubuntu-24.04 ext4 | `CARGO_TARGET_DIR=/tmp/g5-04-verifier-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo test -p yunxi-core --lib memory --locked -- --nocapture --test-threads=1` | exit 0；98 passed，0 failed，1 ignored，662 filtered；既有 ONNX 缺失测试按仓库逻辑 skip |
| Windows | `cargo fmt --all -- --check` | exit 0 |
| Windows | `cargo metadata --no-deps --format-version 1 --locked` | exit 0 |
| Windows | `python test_scripts/arch_dep_check.py` | exit 0 |
| Windows | `python testkit/privacy/g0_scan.py --self-test` | passed |
| Windows | `python testkit/privacy/g0_scan.py --repo .` | passed；credential/personal/private/unreadable 均为 0 |
| Windows | `git diff --check` | exit 0 |

WSL verifier 在测试后删除 `/tmp/g5-04-verifier-target`，并确认没有 cargo/rustc 进程。
仓库根 `target/` 未保留；本切片未下载模型权重、未增加 Python/Node/ONNX/HTTP runtime。

## 未完成与下一施工单

- 尚未在 `apply_organized_batch` 接入 observer；没有真实 consumer/provider、异步 task 或
  production switch。这是有意的边界，不应写成 memory admission 已经使用模型。
- 尚未实现异步迟到 response token；只在 G5-04 设计合同中固定待实现语义。
- `deadline_ms` 的有效预算/queue 由下一 observer integration slice 绑定到 `ShadowBudget`；
  当前 adapter 不会启动 provider，因此没有主路径延迟风险。
- 下一施工单 G5-04-02 只允许在 admission_by_id 计算完成后做 record-only best-effort observe；
  provider 结果不得进入 admitted/promoted、lifecycle、permission、embedding 或 KB。
