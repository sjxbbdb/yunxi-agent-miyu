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

### 3.1 G0-03 数据边界矩阵（as-built）

下表把当前代码中的物理目录、事实表、派生索引、embedding 入口和删除/恢复路径落到同一张矩阵。表中的“否”是硬边界：后续阶段不得因为复用算法而把事实域、权限域或迁移账本合并。embedding 是派生索引，不是任何域的事实源。

| 数据域 | 当前真实位置 / 入口 | 已确认 schema 或内容 | embedding | 删除 / 恢复语义 | G0 判断与后续边界 |
| --- | --- | --- | --- | --- | --- |
| Profile | `YunXiPaths::profile_file()` → `home/<admin>/profile.md`；成员档案为 `home/<user>/profile.md`；旧布局 `identities/user-identity.md` | Markdown；由 `persona_paths.rs` 按 Owner 入口注入 `<current-user-profile>` | **否** | 文件写入/配置保存；transfer `home.profile` 为 Core；可导出明文 | 保持独立，不进入 `memory_embeddings`；G2 只增加 confirmed/source/timestamp 等结构，不改变向量边界 |
| Persona / Soul | `data/prompts`、私有人格目录、`personas/`；入口 `crates/yunxi-base/src/config/persona_paths.rs` | `persona.md`、manifest、人格目录与 skills/scripts | **否** | transfer `data.prompts`、`data.persona_manifest`、`personas.manifest`；按现有路径迁移 | 与 Profile 分域；人格内容可以被提示注入，但不能变成长期记忆事实 |
| Conversation / session | `<home>/<admin>/conversation.db`；旧布局 `state/conversation.db`；`ConversationDb::open_at()` | SQLite WAL：`sessions`、`turns`、`queued_prompts`、`session_loaded_items`、附件、tool reports、journal、redo、goals、accounts、platform bindings 等 | **否** | `delete_session` 级联删除；reset 删除会话内容；redo 事务恢复；迁移前 `VACUUM INTO ...bak` | 是交互事实源；短期上下文不能另建 router/daemon/store |
| Short / long memory | `active_persona_memory_data_dir(...)/memory`，通常 `personas/<scope>/memory/memory.db`；状态库 `state/personas/<scope>/memory/evicted_context.db` | data DB：`facts`、`episodes`、`pending_events`、`skill_records`、`memory_revisions`、`memory_meta`、`memory_embeddings`；state DB：`evicted_turns`、`evicted_embeddings`、FTS5 | **仅 memory 域内部派生**；不与 KB 共库 | `MemoryStore::reset_session()` / `reset_all()` 同事务清理正文与向量；衰减可标记 forgotten | 当前已有 short/long、promotion、expiry 字段；G3 必须补 transient→short→candidate→committed/rejected/expired 的显式 admission，只有 committed 可长期向量化 |
| Memory access | `facts`/`episodes` 的 `visibility`、`owner_principal`、`subjects`；`MemoryAccess` 与 `migrate_memory_access_v1/v2` | 来源、平台、session 推导 ownership；不可信平台字段与 principal 分离 | 召回 SQL 复用同一 principal 过滤 | reset 与 embedding 删除同一事务 | 权限边界已存在；KB 不得复用 memory ownership 表或删除 API |
| KB source / metadata | source：`data/kb/files` 或 `home/<user>/kb/files`；入口 `tools/knowledge_base/{store,files}.rs`；metadata `data/kb/kb_meta.db` | source 文本；`files(name,path,size_bytes,mtime,content_sha256,updated_at)` | **否** | `KnowledgeBase::remove()` / `remove_prefix()` 删除 source、metadata 和对应 chunks；可从 source/hash 重建 | 独立于 memory；禁止导入 `memory.db`、`conversation.db`、persona/skill/config |
| KB semantic index | `data/kb/semantic_index.db`；入口 `tools/knowledge_base/index.rs` / `search.rs` | `semantic_chunks(provider_id,model,file_name,content_sha256,chunk_index,start_char,end_char,text,embedding_json,embedding,created_at)` | **是，且只属于 KB namespace** | 删除文件按 `file_name` 删除 chunks；换模型重建旧 model；断索引保留 source | 可复用 embedder 算法，但不能共用 memory namespace、ACL、删除 API 或迁移账本 |
| Embedding assets | `YUNXI_EMBEDDING_MODELS_DIR`、`~/.yunxi/models`、安装前缀 models；`embedding/manifest.rs` 与 worker | manifest、ONNX、tokenizer、frame/timeout 限制 | 模型资产，不是用户事实 | 可重新安装；不属于默认用户数据迁移 | G4 需按 namespace/model/dimension 校验，memory 与 KB 的模型配置仍要分开 |
| Credentials / web secrets | `config/config.jsonc`；`state/web-passwords/`、`state/daemon-launch.json` | provider keys、access token、MCP/plugin env、voice/TTS keys、端口/密码状态 | **否** | `--no-secrets` 递归脱敏；Web password、daemon launch 为 Never；运行时生命周期清理 | 任何 secret/Never 不进公开归档、向量索引或 prompt 指纹；新增字段必须补扫描测试 |
| Runtime / cache / usage | XDG runtime socket/lock；`paths.cache_dir` logs/cache/reindex；生产账本 `state/usage.db`（`state::usage::ledger`） | socket、lock、可重建 JSON/log；usage DB 的 `usage_records`、`usage_totals`、`usage_meta`；旧 `usage.json` 与 `usage-history.jsonl` 仅作一次性导入源 | **否** | runtime/cache 为 Never；usage.db 与两份 legacy 输入均按独立 Core 单元迁移；legacy 导入由 `state/usage/legacy.rs` 幂等完成 | registry 必须覆盖生产 DB 与两份 legacy 输入，不能只登记旧 JSON；不能把用量账本混入 conversation DB |
| Transfer backup / restore | `crates/yunxi-engine/src/transfer/{registry,manifest,export,import,fixups}.rs` | manifest + tar.gz；SQLite `VACUUM INTO`；tier、scope、`included_units` | 不改变域语义 | staging → validate → backup → install/prune → marker；失败 rollback；legacy manifest merge-only | 61 units 已覆盖 Core/Heavy/Platform/Never；父目录 TOCTOU 仍是未关闭风险 |

#### G0-03 当前结论

已确认四个独立事实域：`Profile/Persona → Conversation/Session → Memory → Knowledge Base`；各域的 source、metadata、embedding 和 transfer 分类已经能在代码中定位。尚未完成的是跨域运行时验证矩阵：Profile 不入 memory embedding、memory reset 不触碰 KB、KB remove/reindex 不触碰 memory、旧新布局 identity、usage 实际路径以及故障注入/恢复证据。因而 G0-03 由“仅定位”提升为“静态边界已证、运行时边界待验”，不视为完成。

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

已完成：入口定位、重复运行时初查、核心数据边界、prompt/cache 接缝、Skills/MCP 与 host 权限真相源定位；工作区基线测试、格式/metadata/架构依赖检查已通过；WSL fish 静态判定 17/17、真实 fish PTY 接管、daemon reload 2/2、IPC 定向 33/33 和 transfer 定向 36/36 已复现。transfer 还覆盖了 coverage-aware stale Core 清理、旧清单 merge-only、输入归档保护、清理后 marker 失败恢复、rename 错误分类、旧新 persona/home wildcard、生产用量账本路径以及 SQLite usage snapshot/import 回归。

仍未完成：

1. TUI、工具执行的隔离黑盒实测记录（G0-09）；IPC 定向单测已完成，`testkit/g0-terminal-combo/run.py` 已在同一隔离 home/daemon 下先后验证真实 fish PTY 与 REPL PTY，并从 `turns.tool_flow` 校验两次工具输出；`testkit/repl-smoke/run.py` 也已自带工具调用并从 `turns.tool_flow` 校验输出，报告 `passed=true`；TUI 表单 PTY `testkit/tui/config_forms.py` 已在 WSL Ubuntu-24.04 pyte venv 下复跑为 16/16。组合黑盒子项已通过，但仍需保留故障注入与跨平台验证。
2. 每条路径的隐私扫描证据索引与 transfer 单元逐项核对。个人路径与凭据形状扫描已完成分类；transfer registry 的 61 个 unit 已完成静态分类，当前定向测试 36/36 通过，manifest/hash/version、资源上限、tier 矩阵、旧新布局映射、恶意归档拒绝、失败回滚、coverage-aware stale Core 和 rename 错误分类证据已落地。仍缺少 install 父目录检查与后续操作之间的目录句柄级竞态消除，以及逐项平台句柄后端的恢复/删除证明。第三方 `APP_SEC` 已核验为公开客户端签名常量并列入 allowlist。
3. Arch Linux 实机和 macOS M-series 编译/运行；当前只能标记为未验证。

### 6.1 可复现隐私门禁

门禁脚本为 `testkit/privacy/g0_scan.py`，只扫描 Git 已跟踪的文本文件。运行：

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test`

`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py`

本轮运行结果：1858 个 Git 跟踪文本文件通过扫描，`personal_path=0`、`private_key=0`、`credential_shape=0`；公开第三方签名常量归类为 1 个 `public_allowlist` 文件，合成测试值归类为 2 个 `fixture_allowlist` 文件。输出只包含类别、计数和路径，不回显匹配内容。该门禁不替代 transfer 的逐项恢复/删除、manifest/hash/version、失败回滚和恶意归档审计。

在上述门禁完成前，不创建 `CompanionContext`、Laya provider、向量 admission 或新的调度器；G1 仍保持未开始。

## 6.2 G0 证据索引

命令级矩阵与复跑要求见 [`2026-09-30-g0-test-matrix.md`](2026-09-30-g0-test-matrix.md)。

| 状态 | 范围 | 当前证据或缺口 |
| --- | --- | --- |
| 已证 | G0-01/G0-02/G0-04/G0-06 | 本文入口表、单运行时结论、prompt/cache 接缝记录，以及 `legacy_config_dir` 正负回归测试。 |
| 已证 | G0-05/G0-07 transfer 子集 | WSL Ubuntu-24.04 engine 650/0/13；transfer 36/36；privacy 1858 tracked text files，`personal_path=0`、`private_key=0`、`credential_shape=0`；架构依赖、metadata、fmt、workspace check 均通过。 |
| 已证 | G0-09 基线闭环 | fish 静态 17/17、daemon reload 2/2、IPC 33/33、terminal-combo、repl-smoke、TUI config 16/16 已有报告；这些证据仍不替代故障注入。 |
| 部分已证 | G0-03 | 已补齐 Profile/Persona、Conversation/Session、Memory、KB、embedding、credentials、cache、transfer 的真实路径、schema、删除/恢复和向量边界矩阵；跨域删除/恢复、旧新布局 identity、usage 实际路径仍待运行时验证。 |
| 仅定位 | G0-05/G0-08 | 权限真相源和测试入口已列出，但尚未形成完整耗时/失败原因、断连/权限组合和 request-shape 故障注入报告。 |
| 未验证 | G0-05/G0-09 跨平台 | Arch Linux 实机和 macOS M-series 尚未运行；只能保留为环境缺口。 |
| 未关闭 | transfer 安全硬化 | import 的路径检查兼容后端仍存在父目录 TOCTOU；export 的 output/source 检查与实际写入/读取之间也存在路径竞态。严格安全语义需要 Linux/macOS `openat`/`renameat` 与 Windows handle-relative backend，当前不能宣称竞态已消除。 |

该索引是 G0 的当前状态，不是阶段完成声明；G1 及后续阶段保持未开始。
