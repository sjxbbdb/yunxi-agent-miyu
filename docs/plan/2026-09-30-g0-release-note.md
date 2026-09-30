# G0 基线审计与长线升级计划（待并入下一版本说明）

日期：2026-09-30  
阶段：YXM-G0 进行中  
范围：`sjxbbdb/yunxi-agent-miyu` 当前产品基线

## 本轮记录

- 建立 `docs/plan/2026-09-30-yunxi-miyu-long-horizon.md`，将 YunXi 化升级拆成 G0–G9，并为每一阶段写入代码落点、依赖、不变量、测试证据和退出条件。
- 新增 `docs/plan/2026-09-30-g0-architecture-audit.md`，逐项固化 daemon/IPC lease、协议与 frame、事件回放、session/compact、memory/KB/transfer 隐私边界、MCP/Skills 生命周期、host guard、scheduler/job 和产品 goal 的真实入口、调用方向、所有者与后续门禁。
- 记录 Miyu 的 fish 接管、daemon、IPC、REPL/TUI、prompt/cache、memory、独立 KB、tools/hosts、MCP/Skills、迁移和 transfer registry 入口；后续实现必须复用这些入口，不能再造第二套运行时。
- 产品层改名后，修复 `legacy_config_dir` 在测试/迁移 root 与默认 config 路径不一致时的兼容回归；匹配真实的 `.yunxi` 与 `.miyu` 默认根后再映射 XDG namespace，保留两套历史路径兼容。

## 验证状态

- `cargo fmt --all -- --check`：通过。
- `cargo metadata --no-deps --format-version 1`：通过。
- `python test_scripts/arch_dep_check.py`：退出码 0。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过。
- WSL Ubuntu-24.04 `cargo test -p yunxi-base --lib --locked -- --test-threads=1`：395 passed、0 failed、6 ignored。
- WSL Ubuntu-24.04 `cargo test -p yunxi-core --lib --locked -- --test-threads=1`：642 passed、0 failed、8 ignored。
- WSL Ubuntu-24.04 `cargo test -p yunxi-engine --lib --locked -- --test-threads=1`：628 passed、0 failed、13 ignored。
- WSL Ubuntu-24.04 `cargo test -p yunxi-hosts --lib --locked -- --test-threads=1`：919 passed、0 failed、10 ignored。
- WSL Ubuntu-24.04 `cargo test -p yunxi --lib --locked -- --test-threads=1`：504 passed、0 failed、4 ignored。
- WSL Ubuntu-24.04 全工作区 `cargo test --workspace --no-fail-fast --locked -- --test-threads=1`：四个下层 crate 与根包的最终复跑结果均为 0 失败；此前汇总中根包的 1 个失败已由回放编辑断言的错误过滤条件复现并修正。Doctest 全部通过。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过；`cargo fmt --all -- --check`、`cargo metadata --no-deps --format-version 1`、`git diff --check`、`python test_scripts/arch_dep_check.py`：均通过。
- WSL Ubuntu-24.04 fish 黑盒基线：`python3 testkit/fish-accept-line/run.py` 17/17 通过；真实 fish PTY `pty_run.py` 通过，未出现通配符报错且 YunXi 成功接管。为兼容 fish 3.7，hook 的原文分词改用 `commandline --input=... --current-process --tokenize`，并修正 testkit 对 `crates/yunxi-base/src/shell/fish.rs` 的路径引用。
- daemon reload 定向黑盒：`cargo test --test daemon_reload --locked -- --test-threads=1` 2/2 通过。
- WSL Ubuntu-24.04 IPC 定向测试：`cargo test -p yunxi-core ipc --lib --locked -- --test-threads=1` 33/33 通过，覆盖 lease、frame、协议版本、半帧、超限、断连和同 home 单例。
- TUI PTY 黑盒 `testkit/tui/config_forms.py` 在隔离 home 下运行结果为 8/16；主菜单与部分表单断言通过，但编辑模型/新增模型导航和保存断言失败。该失败已记录为 G0-09 待修项，不能用 workspace 单测替代。
- 权限位测试改用 WSL 原生 Linux 文件系统临时目录，不再把 `/mnt` DrvFs 的 0777 映射误当作生产语义；bundled script、registry fixture、TUI changed-prefix、renderer event、tool-summary 和回放编辑测试均已按当前 YunXi 产品输出修正或补强。

## 未完成项

- G0 的入口与边界审计主体已完成，证据见 `2026-09-30-g0-architecture-audit.md`；fish 分流与 daemon reload 的黑盒子项已通过，但当前测试全绿仍不等于 G0 已退出。
- 仍需完成 TUI/工具执行的隔离黑盒闭环记录、transfer 单元隐私扫描索引和公开文件最终扫描。IPC 子项已通过；TUI 表单黑盒仍为 8/16。全仓扫描仍发现测试 fixture 中 `<upstream-home>` 等历史绝对路径，以及 `bilibili_live` 中待确认的第三方 `APP_SEC`；在完成 fixture 分类并确认公开性或改为运行时配置前，不得宣称发布面隐私门禁关闭。
- daemon/IPC 的 lease、frame、协议协商、事件回放和问题问答链；session/conversation/compact/evicted context；transfer 的导出/导入/迁移/隐私分类；default KB write-through；MCP/Skills 快照与断连回退；host capability/command/net guard；persona/profile 的重命名、删除和作用域迁移；调度/background job；产品 `goal` 持久化与 Codex active goal 的区分；以及 `docs/interfaces/subsystems.md` 的挂接契约已完成代码定位，但运行时门禁仍按 G0-05/G0-09 逐项执行。
- Arch Linux 实机和 macOS M-series 仍未在本机验证；必须保留为明确的环境缺口。
- G1 及之后的 CompanionContext、结构化 profile、记忆 admission、独立 KB/RAG、Laya、自动总结、终端自然语言层和 TUI 原生化均未开始。

## 发布约束

本记录不代表阶段完成，也不触发 G1。只有 G0 退出条件全部满足后，才按阶段号提交并推送；失败证据必须留在本计划中，不能用“可以启动”替代验收。
