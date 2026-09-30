# G0 基线审计与长线升级计划（待并入下一版本说明）

日期：2026-09-30  
阶段：YXM-G0 进行中  
范围：`sjxbbdb/yunxi-agent-miyu` 当前产品基线

## 本轮记录

- 建立 `docs/plan/2026-09-30-yunxi-miyu-long-horizon.md`，将 YunXi 化升级拆成 G0–G9，并为每一阶段写入代码落点、依赖、不变量、测试证据和退出条件。
- 记录 Miyu 的 fish 接管、daemon、IPC、REPL/TUI、prompt/cache、memory、独立 KB、tools/hosts、MCP/Skills、迁移和 transfer registry 入口；后续实现必须复用这些入口，不能再造第二套运行时。
- 产品层改名后，修复 `legacy_config_dir` 在测试/迁移 root 与默认 config 路径不一致时的兼容回归；匹配真实的 `.yunxi` 与 `.miyu` 默认根后再映射 XDG namespace，保留两套历史路径兼容。

## 验证状态

- `cargo fmt --all -- --check`：通过。
- `cargo metadata --no-deps --format-version 1`：通过。
- `python test_scripts/arch_dep_check.py`：退出码 0。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过。
- WSL Ubuntu-24.04 `cargo test -p yunxi-base --lib --locked -- --test-threads=1`：394 passed、0 failed、6 ignored。
- WSL Ubuntu-24.04 全工作区 `cargo test --workspace --no-fail-fast --locked -- --test-threads=1`：未全绿，结果已返回；根包 508 tests 中 1 个失败，`yunxi-engine` 625 tests 中 3 个失败，`yunxi-hosts` 915 tests 中 4 个失败，其他 crate 与 doctest 通过。失败集中在产品改名后的 TUI changed-prefix fixture、WSL `/mnt` 下的权限位语义、bundled script 清单漂移、tool registry shape fixture 以及 renderer event/golden/tool-summary 基线漂移。
- 其中权限位失败已确认是 WSL DrvFs 测试目录的环境语义，不能据此修改生产逻辑；其余失败必须在 G0 内逐项分类为应修复的基线漂移或明确的环境缺口，不能用“命令启动”替代验收。

## 未完成项

- G0 还需要逐项处理上述 workspace 失败、补齐路径兼容的负向测试、执行完整 diff/privacy 检查并核对计划中的真实文件入口。
- Arch Linux 实机和 macOS M-series 仍未在本机验证；必须保留为明确的环境缺口。
- G1 及之后的 CompanionContext、结构化 profile、记忆 admission、独立 KB/RAG、Laya、自动总结、终端自然语言层和 TUI 原生化均未开始。

## 发布约束

本记录不代表阶段完成，也不触发 G1。只有 G0 退出条件全部满足后，才按阶段号提交并推送；失败证据必须留在本计划中，不能用“可以启动”替代验收。
