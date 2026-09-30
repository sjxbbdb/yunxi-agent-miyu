# YunXi × Miyu 长线原生化升级计划

> 计划编号：YXM-G0
> 版本：2026-09-30
> 当前基线：仓库 `sjxbbdb/yunxi-agent-miyu`
> 上游参考：shorin/miyu

这不是一份“把功能加上去”的愿望清单，而是一份可以逐阶段执行、测试、回滚和验收的工程合同。每个阶段都有明确的代码边界、入口、依赖、测试证据和退出条件；当前阶段没有通过时，不进入下一阶段。

## 1. 固定产品背景

- `yunxi-agent-miyu` 是当前产品基线，fork 自 shorin 的 Miyu。
- `sjxbbdb/YunXi-Native` 是历史参考版本，不是本仓库的实现基线。
- Miyu 提供 Linux 原生底座：fish 接管、常驻 daemon、IPC、TUI、会话/缓存、终端/文件/进程/包管理工具、MCP 与 Skills。
- YunXi 提供人格、灵魂、用户画像、关系阶段、分层记忆、独立知识库、陪伴策略和后续决策层。
- 最终形态是一个活在 Linux 终端中的陪伴型通用系统级 Agent：人使用自然语言，YunXi 将意图翻译成受现有权限与工具边界约束的终端操作，并解释结果。

## 2. 永久不变量

1. 不建立第二个 shell router、daemon、prompt 拼装链、调度器或记忆数据库；优先复用 Miyu 入口。
2. 记忆库和知识库完全分离：目录、schema、embedding、检索入口、权限、迁移、删除和审计都独立。
3. user profile 不向量化；短期记忆只承接当前上下文和语气；长期记忆必须经过 candidate → commit/reject/expire，不能逐句默认写入。`raw_content`、`display_content`、`context_messages` 三分不可破坏，记忆/日志只读原始用户文本，工程附加信息走 sidecar。
4. profile、credentials、API key、秘密和敏感个人字段不得进入长期向量索引。
5. Laya 只能是可关闭的建议器，不能成为工具执行、权限、记忆写入/删除的最终权威；缺失、超时、低置信度和非法输出统一回退确定性规则。
6. prompt/cache 的 append-only、fossilization、顺序、工具字节稳定性和回放一致性不能破坏。
7. 数据本地优先、最小日志、可查看、可导出、可删除、可恢复；公开仓库不得出现令牌、个人档案、绝对本机路径和测试索引。
8. Rust 2021（当前 `Cargo.toml` 的真实值，不能把计划写成不存在的 2024）；模块小而专一；数据库迁移只能追加且必须命名；不能用一个巨型模块承载多个领域。
9. Linux 是产品主平台，但项目规则还要求保持 macOS M-series 兼容性；G0/G9 必须留下对应的编译/定向测试证据，不能静默把范围缩成 Linux-only。

## 3. 阶段任务与执行边界

### G0：基线冻结与架构审计（当前阶段）

**只读为主，不改变业务行为。**本阶段只允许修复审计发现的既有兼容性回归；不得借机开始 G1。

**G0 任务编号与交付物**：

- **G0-01 代码入口清单**：逐项记录 fish capture、daemon、IPC、REPL/TUI、prompt/cache、profile/persona、memory、KB、tools/hosts、MCP/Skills、migration、transfer、scheduler 的真实入口、所有者和调用方向；交付本文件入口表与调用图。
- **G0-02 重复运行时检查**：搜索 daemon、shell router、prompt assembler、scheduler、memory store 的第二实现；每项给出“复用/保留/删除/暂不处理”的结论，不做猜测性重构。
- **G0-03 数据边界清单**：列出 profile、conversation/state、short/long memory、KB、semantic index、credentials、cache 的目录、SQLite 表/索引、embedding 入口、迁移入口、transfer registry 分类和删除/恢复路径。
- **G0-04 提示与缓存契约**：记录 system prompt 组装顺序、append-only/fossilization、动态事实尾、工具字节稳定性、回放测试和敏感信息边界；确认新模块只能挂在现有 seam。
- **G0-05 测试矩阵**：把每个入口映射到定向单测、集成测试、testkit/黑盒脚本、WSL 实测和未来 Arch/macOS 证据；记录命令、环境、耗时、通过/失败和失败原因。
- **G0-06 基线修复记录**：只修复由产品改名暴露的旧路径兼容回归；补充正向、负向和自定义 root/config 组合测试，证明不会扩大路径匹配范围。
- **G0-07 计划清理**：移除公开文档中的个人绝对路径、虚构入口和未经验证的完成表述；更新 `docs/plan/2026-09-30-g0-release-note.md`，提交 G0 证据索引。
- **G0-08 扩展与权限入口**：把 Skills/MCP 的资源布局、persona/机器快照、注册顺序、断连回退和 request-shape 测试单独列出；把 host ports、principal、turn restriction、command/net guard 和 tool registry 列为权限真相源；不新增 YunXi approval 状态机。
- **G0-09 现有终端闭环基线**：记录 fish 普通语法、自然语言接管、daemon/IPC/TUI 和工具执行的最小黑盒路径，后续 G7 只补 YunXi 缺口，不把核心闭环推迟到最后。

- 盘点真实入口：fish capture、daemon、IPC、TUI、session/state、prompt、`persona_hint`、memory、KB、tool/host ports、MCP/Skills、embedding、cache、migration、测试脚本。
- 将每个入口映射到实际文件、trait、状态机和测试；画出依赖方向，标出可复用点和重复实现。
- 记录基线命令、耗时、失败原因和资源限制；形成可重复的测试清单。
- 输出本文件的“实现地图”与阶段追踪表，更新 `docs/plan/2026-09-30-g0-release-note.md`（下一版本说明的待合并记录）。

**退出条件**：没有未解释的第二运行时/重复入口；G1–G9 均有代码落点和测试入口；现有自然语言终端闭环已登记为可复现实验；`git diff --check` 通过；基线结果可复现。Arch/macOS 若尚无实机，只能标为未验证，不得冒充通过，也不自动阻塞 Linux 阶段开发；G9 发布门再决定是否阻塞。

#### G0 初始基线证据（2026-09-30，后续复跑结果）

- `git diff --check`：通过。
- `cargo fmt --all -- --check`：通过。
- `cargo metadata --no-deps --format-version 1`：通过；当前工作区成员为 `yunxi-base`、`yunxi-core`、`yunxi-engine`、`yunxi-hosts` 和根包 `yunxi`。
- `python test_scripts/arch_dep_check.py`：退出码 0；终端编码导致中文摘要显示异常，但脚本判定通过。
- WSL Ubuntu-24.04：`CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked` 通过，约 1 分 07 秒。
- WSL Ubuntu-24.04：`CARGO_BUILD_JOBS=1 cargo test -p yunxi-base --lib resource_path_remapping_includes_the_legacy_xdg_config_root --locked -- --test-threads=1` 通过；新增的自定义 root 负向路径测试也通过。
- Windows 原生 `cargo check --workspace --all-targets --locked`：失败，原因是基线包含 `std::os::unix`、Unix socket、Unix 权限和 `AsRawFd` 等 Linux 专属实现；这确认本项目的构建验收必须以 WSL/Arch Linux 为主，不能为 Windows 编译通过而削弱 Linux 路径。
- 受 `systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0` 保护的工作区分批测试最终为：根包 `yunxi` 504/0/4 ignored，`yunxi-base` 395/0/6 ignored，`yunxi-core` 642/0/8 ignored，`yunxi-engine` 628/0/13 ignored，`yunxi-hosts` 919/0/10 ignored；doctest 全部通过。
- 初次失败已逐项处理：TUI changed-prefix fixture 与回放编辑断言更新为当前 YunXi 输出；权限位测试改用 WSL 原生临时目录；bundled script 清单与 `douyin-dl` 描述同步；registry fixture 重生成；renderer event 与 tool-summary 断言改为检查本地化语义而非不稳定装饰符号。
- 这些修复只校准测试夹具、环境选择和既有输出契约，没有新增业务能力；`legacy_config_dir` 另补了自定义 root 名称不应选择旧 namespace 的负向回归测试。

#### G0 基线修复记录

产品层改名提交 `7698b643` 将 `legacy_config_dir` 泛化为从 `root_dir.file_name()` 推导命名空间；当测试/迁移构造的 `root_dir` 与真实默认 `config_dir` 不同名时，旧绝对身份路径无法识别，静默回退到旧路径。修复为在真实默认根 `~/.yunxi`/`~/.miyu` 中匹配 `config_dir` 后返回对应 XDG namespace，并保留迁移兼容。该修复不增加新能力，只恢复已有兼容契约；对应 `yunxi-base` 400 项测试已全绿。

#### G0 尚未完成的验证

- 全工作区测试已全绿；入口、调用方向、所有者、不变量和测试映射的逐项取证见 [`2026-09-30-g0-architecture-audit.md`](2026-09-30-g0-architecture-audit.md)。fish 分流静态判定 17/17、真实 fish PTY 接管和 daemon reload 2/2 已复现，但仍需完成其余隔离黑盒闭环、transfer 单元隐私索引和最终公开文件扫描；特别是 IPC lease/frame/协议/回放、session/compact/evicted context、KB write-through、MCP/Skills、host capability/guard、scheduler/background job、goal 持久化与 Codex active goal 的区分已完成定位，但尚未宣称全部运行时门禁通过。
- macOS M-series 只能在对应环境或 CI 上验证，当前本机没有该运行环境；需在 G0 状态中保留为未验证项。
- Arch Linux 实机尚未验证；WSL Ubuntu 已确认 `/usr/bin/fish`、Rust/Cargo 和 systemd user scope 可用。

#### G0 已定位的第一批集成接缝

- 人格提示：`crates/yunxi-core/src/persona_hint.rs`；已有 hints、distilled persona cache 和默认 YunXi prompt 回退。
- Prompt/cache：`crates/yunxi-engine/src/agent/prompt.rs`；已有 append-only block、memory preamble、host environment、persona system prompt 和字节稳定性测试。
- 记忆底座：`crates/yunxi-core/src/memory/mod.rs`；已有 short/long scope、`MemoryAccess`、`MemoryOrigin`、`MemoryHit`、organizer、SQLite/config/embedding 接缝。
- 跨 crate 纯数据：`crates/yunxi-base/src/memory_types.rs`；已有 `EvictedTurn`，用于隔离 state 与 memory，避免循环依赖。
- 下一步 G0 只需补齐 fish/daemon/IPC/TUI、KB、host/tool、migration 的入口表和重复运行时检查；在此之前不创建 `CompanionContext` 或 Laya 代码。

#### G0 入口表（首版）

逐项的调用方向、所有者、数据不变量、恢复边界和对应测试文件已独立固化在 [`2026-09-30-g0-architecture-audit.md`](2026-09-30-g0-architecture-audit.md)。本节保留产品计划中的导航表，避免把审计证据和未来设计混在一起。

| 领域 | 当前真实入口 | 约束/复用结论 |
| --- | --- | --- |
| fish 接管 | `crates/yunxi-base/src/shell/fish.rs`、`src/cli/shell_bridge.rs` | hook 已区分 classify/intercept、普通 fish 语法和 command-not-found；G7 只能扩展现有路径。 |
| daemon 单例 | `crates/yunxi-hosts/src/daemon.rs`、`src/cli/daemon_cmds.rs`、`crates/yunxi-core/src/ipc/` | home singleton、lease、socket、frame、版本协商、启动/停止/重载/残留清理已存在；不得再造常驻进程。 |
| runtime/IPC | `crates/yunxi-hosts/src/runtime/`、`crates/yunxi-core/src/ipc/` | 事件、问题/取消、状态、会话操作和 replay 集中在现有层；新上下文要通过现有请求/事件边界。 |
| REPL/TUI/交互 | `src/cli/repl/`、`src/cli/output/`、`src/cli/terminal_guard.rs`、`src/question_tui/`、`src/config_tui/` | 常驻 REPL、渲染、输入、问题流、远程回合和 terminal guard 才是 G8 真实入口；不得只改 `entry_flows.rs` 或新增外挂面板。 |
| prompt/persona | `crates/yunxi-core/src/persona_hint.rs`、`crates/yunxi-engine/src/agent/prompt.rs` | 已有 persona cache、append-only block、memory/host preamble；G1 从这里垂直切片。 |
| session/state/compact | `crates/yunxi-core/src/state/conversation_db/`、`crates/yunxi-core/src/state/`、`crates/yunxi-engine/src/agent/compact*.rs` | 短期上下文、evicted context、固定列序、WAL/VACUUM INTO、append-only journal 和回放一致性是 G3/G9 的硬证据。 |
| memory/embedding | `crates/yunxi-core/src/memory/`、`crates/yunxi-base/src/embedding/local.rs`、`src/cli/embed_cmds.rs` | 现有 short/long、origin/access、semantic/search/schema；G3 扩 admission，不复制 store。 |
| state/migration | `crates/yunxi-core/src/state/`、`crates/yunxi-core/src/state/migrations/`（`mod.rs`、`named.rs`、`columns.rs`、`baseline.rs`、`tests.rs`） | 当前**版本化 schema** latest version 为 40；`named.rs` 是独立的命名迁移链，不与 `user_version` 混用。G2/G3 只能追加迁移，Turn 固定列序和 `map_turn_row` 必须同步。 |
| knowledge | `crates/yunxi-engine/src/tools/knowledge_base/{mod,store,search,dashboard}.rs`、`kb/`、`crates/yunxi-hosts/src/web/dashboards/kb.rs`、`src/cli/data_cmds.rs`/`embed_cmds.rs` | 已有独立元库/语义库、文件边界、关键词/embedding 搜索和 reindex；G4 以现有 KB 为底座，不得与 memory 合表。 |
| KB 更新/迁移 | `crates/yunxi-engine/src/default_kb.rs`、`tools/apply_patch/{mod,tests}.rs`、`crates/yunxi-engine/src/transfer/{export,import,fixups,manifest,mod,registry}.rs` | source→snapshot→meta DB→semantic index→reindex、write-through、版本、导入/导出/恢复和秘密分类是 G4/G9 权威；Web dashboard 只作历史/展示入口，不是 Linux 主链。 |
| MCP/Skills/工具面 | `crates/yunxi-core/src/skills/{mod.rs,manifest.rs,draft/}`、`crates/yunxi-engine/src/tools/{compose_core.rs,compose_providers.rs,load_tools.rs,skills.rs,mcp/{connection,pool,protocol,scope,runtime}.rs}`、`src/personas/*/skills/**/SKILL.md`、`testkit/{oobe/mcp_probe.py,mcp-persistent/}` | 复用 persona manifest × machine config 快照、PluginKind::Provider 注册顺序、SkillsSource 指令尾、MCP 常驻断连回退；验收资源布局、开关组合、request-shape 五脸和持久 MCP。 |
| tools/host/权限 | `crates/yunxi-engine/src/tools/{command_guard.rs,net_guard.rs,registry/,default_tools/command*}`、`crates/yunxi-base/src/host_ports/{host_grants.rs,host_query.rs,turn_restrictions.rs,ports.rs,live_turn.rs}`、`crates/yunxi-hosts/src/platforms/{access_control.rs,commands.rs,tool_context.rs,turn_context.rs,turn_ownership.rs,turn_run.rs}` | 真实 principal、host capability、turn restriction、command/net guard 和执行结果是权限权威；prompt 标签与决策层都不能鉴权或提权。 |
| 扩展生命周期 | `crates/yunxi-core/src/skills/`、`crates/yunxi-engine/src/tools/compose_providers.rs`、`crates/yunxi-engine/src/tools/mcp/` | Skills/MCP 是跨层 seam，不得作为外挂功能单独绕过 persona 快照、工具注册或权限模型。 |
| persona/profile | `crates/yunxi-core/src/persona_hint.rs`、`src/config_tui/`、`crates/yunxi-core/src/state/`、`crates/yunxi-engine/src/transfer/registry.rs` | G1/G2 必须覆盖确认/推断来源、persona rename/delete scope migration 和 profile 不进向量库。 |
| 调度/后台 job | `crates/yunxi-hosts/src/platforms/scheduling.rs`、`crates/yunxi-hosts/src/platforms/plugins/scheduled_messages/`、`crates/yunxi-hosts/src/runtime/events.rs`、`crates/yunxi-engine/src/tools/jobs/`、`crates/yunxi-engine/src/agent/turn_loop/` | G6 只能复用现有事件、job 生命周期、取消和前台让位规则；不得新建第二调度器。 |
| goal 持久化 | `crates/yunxi-engine/src/tools/goal/`、`src/tools/descriptions/goal.json`、`crates/yunxi-core/src/state/conversation_db/goals.rs`、`crates/yunxi-hosts/src/web/goal_driver.rs` | 产品内 `goal` 的 rounds/armed/awaiting/restart 运行时持久化与 Codex active goal 是两件事；G0 需单独记录，不能混为计划文本。 |
| subsystem seam | `docs/interfaces/subsystems.md`、`crates/yunxi-engine/src/tools/compose_providers.rs` | memory 的 ToolRegistration/SystemPrompt/BeforeModel/AfterTurn、skills/persona_hint 的互不 use、插件快照顺序和 raw/display/context 三分是接入协议；新模块先挂在这些 seam 上。 |

本表是 G0 的审计产物，不代表所有领域都已经完成设计；下一次只补齐入口的调用关系、状态流和测试文件，不提前实现业务模块。

#### G0 调用关系（当前基线）

```text
fish hook
  ├─ 可判定为普通 fish 语法 ───────────────→ fish 原生执行
  └─ classify/intercept ─→ yunxi CLI shell bridge
                              ├─ 需要 daemon ─→ home singleton → runtime/IPC
                              └─ 直接回合   ─→ engine agent
                                                   ├─ prompt/persona/memory context
                                                   ├─ tools/host/MCP/Skills
                                                   └─ result/event → runtime → TUI/平台 host

persona_hint + state/profile ─→ prompt append-only/cache
memory store ─→ short/long + semantic search ─→ memory preamble
KB files ─→ KB metadata/semantic DB ─→ knowledge tools/search（独立于 memory）
```

当前没有发现第二个 daemon 生命周期；`crates/yunxi-hosts/src/daemon.rs` 是统一 host 入口，`src/cli/daemon_cmds.rs` 与 `crates/yunxi-core/src/ipc/{launch,lifecycle}.rs` 共同负责生命周期控制，fish 只通过既有 CLI/IPC 路径进入。后续若发现重复入口，必须先登记并解释，不能静默新增。

G0 入口表的测试对应关系：fish/shell 使用 `crates/yunxi-base/src/shell/` 单测；daemon/IPC 使用 `crates/yunxi-core/src/ipc/`、`tests/daemon_reload.rs` 与 runtime IPC tests；REPL/TUI 使用 `src/cli/tests/` 中的 `tui_*`、`turn_panel.rs`、`replay_*` 和 `testkit/tui/`；prompt/cache 使用 `crates/yunxi-engine/src/agent/tests/` 中的 `request_shape.rs`、`instruction_source.rs`、`tool_face_cache.rs`、`context.rs`、`compact_*.rs` 与 `testkit/cache-forensics/`；memory 使用 `crates/yunxi-core/src/memory/tests/` 与 `testkit/memory-quality/`；KB 使用 `crates/yunxi-engine/src/tools/knowledge_base/`、`default_kb.rs`、`tools/apply_patch/tests.rs`；MCP/Skills 使用 `testkit/oobe/mcp_probe.py`、`testkit/mcp-persistent/`；迁移/隐私使用 `transfer/`、`state/migrations/tests.rs`；job/调度使用 `tools/jobs/tests/`、`platforms/plugins/real_context/tests/` 和 `testkit/compact-concurrency/`。这些是后续阶段的验收入口，不表示本轮已经跑完全部套件。

### G1：原生 CompanionContext 与人格/灵魂提示链

**代码落点**：优先使用 `crates/yunxi-core/src/persona_hint.rs`、`crates/yunxi-engine/src/agent/prompt.rs`、现有 `PersonaLane` 与 prompt cache 入口。

- 增加最小、可序列化、带来源/版本/作用域的 `CompanionContext`：关系阶段、稳定语气、边界、回应偏好、当前陪伴状态。
- 只在现有 prompt append/fossilization 允许的位置注入；没有档案时保持 Miyu 默认行为。
- 不让人格上下文绕过工具权限、审批和 host capability。

**验收**：prompt 字节/顺序/cache 测试；数据缺失、损坏、旧版本回退；同输入回放一致；日志不泄露个人内容。

### G2：UserProfile 与关系状态

- 在现有 config/state/persona 边界内增加结构化 profile、relationship stage、confirmed/unconfirmed、来源和时间戳。
- 追加式 migration、旧数据启动兼容、读取/导出/删除；区分用户确认、模型推断、临时上下文。
- profile 只能参与明确的 prompt/companion context，不得进入向量库。

**验收**：迁移、重启恢复、并发访问、权限隔离、删除和隐私扫描；旧数据无需手工重建即可启动。

### G3：分层记忆与长期 admission

**代码落点**：复用 `crates/yunxi-core/src/memory/mod.rs`、`crates/yunxi-base/src/memory_types.rs`、现有 SQLite/embedding 端口。

- 明确 `turn/transient`、`short-term working set`、`long-term candidate`、`long-term committed` 四态。
- 设计 candidate 的来源、置信度、原因、保留期、撤销、commit/reject/expire；“记住/忘记”必须可预测。明确软遗忘与真正删除：删除必须清除正文、embedding、关联/缓存/摘要引用，并定义 transfer/backups 的处理；候选拒绝/过期永不进入索引。
- 规则先筛稳定偏好、项目约束、重复确认、未来价值和敏感度；只有 committed 才进入长期向量索引。
- 检索返回 provenance、scope、timestamp，并按预算注入 prompt；删除后不可召回，重复写幂等。

**验收**：闲聊不增长长期索引；敏感字段拒绝；崩溃/重启/半写恢复；召回质量与延迟有基线；已删除 id 不能被 keyword、semantic、association、compact、restore 或 backup import 再召回。

### G4：独立知识库与 RAG

- Linux 命令/系统知识使用独立 KB schema、目录、embedding、检索服务；未来私有知识通过明确授权的 namespace/source 加入，不与 memory 共表共索引。两库可以共享纯无状态 embedding 实现，但 schema、collection、召回 API、权限、迁移和生命周期必须独立；聊天内容不能静默写成私有知识。
- 对文档切片、去重、版本、失效、重建、引用/provenance 建立边界；检索只是事实候选，不能绕过执行层。
- 与 Skills/MCP/tool 说明衔接，缺 KB 时退化为现有 help/tool 结果。

**验收**：能证明 memory/KB 不共入口；更新 KB 不污染个人记忆；删除/重建/断索引可恢复；Linux 命令答复带引用和安全提示。

### G5：DecisionPort 与 Laya 可选决策层

- 定义低层 `DecisionPort`、请求/结果/原因类型；不把 Laya/ONNX 类型泄漏进核心领域。
- 先做 G5-00 可行性门：核实可用 artifact/checkpoint、推理接口、许可证、ONNX/runtime、Linux 与 macOS ARM/CPU 部署、延迟/RAM，以及中文/Linux 术语数据；先建立 deterministic baseline，再决定是否使用具体输出字段。之后才是 shadow Laya provider；feature/config 默认关闭，任何字段都需 schema 校验，score 不等于事实。
- 用于 context salience、memory admission、recall/rerank、terminal intent、主动陪伴候选排序；不能直接执行 shell 或写删数据。
- 建立脱敏的中文/Linux 评测集、延迟预算、拒绝率、混淆矩阵和回放；失败统一 fallback。

**验收**：关闭 Laya 时与基线一致；shadow 零副作用；无模型可编译/运行；开关、延迟、fallback 可观测。

### G6：陪伴行为与自动总结

- 复用现有事件/会话收尾/调度机制，不建第二调度器。
- 复用 conversation closing/idle trigger 的现有 owner。分别产出短期情景摘要、长期记忆候选、知识候选、profile 提案、关系/陪伴事件；每类拥有独立 destination、consent、expiry、dedupe、取消/重启协议，不能一份总结跨库写入。profile 推断不得静默落地，私有 KB 不由聊天内容静默收录。
- 情书/陪伴信箱、关系阶段、情绪语气作为可插拔 capability；失败不能影响终端主链路。

**验收**：不重复、不抢前台、不丢对话；重启幂等；候选可查看/拒绝/删除。

### G7：Linux 原生自然语言终端控制

- 深入复用 Miyu fish 接管、daemon、IPC、TUI、command/file/process/package/MCP 路径，不再造 shell parser/router。
- 链路固定为：理解 → 语义不足时澄清 → 复用 Miyu 当前 permission/confirmation contract → 执行 → 解释 → 可回放；只有既有规则要求确认时才确认，不新增 YunXi 通用审批状态机。危险/覆盖/批量/网络/安装操作沿用现有权限。
- 失败时可原样回退 fish；支持交互式/非交互式、TTY resize、SIGINT、退出、IPC 断连、长 Unicode 输出。

**验收**：模拟和真实 WSL Ubuntu 通过；条件允许时 Arch Linux 通过；daemon 重启、命令失败和回滚提示可验证。

### G8：TUI 原生化与可观测性

- 复用 Miyu 状态机、渲染、输入、通知和布局；YunXi 状态是原生字段，不是外挂面板。
- 展示当前模式、执行阶段、确认点、记忆/知识引用和错误；敏感内容默认脱敏。

**验收**：窄终端、长中文/Unicode、无色 ANSI、kitty、滚动、重绘、并发输出、窗口恢复、键盘路径；截图仅作视觉证据，不代替行为测试。

### G9：性能、隐私、鲁棒性与发布

- 建立启动、CPU/RAM、首 token、检索、决策延迟基线；embedding/决策模型按需加载、可禁用、失败回退。
- 故障注入数据库锁/索引损坏/模型缺失/磁盘满/权限不足/网络断开/重启/半写/缓存不一致/非法决策。
- 完成 README、AGENTS、架构/迁移/部署/恢复/release note；公开仓库秘密扫描。

**最终验收**：fmt、metadata、workspace check、分批 workspace test、架构依赖检查、WSL 实机、Arch 实机或明确记录未完成项；安装/升级/卸载/恢复和最小自然语言终端闭环可复现。

## 4. 每阶段固定工作流

1. 只读定位真实入口，登记阶段编号、代码文件、trait、状态和不变量。
2. 先写设计与失败/边界测试，再实现最小垂直切片；不先做大范围重构。
3. `cargo fmt --all -- --check`、定向测试、相关 crate check、workspace check；资源不足时分批并记录证据。
4. 做故障注入、重启/回放、隐私扫描和 WSL/Arch 实测；修复后重复同一命令。
5. 更新本计划的状态、实际文件、命令结果、残留风险和对应的阶段 release note（当前为 `docs/plan/2026-09-30-g0-release-note.md`）。
6. 只有退出条件全部满足才提交并推送当前分支，提交信息必须包含阶段编号；失败留在本阶段。

## 5. 阶段状态表

| 阶段 | 状态 | 下一步 |
| --- | --- | --- |
| G0 基线审计 | 进行中（基线测试已全绿；入口调用关系与隐私审计待收口） | 完成逐项入口/调用/测试映射、公开文件隐私扫描和 Arch/macOS 缺口记录 |
| G1 CompanionContext | 未开始 | G0 通过后做最小垂直切片 |
| G2 Profile/关系 | 未开始 | G1 通过后追加迁移 |
| G3 分层记忆 | 未开始 | G2 通过后做 admission |
| G4 独立知识库 | 未开始 | G3 边界验收后进行 |
| G5 DecisionPort/Laya | 未开始 | G3/G4 接口稳定后 shadow |
| G6 陪伴/总结 | 未开始 | G1–G5 的保存入口稳定后 |
| G7 终端自然语言层 | 未开始 | 复用 Miyu 终端路径 |
| G8 TUI | 未开始 | 终端链路稳定后 |
| G9 硬化发布 | 未开始 | 所有功能阶段完成后 |

当前只允许执行 G0；不能用“能启动”替代阶段验收。
