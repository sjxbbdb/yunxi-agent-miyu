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
- WSL Ubuntu-24.04 上一轮复跑（归档资源上限、tier 矩阵和 registry 子路径边界之后）：`cargo test -p yunxi-engine --lib --locked -- --test-threads=1` 为 643 passed、0 failed、13 ignored；未设置 `YUNXI_LANG=zh` 时仅复现既有 locale 选择导致的中文显示名断言失败，设置后通过。
- WSL Ubuntu-24.04 本轮覆盖感知导入复跑：`YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1` 为 648 passed、0 failed、13 ignored；其中 transfer 定向测试为 34/34，包含 stale Core 移动后 marker 失败恢复。
- WSL Ubuntu-24.04 `cargo test -p yunxi-hosts --lib --locked -- --test-threads=1`：919 passed、0 failed、10 ignored。
- WSL Ubuntu-24.04 `cargo test -p yunxi --lib --locked -- --test-threads=1`：504 passed、0 failed、4 ignored。
- WSL Ubuntu-24.04 全工作区 `cargo test --workspace --no-fail-fast --locked -- --test-threads=1`：四个下层 crate 与根包的最终复跑结果均为 0 失败；此前汇总中根包的 1 个失败已由回放编辑断言的错误过滤条件复现并修正。Doctest 全部通过。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过；`cargo fmt --all -- --check`、`cargo metadata --no-deps --format-version 1`、`git diff --check`、`python test_scripts/arch_dep_check.py`：均通过。
- WSL Ubuntu-24.04 fish 黑盒基线：`python3 testkit/fish-accept-line/run.py` 17/17 通过；真实 fish PTY `pty_run.py` 通过，未出现通配符报错且 YunXi 成功接管。为兼容 fish 3.7，hook 的原文分词改用 `commandline --input=... --current-process --tokenize`，并修正 testkit 对 `crates/yunxi-base/src/shell/fish.rs` 的路径引用。
- daemon reload 定向黑盒：`cargo test --test daemon_reload --locked -- --test-threads=1` 2/2 通过。
- WSL Ubuntu-24.04 IPC 定向测试：`cargo test -p yunxi-core ipc --lib --locked -- --test-threads=1` 33/33 通过，覆盖 lease、frame、协议版本、半帧、超限、断连和同 home 单例。
- WSL Ubuntu-24.04 终端组合黑盒 `testkit/g0-terminal-combo/run.py`：同一隔离 home/daemon/stub 先走真实 fish PTY + `fish-init`/accept-line，再走真实 REPL PTY；两条 IPC 客户端都在 `turns.tool_flow` 中留下 `run_command` marker，fish 无 wildcard error，报告 `passed=true`，已重复运行两次通过。
- WSL Ubuntu-24.04 pyte venv 的 TUI PTY 黑盒 `testkit/tui/config_forms.py` 在隔离 home 下复跑为 16/16；fixture 显式提供 `custom_models`，并在选择 `stub-model` 前定位到 `Stub` 供应商，覆盖主菜单、全局设置、编辑模型、新增模型和保存退出路径。该证据仍只代表表单闭环，不能用它替代整个终端闭环。
- 隐私扫描复跑：仓库中的个人绝对路径与上游本机路径已替换为 `<user>`、`/home/tester`、仓库相对路径或运行时环境变量；公开 `docs/`、发布说明与生产注释未发现真实个人路径。`bilibili_live` 的 `APP_KEY`/`APP_SEC` 已核验与公开的 `Rsplwe/bili-live-hime` `src/lib/app-sign.ts` 一致，属于第三方客户端公开签名常量，不是用户凭据，已列入 allowlist。完整迁移/删除路径审计仍未关闭。
- G0-09 工具执行黑盒：`testkit/repl-smoke/run.py` 现已自带 `STUB_TOOL=1` 和 `printf G0_09_TOOL_OK`，对中英文占位符统一判定，并从真实 `turns.tool_flow` 校验 `run_command` 输出。隔离运行报告 `placeholder_on_paste=true`、`reply_seen=true`、`footer_speed=78 tok/s`、`placeholder_on_recall=true`、`raw_text_on_recall=false`、`repl_alive=true`、`tool_flow_marker=true`、`passed=true`；该探针自身闭环通过，但仍不能替代 fish/daemon/REPL 组合终端闭环。
- transfer 定向测试：`cargo test -p yunxi-engine transfer --locked -- --test-threads=1` 34/34 通过；新增未知 manifest、size/hash、entry 集合、Windows 路径、非 regular tar、Never unit、symlink 导出、new home conversation、staged fixup、marker rollback、归档资源上限、coverage-aware stale Core 清理、legacy merge-only、输入归档保护、stale 清理失败恢复、tier 矩阵和 file/SQLite 子路径边界测试。registry 当前 59 个 unit（Core 42、Heavy 1、Platform 2、Never 14）的静态分类已逐项核对。剩余风险是 install 父目录检查与后续操作之间的本地竞态消除。
- 权限位测试改用 WSL 原生 Linux 文件系统临时目录，不再把 `/mnt` DrvFs 的 0777 映射误当作生产语义；bundled script、registry fixture、TUI changed-prefix、renderer event、tool-summary 和回放编辑测试均已按当前 YunXi 产品输出修正或补强。


## 未完成项

- G0 的入口与边界审计主体已完成，证据见 `2026-09-30-g0-architecture-audit.md`；fish 分流与 daemon reload 的黑盒子项已通过，但当前测试全绿仍不等于 G0 已退出。
- 仍需完成更广的终端闭环记录和 transfer 单元逐项安全证据。IPC 子项已通过；TUI 表单黑盒 `testkit/tui/config_forms.py` 已为 16/16，终端组合黑盒已通过，但故障注入、跨平台验证和 transfer P0/P1 仍未完成。隐私门禁已纳入 `testkit/privacy/g0_scan.py` 并报告通过。
- transfer 审计发现安装阶段仍有目录句柄级竞态待平台化消除；manifest/hash/version、路径/类型安全、归档资源上限、coverage-aware stale Core 清理、new home fixup、导出 symlink 拒绝、marker 失败回滚和完整 tier 覆盖已有直接测试。
- daemon/IPC 的 lease、frame、协议协商、事件回放和问题问答链；session/conversation/compact/evicted context；transfer 的导出/导入/迁移/隐私分类；default KB write-through；MCP/Skills 快照与断连回退；host capability/command/net guard；persona/profile 的重命名、删除和作用域迁移；调度/background job；产品 `goal` 持久化与 Codex active goal 的区分；以及 `docs/interfaces/subsystems.md` 的挂接契约已完成代码定位，但运行时门禁仍按 G0-05/G0-09 逐项执行。
- Arch Linux 实机和 macOS M-series 仍未在本机验证；必须保留为明确的环境缺口。
- G1 及之后的 CompanionContext、结构化 profile、记忆 admission、独立 KB/RAG、Laya、自动总结、终端自然语言层和 TUI 原生化均未开始。

## 可复现隐私门禁

在 WSL/Arch Linux 中运行：

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test`

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py`

本轮结果：扫描 1852 个已跟踪文本文件，`personal_path=0`、`private_key=0`、`credential_shape=0`；公开 `APP_KEY`/`APP_SEC` 归类为 1 个 `public_allowlist` 文件，2 个合成测试值归类为 `fixture_allowlist`。脚本只输出类别、计数和路径，不输出匹配内容。

## 发布约束

本记录不代表阶段完成，也不触发 G1。只有 G0 退出条件全部满足后，才按阶段号提交并推送；失败证据必须留在本计划中，不能用“可以启动”替代验收。
