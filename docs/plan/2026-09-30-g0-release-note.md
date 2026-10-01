# G0 基线审计与长线升级计划（待并入下一版本说明）

日期：2026-09-30  
阶段：YXM-G0 退出审计中
范围：`sjxbbdb/yunxi-agent-miyu` 当前产品基线

## 本轮记录

- 建立 `docs/plan/2026-09-30-yunxi-miyu-long-horizon.md`，将 YunXi 化升级拆成 G0–G9，并为每一阶段写入代码落点、依赖、不变量、测试证据和退出条件。
- 新增 `docs/plan/2026-09-30-g0-architecture-audit.md`，逐项固化 daemon/IPC lease、协议与 frame、事件回放、session/compact、memory/KB/transfer 隐私边界、MCP/Skills 生命周期、host guard、scheduler/job 和产品 goal 的真实入口、调用方向、所有者与后续门禁。
- 记录 Miyu 的 fish 接管、daemon、IPC、REPL/TUI、prompt/cache、memory、独立 KB、tools/hosts、MCP/Skills、迁移和 transfer registry 入口；后续实现必须复用这些入口，不能再造第二套运行时。
- 产品层改名后，修复 `legacy_config_dir` 在测试/迁移 root 与默认 config 路径不一致时的兼容回归；匹配真实的 `.yunxi` 与 `.miyu` 默认根后再映射 XDG namespace，保留两套历史路径兼容。
- 修复 G0-03 发现的会话边界回归：Web actor 重置改用现有 `MemoryStore::reset_session(session_id)`，不再把同一人格下其它会话的 pending events、evicted context、facts、episodes 或 embeddings 一并清掉；新增真实 actor 入口的 A/B 会话回归测试。
- 补充 G0-03 反向跨域护栏：`MemoryStore::reset_all()` 后，知识库源文件、`kb_meta.db` 元数据和 `semantic_index.db` 的 `semantic_chunks` 均保持不变；memory 与 knowledge base 的删除边界已有双向运行时证据。
- 固定 G0-03 的新旧 profile 路径事实边界：新布局 prompt 读取 `home/<user>/profile.md`；`state/profile.md` 保留为 legacy/transfer 兼容单元，不会反向覆盖当前属主档案；新增路径回归与 transfer wildcard 定向测试，1/1 + 1/1 通过。
- 修复 G0-09 daemon 黑盒在 WSL/CI 中的测试隔离：`testkit/daemon-orphan/run.py` 为每个临时 home 建立独立 `XDG_RUNTIME_DIR`，避免 IPC socket 目录缺失或误连真实用户 daemon；使用非 test binary 进行 parent/orphan 生命周期复跑，6/6 连续两次通过。
- 将决策模型主线显式固化为 `DecisionPort`/可选 Laya provider：先 deterministic、再 shadow；模型只能提供 salience/admission/rerank/intent/主动候选建议，不能执行工具、鉴权或写删数据。G0 只记录 seam 与验收契约，G5 才实现模型。
- 将长线 goal 重构为可执行 v3 合同：增加阶段边界、G0 退出审计、证据计数/失败原因/后续 owner 和“登记不等于通过”的规则；明确 G1～G9 的任务编号和 G5-00～G5-06 的可行性门、`DecisionPort`、deterministic provider、shadow Laya、逐消费者接入、故障评测与实际建议采纳/回滚。当前活动 goal 不结束、不重建，仓库计划文件作为可审计细化版本，活动 objective 文本未原地改写。
- 当前 goal 已暂停并准备替换为 v4：[`2026-10-01-goal-command-v4.md`](2026-10-01-goal-command-v4.md) 保留原 G0–G9 交付范围，同时把 Laya/DecisionPort 拆成 D0–D8 的可执行主线；重新启动后以 v4 为唯一执行合同。

## 验证状态

命令级入口—环境—结果矩阵见 [`2026-09-30-g0-test-matrix.md`](2026-09-30-g0-test-matrix.md)；本文件保留阶段摘要和残余风险。

- `cargo fmt --all -- --check`：通过。
- `cargo metadata --no-deps --format-version 1`：通过。
- `python test_scripts/arch_dep_check.py`：退出码 0。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过。
- WSL Ubuntu-24.04 `cargo test -p yunxi-base --lib --locked -- --test-threads=1`：396 passed、0 failed、6 ignored（含新旧 profile 路径事实边界回归）。
- WSL Ubuntu-24.04 `cargo test -p yunxi-core --lib --locked -- --test-threads=1`：642 passed、0 failed、8 ignored（改动前基线）。
- WSL Ubuntu-24.04 最新复跑：`cargo test -p yunxi-core --lib --locked -- --test-threads=1`：643 passed、0 failed、8 ignored；新增 Profile prompt-only 边界回归，确认 profile marker 只进入 prompt，不进入 facts、episodes 或 `memory_embeddings`。
- WSL Ubuntu-24.04 `cargo test -p yunxi-engine --lib --locked -- --test-threads=1`：628 passed、0 failed、13 ignored（历史基线）。
- WSL Ubuntu-24.04 上一轮复跑（归档资源上限、tier 矩阵和 registry 子路径边界之后）：`cargo test -p yunxi-engine --lib --locked -- --test-threads=1` 为 643 passed、0 failed、13 ignored；未设置 `YUNXI_LANG=zh` 时仅复现既有 locale 选择导致的中文显示名断言失败，设置后通过。
- WSL Ubuntu-24.04 本轮覆盖感知导入与 rename 错误分类复跑：`YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1` 为 651 passed、0 failed、13 ignored；其中 transfer 定向测试为 37/37，包含 stale Core 移动后 marker 失败恢复、仅 EXDEV 允许 copy fallback、旧新 persona/home wildcard、生产用量账本路径覆盖和成员路径缺口护栏。
- WSL Ubuntu-24.04 最新复跑：`YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1` 为 658 passed、0 failed、13 ignored；新增 MCP 启动失败隔离回归，确认坏服务器不会阻塞健康服务器注册且健康 listing 缓存仍命中；KB 双向删除隔离与 embedding 进度损坏回归继续通过。
- WSL Ubuntu-24.04 `cargo test -p yunxi-hosts --lib --locked -- --test-threads=1`：920 passed、0 failed、10 ignored；新增 `resetting_a_conversation_clears_only_the_named_session_memory`，证明 actor 重置不会误伤同人格的其它会话。
- WSL Ubuntu-24.04 profile 路径定向测试：`cargo test -p yunxi-base --lib config::tests::paths::home_layout_prompt_ignores_legacy_state_profile --locked -- --exact --test-threads=1` 与 `cargo test -p yunxi-engine transfer::tests::home_and_persona_layout_wildcards_resolve_to_their_units --locked -- --exact --test-threads=1` 均为 1/1；只固定事实边界，不改变产品迁移行为。
- WSL Ubuntu-24.04 `cargo test -p yunxi --lib --locked -- --test-threads=1`：504 passed、0 failed、4 ignored。
- WSL Ubuntu-24.04 全工作区 `YUNXI_LANG=zh cargo test --workspace --locked -- --test-threads=1`：退出码 0；root 504/0/4、base 396/0/6、core 643/0/8、engine 658/0/13、hosts 920/0/10，所有 doctest 通过。未固定 `YUNXI_LANG` 的旧复跑会触发英文/中文 golden 与可读名称断言。
- WSL Ubuntu-24.04 `cargo check --workspace --all-targets --locked`：通过；`cargo fmt --all -- --check`、`cargo metadata --no-deps --format-version 1`、`git diff --check`、`python test_scripts/arch_dep_check.py`：均通过。
- WSL Ubuntu-24.04 fish 黑盒基线：`python3 testkit/fish-accept-line/run.py` 17/17 通过；真实 fish PTY `pty_run.py` 通过，未出现通配符报错且 YunXi 成功接管。为兼容 fish 3.7，hook 的原文分词改用 `commandline --input=... --current-process --tokenize`，并修正 testkit 对 `crates/yunxi-base/src/shell/fish.rs` 的路径引用。
- daemon reload 定向黑盒：`cargo test --test daemon_reload --locked -- --test-threads=1` 2/2 通过。
- WSL Ubuntu-24.04 IPC 定向测试：`cargo test -p yunxi-core ipc --lib --locked -- --test-threads=1` 33/33 通过，覆盖 lease、frame、协议版本、半帧、超限、断连和同 home 单例。
- WSL Ubuntu-24.04 终端组合黑盒 `testkit/g0-terminal-combo/run.py`：同一隔离 home/daemon/stub 先走真实 fish PTY + `fish-init`/accept-line，再走真实 REPL PTY；两条 IPC 客户端都在 `turns.tool_flow` 中留下 `run_command` marker，fish 无 wildcard error，报告 `passed=true`，已重复运行两次通过。
- WSL Ubuntu-24.04 pyte venv 的 TUI PTY 黑盒 `testkit/tui/config_forms.py` 在隔离 home 下复跑为 16/16；fixture 显式提供 `custom_models`，并在选择 `stub-model` 前定位到 `Stub` 供应商，覆盖主菜单、全局设置、编辑模型、新增模型和保存退出路径。该证据仍只代表表单闭环，不能用它替代整个终端闭环。
- 隐私扫描复跑：1861 个 Git 跟踪文本文件；个人绝对路径与上游本机路径已替换为 `<user>`、`/home/tester`、仓库相对路径或运行时环境变量；`personal_path=0`、`private_key=0`、`credential_shape=0`。完整迁移/删除路径审计仍未关闭。
- G0-09 工具执行黑盒：`testkit/repl-smoke/run.py` 现已自带 `STUB_TOOL=1` 和 `printf G0_09_TOOL_OK`，对中英文占位符统一判定，并从真实 `turns.tool_flow` 校验 `run_command` 输出。隔离运行报告 `placeholder_on_paste=true`、`reply_seen=true`、`footer_speed=78 tok/s`、`placeholder_on_recall=true`、`raw_text_on_recall=false`、`repl_alive=true`、`tool_flow_marker=true`、`passed=true`；该探针自身闭环通过，但仍不能替代 fish/daemon/REPL 组合终端闭环。
- transfer 定向测试：`cargo test -p yunxi-engine transfer --locked -- --test-threads=1` 37/37 通过；新增未知 manifest、size/hash、entry 集合、Windows 路径、非 regular tar、Never unit、symlink 导出、new home conversation、staged fixup、marker rollback、归档资源上限、coverage-aware stale Core 清理、legacy merge-only、输入归档保护、stale 清理失败恢复、rename 错误分类、tier 矩阵、旧新 persona/home wildcard、生产 `state/usage.db` SQLite 快照/导入回归、file/SQLite 子路径边界和成员 Persona/KB 路径缺口护栏测试。registry 当前 61 个 unit（Core 44、Heavy 1、Platform 2、Never 14）的静态分类已逐项核对，并覆盖生产 usage DB 与 legacy 用量输入。成员 `home/<user>/kb` 与 `home/<user>/personas` 当前仍未纳入导出策略，测试只负责显式暴露缺口；剩余风险还包括 import 安装/回滚/stale/marker 的父目录竞态，以及 export 输出/source 检查与实际使用之间的路径竞态。
- daemon orphan 黑盒：先用 `cargo build --locked` 构建非 test binary，再在仓库根运行 `PYTHONDONTWRITEBYTECODE=1 python3 testkit/daemon-orphan/run.py --binary target/debug/yunxi`，6/6 连续两次通过；覆盖直接 daemon 随启动者 SIGKILL/正常退出而退出、detached daemon 跨启动命令存活及显式 stop。testkit 构建产物按设计拒绝自启动，因此该黑盒使用非 test binary；权限、延迟、断连和其它平台仍未覆盖。
- daemon orphan 构建边界已复核：若在 `cargo test --workspace` 之后直接复用 `target/debug/yunxi`，其 `testkit` feature 会把自重启 executable 指向不存在的占位路径，造成 detached 分支假失败；M-14 固定先执行独立 `cargo build --locked`，不得把 testkit 产物当作生产 daemon 黑盒输入。
- MCP/Skills 生命周期黑盒：在独立 `cargo build --locked` 后运行 `PYTHONDONTWRITEBYTECODE=1 python3 testkit/mcp-persistent/run.py target/debug/yunxi`，5/5 通过；确认同 session 复用、跨 session 隔离、system prompt instructions、session 删除回收和 daemon stop 无孤儿。MCP server 启动失败、断连、超时、非法 request-shape 与权限组合仍未覆盖。
- MCP 非法 JSON-RPC 响应：新增 `protocol_tests` 的 2 个分类回归与 `malformed_mcp_response_fails_the_matching_call_immediately` 运行时回归（2/2 + 1/1）；缺失 `jsonrpc`、非数值 id、畸形 `error` 会被标记为协议错误，能关联请求 id 时立即结束对应调用，不再等到超时；该运行时测试调用现有 session cleanup 路径，但未独立证明 PID 退出。
- MCP response-shape 修复复跑（提交 `ae1a8e56`）：`cargo test -p yunxi-engine tools::mcp --lib --locked -- --test-threads=1` 为 28/28；新增覆盖 `method + result` 混合对象的协议分类和运行时快速失败，避免把响应误当作服务器请求而等到调用超时。测试完成后删除 WSL `/tmp/yunxi-g0-mcp-target` 临时构建目录。
- MCP 故障边界复跑（提交 `0e4f9d40`）：同一 MCP 定向命令为 32/32；新增非法 JSON 噪声、分段 flush、半写 EOF 和异常退出 PID/连接池回收证据。测试结束后已删除 WSL `/tmp/yunxi-g0-mcp-target` 临时构建目录。
- MCP 连续超时回收复跑（提交 `e17e06fb`）：同一 MCP 定向命令为 32/32；新增首次超时保留进程、第二次超时回收旧 PID/连接池、下一次调用新起进程并报告状态丢失的证据。测试结束后已删除 WSL `/tmp/yunxi-g0-mcp-target` 临时构建目录。
- MCP 断连重启提示复跑（提交 `65deb53d`）：同一 MCP 定向命令为 33/33；新增服务器在两次调用之间自行退出时，连接池先 sweep 再取 retired notice，首次重启即带“状态已丢失”提示，且新进程状态从 `count 1` 开始。测试结束后已删除 WSL `/tmp/yunxi-g0-mcp-target` 临时构建目录。
- MCP 启动失败隔离：新增 `a_failed_mcp_startup_does_not_hide_a_healthy_server`（1/1）；不存在的 MCP 可执行文件只使自身 listing 失败，健康服务器仍注册工具，重复 registry 构建复用健康 listing 缓存。
- 修复并固定 G0 故障注入：KB embedding 的 `embedding-reindex.json` 被截断或写入非法 JSON 时，不再静默回退为空进度；状态明确为 `failed`，遗留锁可清理，损坏文件保留为取证。`cargo test -p yunxi-engine tools::knowledge_base::dashboard::tests --locked -- --test-threads=1` 为 6/6。
- 权限位测试改用 WSL 原生 Linux 文件系统临时目录，不再把 `/mnt` DrvFs 的 0777 映射误当作生产语义；bundled script、registry fixture、TUI changed-prefix、renderer event、tool-summary 和回放编辑测试均已按当前 YunXi 产品输出修正或补强。


## 退出审计中的未决项

- G0 的入口与边界审计主体已完成，证据见 `2026-09-30-g0-architecture-audit.md`；fish 分流与 daemon reload 的黑盒子项已通过，但当前测试全绿仍不等于 G0 已退出。退出审计必须先核对最低合同，再决定哪些缺口移交后续阶段。
- 仍需完成更广的终端闭环记录和 transfer 单元逐项安全证据。IPC 子项已通过；TUI 表单黑盒 `testkit/tui/config_forms.py` 已为 16/16，终端组合黑盒已通过，但故障注入、跨平台验证和 transfer P0/P1 仍未完成。隐私门禁已纳入 `testkit/privacy/g0_scan.py` 并报告通过。
- transfer 审计发现安装阶段仍有目录句柄级竞态待平台化消除；manifest/hash/version、路径/类型安全、归档资源上限、coverage-aware stale Core 清理、new home fixup、导出 symlink 拒绝、marker 失败回滚和完整 tier 覆盖已有直接测试。
- 跨会话记忆重置的 actor 入口已修复并由 920 个 hosts 单测覆盖；Profile/KB/Persona 删除恢复矩阵、成员路径迁移策略和更广故障注入仍未完成。
- daemon/IPC 的 lease、frame、协议协商、事件回放和问题问答链；session/conversation/compact/evicted context；transfer 的导出/导入/迁移/隐私分类；default KB write-through；MCP/Skills 快照与断连回退；host capability/command/net guard；persona/profile 的重命名、删除和作用域迁移；调度/background job；产品 `goal` 持久化与 Codex active goal 的区分；以及 `docs/interfaces/subsystems.md` 的挂接契约已完成代码定位，但运行时门禁仍按 G0-05/G0-09 逐项执行。
- Arch Linux 实机和 macOS M-series 仍未在本机验证；必须保留为明确的环境缺口。
- G1 及之后的 CompanionContext、结构化 profile、记忆 admission、独立 KB/RAG、Laya、自动总结、终端自然语言层和 TUI 原生化均未开始。

## 可复现隐私门禁

在 WSL/Arch Linux 中运行：

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test`

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py`

本轮结果：扫描 1861 个已跟踪文本文件，`personal_path=0`、`private_key=0`、`credential_shape=0`；公开 `APP_KEY`/`APP_SEC` 归类为 1 个 `public_allowlist` 文件，2 个合成测试值归类为 `fixture_allowlist`。脚本只输出类别、计数和路径，不输出匹配内容。

## 发布约束

本记录不代表阶段完成，也不触发 G1。验证通过的独立切片按阶段号提交并推送；只有 G0 退出条件全部满足后才推进 G1。失败证据必须留在本计划中，不能用“可以启动”替代验收。
