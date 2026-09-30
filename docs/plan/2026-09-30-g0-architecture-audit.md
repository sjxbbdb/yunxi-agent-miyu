# G0 架构入口与边界审计（as-built）

日期：2026-09-30
阶段：YXM-G0 进行中
范围：`sjxbbdb/yunxi-agent-miyu` 当前产品基线

本文是对 `2026-09-30-yunxi-miyu-long-horizon.md` 中 G0-01、G0-02、G0-03、G0-04、G0-08、G0-09 的逐项取证。它只描述当前代码已经存在的入口和后续必须守住的接缝，不提前实现 G1–G9 的业务能力。

## 1. 单一运行时与终端入口

| 责任 | 真实入口（as-built） | 调用方向与所有者 | 不变量 | 证据/后续门禁 |
| --- | --- | --- | --- | --- |
| fish 接管 | `crates/yunxi-base/src/shell/fish.rs`、`src/cli/shell_bridge.rs` | fish hook 先分类；普通 fish 语法回 fish，非命令文本进入 `run_shell_intercept`，再复用当前 CLI/daemon 回合 | 不新增 shell parser/router；普通 fish 语法不被截获 | fish 模板单测；G7 黑盒测试普通命令、自然语言、多行、Ctrl-C |
| daemon 单例 | `crates/yunxi-hosts/src/daemon.rs`、`src/cli/daemon_cmds.rs` | CLI 通过 `HomeSingletonLease`/`DirectCoreLease`/`WebCoreLease` 连接同一 daemon；启动、停止、reload、残留清理由现有入口负责 | 同一 home 不运行第二 daemon；lease 崩溃可回收 | `tests/daemon_reload.rs`；G7/G9 重启与残留 lease 注入 |
| IPC 协议 | `crates/yunxi-core/src/ipc/{lifecycle,launch,protocol,frames}.rs` | `lifecycle` 管生命周期与 socket，`protocol` 管 v3 请求/响应和版本，`frames` 管 4-byte frame 与半帧读取 | 版本协商、长度上限、半帧/EOF 行为不可被新模块绕过 | IPC unit tests；G9 超限、半帧、断连、协议不匹配 |
| 事件/回放 | `crates/yunxi-hosts/src/runtime/{events,ipc_events}.rs` | `EventHub` 发布并保存有限 ring；订阅者从 `subscribe_after`/`replay_after` 读取，解码器统一解释事件 | replay/resync 不能重复执行副作用；未知/坏事件只能降级 | `ipc_events` 测试；G7/G8 断连重连与 resync |
| 回合/问题 | `crates/yunxi-hosts/src/runtime/{run,questions,turn_update}.rs` | `ManagerState`/`IpcRunGuard` 管运行中回合；`QuestionBroker` 管 pending/settled/answer/close/cancel | cancel、question settle、run guard 必须幂等，退出时释放占用 | runtime unit tests；G7 SIGINT、断连、问题取消 |

**结论**：当前未发现第二个 daemon 生命周期、第二个 shell router 或第二个 IPC 协议实现。未来 CompanionContext、Laya、自动总结均只能挂在上述回合/提示/事件边界，不能新建常驻链路。

## 2. session、compact 与内容三分

| 责任 | 真实入口 | 关键边界 |
| --- | --- | --- |
| session/turn 状态 | `crates/yunxi-core/src/state/conversation_db/{sessions,session_state,turns,turns/journal,turns/running,queue,restart,pages}.rs` | session 所有权、运行中回合、队列、journal 和重启恢复属于 state；不可由 memory 或 KB 复制 |
| compact | `crates/yunxi-engine/src/agent/{compact,compact_extras,compact_transcript}.rs` | compact 只单调折叠上下文；evicted context 可被召回，但不是长期记忆提交 |
| 展示/原始/上下文 | web/CLI turn DTO 与 `turns.rs` 的 `raw_content`、`display_content`、`context_messages` | 原始用户内容只读保存；展示文本可本地化/脱敏；上下文注入走独立消息，不回写原始正文 |
| persona/profile sidecar | `crates/yunxi-core/src/persona_hint.rs`、`src/config_tui/`、state/transfer | profile 与 persona 提案具有来源、版本和作用域；不进向量库 |

G3/G9 必须分别证明 transient、short、evicted、long 四种数据在 compact、replay、export/import、删除后不会互相污染。真正删除必须同时处理正文、embedding、association、缓存、摘要引用以及可恢复归档；“隐藏”或“forgotten”不等于删除。

## 3. memory、KB 与 transfer 隐私边界

| 领域 | 权威入口 | 生命周期与恢复约束 |
| --- | --- | --- |
| memory | `crates/yunxi-core/src/memory/`、`crates/yunxi-base/src/memory_types.rs` | short/long、origin/access、semantic/search 共用现有 memory seam；后续只增加 admission 和删除语义，不复制 store |
| knowledge base | `crates/yunxi-engine/src/tools/knowledge_base/{mod,files,index,search,store,dashboard}.rs` | source → snapshot → metadata DB → semantic index；只由 KB 工具/写穿路径更新，聊天不能静默写入 |
| 默认知识更新 | `crates/yunxi-engine/src/default_kb.rs`、`src/cli/{data_cmds,embed_cmds}.rs` | 远端更新有超时、快照 hash 和 reindex；断网时保留旧快照，不污染 memory |
| transfer registry | `crates/yunxi-engine/src/transfer/{registry,manifest,export,import,fixups}.rs` | 所有可迁移单元必须登记 tier、secret/Never、schema 版本；SQLite 用 `VACUUM INTO` 快照，导入走 staging、占用检查、备份和回滚 |

G4/G9 的证据必须覆盖：memory/KB 不共表、不共检索 API、不共权限；KB 删除/reindex 不触碰 memory；secret/Never 单元不进入公开归档；schema downgrade 被拒绝；恢复失败不会留下半安装状态。

## 4. Skills、MCP 与权限真相源

### 扩展生命周期

- Skills 入口：`crates/yunxi-core/src/skills/{mod,manifest,draft/*}.rs`、`crates/yunxi-engine/src/tools/{compose_providers,compose_core,load_tools,skills}.rs`。
- Skills 已作为 `PluginKind::Provider` 注册；人格 manifest × 机器配置先生成快照，再决定工具面和 `<available-skills>` 指令尾。
- MCP 入口：`crates/yunxi-engine/src/tools/mcp/{mod,connection,pool,protocol,scope,runtime}.rs`。连接池负责 idle sweep、crash/timeout pause、session forget、retire_changed、shutdown；断连只能回退，不能阻塞整个 daemon。
- `docs/interfaces/subsystems.md` 是当前挂接契约：memory 的 `ToolRegistration/SystemPrompt/BeforeModel/AfterTurn` 顺序、插件注册顺序和 request-shape 字节稳定性都属于兼容面。

### 权限与执行

- host 端口：`crates/yunxi-base/src/host_ports/{host_grants,host_query,turn_restrictions,ports,live_turn}.rs`。
- 平台执行：`crates/yunxi-hosts/src/platforms/{access_control,commands,tool_context,turn_context,turn_ownership,turn_run}.rs`。
- 工具守卫：`crates/yunxi-engine/src/tools/{command_guard,net_guard,registry/}`。

这些模块中的 principal、capability、turn restriction、command/net guard 和执行结果才是权限真相源。prompt 标签、Laya 结果和 YunXi 陪伴状态都不能鉴权、提权或另造 approval 状态机。

## 5. 后台调度与产品 goal 的区别

- 回合调度/限额/会话租约：`crates/yunxi-hosts/src/platforms/scheduling.rs`。
- 后台任务：`crates/yunxi-engine/src/tools/jobs/{mod,ledger,output,tests/*}.rs`。
- 平台定时消息：`crates/yunxi-hosts/src/platforms/plugins/scheduled_messages/{mod,schedule}.rs`。
- 产品 goal：`crates/yunxi-core/src/state/conversation_db/goals.rs`、`crates/yunxi-engine/src/tools/goal/{mod,command,prompt,runtime}.rs`、`crates/yunxi-hosts/src/web/goal_driver.rs`。

产品 goal 的 CAS revision、round claim、armed/awaiting/restart 与 Codex 的 active goal 完全不同。G0/G9 不能用 Codex goal 文本证明产品 goal 已恢复；必须分别测试产品 session 状态、daemon 重启、人类 resume 和 blocked 计数。

## 6. 当前阶段门禁与未验证项

已完成：入口定位、重复运行时初查、核心数据边界、prompt/cache 接缝、Skills/MCP 与 host 权限真相源定位；工作区基线测试、格式/metadata/架构依赖检查已通过；WSL fish 静态判定 17/17、真实 fish PTY 接管、daemon reload 2/2 和 IPC 定向 33/33 已复现。

仍未完成：

1. TUI、工具执行的隔离黑盒实测记录（G0-09）；IPC 定向单测已完成，工具执行隔离探针能实际跑通命令并保活 REPL，但 TUI 表单 PTY 当前 8/16，fish 分流与 daemon reload 子项已完成，仍不能替代整个终端闭环。
2. 每条路径的隐私扫描证据索引与 transfer 单元逐项核对。公开文档路径命中已清零；transfer registry 的 59 个 unit 已完成静态分类，但仅 11 个代表性测试通过，逐项恢复/删除、manifest/hash/version、失败回滚和恶意归档证据仍缺；另有待确认的第三方 `APP_SEC`，需在发布前分类处理。
3. Arch Linux 实机和 macOS M-series 编译/运行；当前只能标记为未验证。

在上述门禁完成前，不创建 `CompanionContext`、Laya provider、向量 admission 或新的调度器；G1 仍保持未开始。
