# YunXi × Miyu Linux 原生化长期 Goal Command v5

状态：已接入当前活动 goal，按 G0→G9 执行中。本文是可审计、可复制的长期执行合同，不是 G0 完成声明。

## 1. 用户目标的完整提取

用户要做的不是给 Miyu 外挂几个 YunXi 功能，而是把 `shorin/miyu` fork 逐步改造成 YunXi 的 Linux 原生基底：

1. 保留 Miyu 已经成熟的 fish 接管、常驻 daemon、IPC、TUI、session/state、prompt/cache、tool/host、MCP 和 Skills 能力。
2. 将 YunXi 的人格、灵魂、用户画像、关系阶段、分层记忆、独立知识库和陪伴策略以内生方式接入，使它们看起来像 Miyu 原生的一部分，而不是外挂面板。
3. 重新定义 Linux 交互：普通 fish 语法保持原义；用户也可以用自然语言直接与终端交互，由已有工具、沙盒和权限边界完成可解释的系统操作。
4. 最终目标是常驻 Linux 的陪伴型通用系统级 Agent，而不是普通聊天机器人、桌面 Web 应用或语音助手。
5. 记忆库与知识库永久分离：profile 不向量化；短期记忆承接上下文、语气和短期情景；长期记忆只有经过明确 admission 的 committed 内容才能进入自己的向量索引；Linux 命令知识先进入独立 KB，后续可增加授权私有知识 namespace。
6. 引入决策模型主线，但不能把模型变成第二个 Agent、router、scheduler、permission layer 或 memory writer。`DecisionPort` 是唯一接缝，Laya 只是可替换 provider；先 deterministic，再 shadow，再按消费者评测决定是否采纳。
7. 质量要求：代码简洁、模块化、可回滚、符合 Miyu 代码风格；开发、测试、故障注入、修复、复测、审查和推送必须形成闭环，不能用“能启动”代替验收。
8. 每次代码、测试或文档变更在验证通过后立即提交并推送；测试后必须清理临时 target、WSL 临时目录、测试 home、daemon、MCP 子进程和其它生成物，不把个人数据、token、绝对路径或构建产物提交到公开仓库。

## 2. 可复制的长期 Goal Command

下面代码块是当前活动 goal 的详细执行合同；活动状态由 Codex goal 跟踪，阶段细节、证据和残余风险以本文为准。合同最初从 `CURRENT_STAGE=G0` 激活；阶段推进必须以退出审计和当前状态对账为准，不得跳过未退出阶段。

```text
GOAL_ID
  YXM-LINUX-NATIVE-V5

MODEL_ROLES
  lead_model: GPT-6Astra
  worker_model: GPT-6Astra
  verifier_model: GPT-6Astra
  research_model: GPT-6Astra

ROLE_CONTRACT
  Lead/GPT-6Astra 负责理解用户意图、读取计划和当前源码、做架构归属判断、拆分任务、确定是否允许实施、审查 diff、判断测试证据、决定提交边界和报告残余风险。Lead 不把未验证的 worker 输出当作事实，不把计划中的 API 写成已存在，不在 G0 实现 G1-G9 业务代码，不加载 Laya 权重。
  Worker/GPT-6Astra 只执行 Lead 发出的一个受限施工单。施工单必须列出绝对仓库路径、当前函数/trait/测试锚点、允许修改的文件、禁止修改的文件、输入输出契约、逐步命令、测试命令、临时目录、清理命令和停止条件。Worker 不得扩展范围、重命名公共 API、改变权限语义、删除数据、修改 secrets、运行 cargo fix、reset/rebase/force-push、提交不在施工单内的文件，锚点对不上或发现第二运行时就停下报告。
  Research/GPT-6Astra 只做源码/文档/GitHub/许可证/模型来源调查；输出 URL、commit/tag、许可证、版本、事实与推断分栏；不把未经核验的开源项目、模型或 API 写进实现计划，不下载权重，不修改仓库。
  Verifier/GPT-6Astra 只做独立检查：读取 diff、运行指定测试和隐私扫描、检查进程/临时目录/远端 SHA、报告通过或失败及未覆盖项；不得顺手修代码。失败后由 Lead 重写施工单，再派新的 worker。
  一棵 worktree 同时只允许一个会话写文件；其它 agent 只能只读。Worker 完成后必须发送结构化交接：改动文件、函数锚点、测试命令/退出码/计数、故障注入、清理证明、commit SHA、push SHA、残余风险。

REPOSITORY
  root: D:/YunXi-Miyu
  product: sjxbbdb/yunxi-agent-miyu
  base: shorin/miyu
  historical_reference: sjxbbdb/YunXi-Native
  branch: codex/yunxi-product-rename
  current_head: 79d2976a

OBJECTIVE
  在 Miyu 的 Linux 原生底座上完成 YunXi 化长期升级：保留 fish/daemon/IPC/TUI/session/prompt/cache/tool/host/MCP/Skills；以内生方式加入人格、灵魂、profile/关系、分层记忆、独立知识库和陪伴策略；最终让用户可以用自然语言接管 fish 终端并完成系统级操作，同时保留原生 shell 语义、可解释权限和可回滚故障边界。

CURRENT_STAGE
  G5-06-DESIGN（G0/G1/G2 已按退出审计推进；G3-05 已按边界退出，G4-01 provenance、
  G4-02-01 数据面隔离、G4-02-02 可信调用方注入、G4-03 版本生命周期、G4-04
  更新/导入失败可观测性与 G4-05 更新事务恢复/收敛均已完成；G5-00 来源审计、
  G5-01 DecisionPort 契约与 G5-02 abstain-only deterministic provider 已固化，
  G5-03 已按限定边界退出，G5-04-01 raw-free adapter、G5-04-02 默认关闭 observer
  与 G5-04-03 metrics/token、G5-04-04 stale-response harness、G5-04-05
  consumer-facing harness、G5-04-06 无模型异步边界 harness、G5-04-07 fault/replay
  边界、G5-04-08 metrics/replay 收敛和 G5-04-09 无模型 consumer 退出审计均已完成；
  G5-05 deterministic 评测已按 `2026-10-04-g5-05-exit-audit.md` 退出；当前进入
  G5-06 consumer adoption 设计，合同见 `2026-10-04-g5-06-consumer-adoption-design.md`。）

CURRENT_AUTHORIZATION
  G0/G1/G2 已退出，G3-05 按标准读线性化、有界 transcript barrier 与明确非目标边界退出；
  G4-01 已推送 `21dfb327`，G4-02-01 已推送 `0281b062`，G4-02-02 已推送 `58a0cbca`，
  G4-03 已推送 `ead9968e`，G4-04 已推送 `dbcc96b5`，G4-05 第一切片已推送 `f905f538`，
  G4-05-02 重索引耗尽状态已推送 `e4187b98`，G4-05-03 更新锁已推送 `69698f8d`，
  G4-05-04 重复 revision 收敛已推送 `2dc988bc`；G4-05 退出审计见
  `2026-10-04-g4-05-exit-audit.md`；G5-00 来源审计见
  `2026-10-04-g5-00-laya-source-audit.md`、`2026-10-04-g5-01-decision-port-contract.md`；
  G5-02 已推送 `2b458d39`，G5-03-A/B observer 已推送 `65f9319d`，G5-03-C
  预算/取消/队列/回放实现已推送 `a5f48982`，G5-03-D 第一批硬化已推送 `a735444b`；
  G5-03 限定退出审计见 `2026-10-04-g5-03-exit-audit.md`；G5-04-01 adapter 证据见
  `2026-10-05-g5-04-01-adapter-evidence.md`，G5-04-02 observer 证据见
  `2026-10-05-g5-04-02-observer-evidence.md`；当前只允许 G5-04-03 metrics/replay 与
  `2026-10-04-g5-04-03-metrics-token-evidence.md` 已记录 G5-04-03；G5-04-04 stale-response
  harness 已按 `2026-10-04-g5-04-04-stale-gate-evidence.md` 退出；当前只允许
  G5-04-05 consumer-facing harness 已按 `2026-10-04-g5-04-05-consumer-harness-evidence.md`
  退出；G5-04-06 已按 `2026-10-04-g5-04-06-async-boundary-evidence.md` 退出；当前只允许
  G5-04-07 已按 `2026-10-04-g5-04-07-fault-replay-evidence.md` 退出；G5-04-08 已按
  `2026-10-04-g5-04-08-metrics-replay-evidence.md` 退出；G5-04-09 已按
  `2026-10-04-g5-04-09-consumer-exit-audit.md` 退出；G5-05 已按
  `2026-10-04-g5-05-exit-audit.md` 退出；当前只允许 G5-06 consumer adoption
  deterministic/test-only 设计与数据集施工，不得下载/加载权重、创建真实 Laya provider、
  接入生产消费者或修改 memory/KB/runtime。Web identity/dashboard 与
  MCP grant 仍未授权，除非另立施工单。
  不得下载或加载 Laya 权重，不新增第二 router/daemon/prompt 链/memory store。

G5-05 设计合同见 `2026-10-04-g5-05-evaluation-design.md`，退出审计见
`2026-10-04-g5-05-exit-audit.md`。G5-06 设计合同见
`2026-10-04-g5-06-consumer-adoption-design.md`：先固定消费者采纳门、逐消费者开关、
脱敏评测、统计方法、故障回退与关闭路径；执行批次只能修改 `#[cfg(test)]` 夹具与证据
文档，不得把评测结果接入生产写入或 provider。

NON_NEGOTIABLE_PRODUCT_INVARIANTS
  N1. fish capture、daemon、IPC、TUI、session/state、prompt/cache、tool/host、MCP/Skills 各只有一套权威运行时；新模块只能挂既有 seam。
  N2. memory 与 knowledge base 的目录、schema、embedding、检索、权限、迁移、删除、恢复和审计全部独立；禁止共享表或混合索引。
  N3. profile 不向量化；短期记忆只承接当前上下文/语气/短期情景；长期向量只允许来自 committed 记忆；candidate/rejected/expired 不得进入长期 semantic index。
  N4. raw_content、display_content、context_messages 三分不可破坏；append-only、fossilization、prompt/cache 前缀和回放字节不得因新模块漂移。
  N5. secret、credential、完整 profile、未授权私密原文不得进入日志、embedding、memory index、KB index 或 DecisionPort 输入。
  N6. 所有本地数据必须可查看、导出、删除、恢复；公开仓库禁止 token、个人档案、绝对路径、生成索引和测试 home。
  N7. Linux 是主平台；WSL 证据不能冒充 Arch 实机，macOS 只要求 M-series 可编译边界；三者证据分开记录。
  N8. 普通 shell 语法不被自然语言层破坏；危险、覆盖、批量、提权和不明确操作必须走现有权限/澄清/预览边界，不能由 prompt 自我授权。
  N9. DecisionPort/Laya 只能建议选择或排序，不能执行工具、提权、鉴权、写删 memory/KB/profile、改变 scheduler 或主动发消息。

GOAL_LIFECYCLE
  阶段验收只推进 CURRENT_STAGE，不是长期 goal 的终止条件。goal 必须持续保持 active，
  跨越 G0→G9 逐项施工、测试、修复、审查、清理和推送；闲置、阶段完成、一次测试通过、
  子代理完成或用户暂时没有新消息都不能终止 goal。唯一终止条件是：①可用额度耗尽；或
  ② G0–G9 全部任务、验收、故障矩阵、隐私扫描、跨平台证据和发布准备全部完成。若额度
  耗尽，保留最后一个已推送 SHA、当前阶段、未决风险和下一施工单；若全部完成，必须先
  给出逐阶段证据索引和远端 SHA，再结束 goal。

SOURCE_MAP_CURRENT_ANCHORS
  行号只是当前 commit 的定位锚点；每次 worker 开工前必须用 rg 重新确认，锚点漂移即停止并回报。
  S1 fish：crates/yunxi-base/src/shell/fish.rs:161 __yunxi_on_prompt；:236 __yunxi_hand_to_ai；:247 __yunxi_accept_line；:291-294 Enter 绑定；:327 install；同文件 437+ 的 hook tests；黑盒 testkit/fish-accept-line/{run.py,pty_run.py}。
  S2 CLI/daemon/IPC：src/cli/mod.rs:193 run、:301 daemon 入口；crates/yunxi-hosts/src/daemon.rs:8 run；crates/yunxi-hosts/src/web/server.rs:10 run；crates/yunxi-hosts/src/web/ipc_server.rs:66 handle_ipc_connection、:1011 handle_ipc_turn；crates/yunxi-core/src/ipc/protocol.rs:175 Command、:290 StartTurn、:563 send、:574 receive；frames.rs:30 FrameReader；lifecycle.rs:123 acquire_home_singleton、:259 connect、:328 ensure_daemon、:530 shutdown_daemon。
  S3 TUI/session：src/cli/repl/mod.rs、editor.rs:94 LiveReplEditor、session.rs；src/cli/repl/tail/screen/{mod,draw,term,overlay}.rs；crates/yunxi-hosts/src/web/sessions/mod.rs:574 build_session_agent；crates/yunxi-core/src/state/conversation_db/{sessions,session_state,turns,queue,restart,pages}.rs；state/sessions.rs:166 ensure_repl_session、:287 create_session、:424 delete_session。
  S4 prompt/cache/compact：crates/yunxi-base/src/config/persona_paths.rs:39 system_prompt_with；crates/yunxi-engine/src/agent/{prompt.rs,setup.rs,instruction_source.rs,compact.rs,compact_extras.rs,compact_transcript.rs}；setup.rs:81 Agent::new_with_profile、:674 user_profile_applies；crates/yunxi-core/src/llm/cache_prefix.rs:31 PrefixChain；cache_break.rs:142 note_cache_rebuild；request shape tests 在 agent/tests/request_shape.rs。
  S5 memory/profile：crates/yunxi-core/src/memory/mod.rs:42 MemoryStore、:325 new、:431 init；memory/{write,dedup,organizer,recall,semantic}.rs；paths/mod.rs:471 profile_file、:481 user_profile_file；memory/tests/store.rs 的 profile prompt-only 回归。
  S6 KB：crates/yunxi-engine/src/tools/knowledge_base/mod.rs:71 KnowledgeBase、:80 new、:107 search、:112 search_readonly；index.rs:75 reindex_embeddings、:254 semantic_search、:316 reindex_embeddings_inner、:652 failed_chunk_write_rolls_back_the_file_and_a_retry_rebuilds_it；files/index/store/dashboard 必须保持分层。
  S7 MCP/Skills/权限：crates/yunxi-engine/src/tools/mcp/{mod.rs,connection.rs,pool.rs,scope.rs,protocol.rs,protocol_tests.rs,tests.rs}；mcp/mod.rs:46 register、:106 call_tool；pool.rs:76 enable、:337 forget_session、:355 retire_changed、:378 shutdown_all；crates/yunxi-core/src/skills/mod.rs:128 discover、:219 load；crates/yunxi-engine/src/tools/skills.rs:14 register_skills、:163 load_skill；config/mod.rs:396 McpConfig、:404 McpServerConfig、:456 McpSandbox；权限真相在 yunxi-base/src/host_ports/* 与 yunxi-hosts/src/platforms/access_control.rs，不能由 prompt 声明。
  S8 transfer：crates/yunxi-engine/src/transfer/{registry.rs,export.rs,import.rs,manifest.rs,fixups.rs}；registry.rs:35 DataUnit、:596 unit_for；export.rs:52 export；import.rs:88 import；现有跨域回滚测试 marker_failure_restores_memory_and_kb_together。
  S9 Decision seam：G5-02 仅已实现 yunxi-core 的版本化 DecisionPort 协议、校验器和
  abstain-only deterministic provider；尚不存在 Laya/shadow provider 或消费者接入。
  compact、memory admission、recall/rerank、terminal intent、proactive ranking 仍须
  保持既有 deterministic 规则为权威，不能把计划中的模型行为写成已实现。

EXECUTION_LOOP_FOR_EVERY_SLICE
  1 LOCATE：Lead 读取就近 AGENTS、计划、当前 worktree、远端 SHA；用 rg/CodeGraph（若存在）确认入口和行号。
  2 SPEC：Lead 写施工单，必须含不变量、正向/负向/故障/回放样例、允许文件、禁止文件、验收命令和清理命令。
  3 FAILING_TEST：Worker 先添加或确认真实入口上的失败测试；如果测试已经存在，必须报告当前覆盖，不重复堆测试。
  4 MINIMAL_SLICE：Worker 只改施工单允许范围；不顺手重构、不复制运行时、不改变公共语义。
  5 VERIFY：Verifier 按施工单运行 fmt、metadata/check、定向测试、必要黑盒、privacy/arch/refactor 门禁；一次只跑一个 cargo，并使用 cgroup 或唯一临时 target。
  6 FAULT_AND_REPLAY：对当前切片注入相关的断连、超时、非法输出、半写、权限、锁、删除/恢复或 SIGINT；确认无脏状态且 fallback 可回放。
  7 REVIEW：Lead 审核 diff、权限、prompt/cache 字节、数据跨域、日志、测试覆盖和残余风险。
  8 SYNC：验证通过后只 `git add` 明确路径，提交阶段/任务号，立即 push 当前分支；禁止把未验证变更留在本地。
  9 CLEANUP：删除 `/tmp/yunxi-*`、仓库 `target/`、隔离 home、临时 venv、fixture 数据、daemon/MCP 子进程；复查 `git status`、进程、远端 SHA。
  10 REPORT：报告任务号、改动文件/函数、真实命令、退出码/计数、失败与修复、清理证明、commit/push SHA、总体进度和未验证项。

G0_TASKS_AND_ACCEPTANCE
  G0-01 inventory：Lead 维护 SOURCE_MAP_CURRENT_ANCHORS；worker 只读核对入口；verifier 跑 arch_dep_check。证据是文件/函数/调用方向表；发现第二 daemon/router/prompt/memory 入口即停。
  G0-02 duplicate-runtime audit：核对 fish→daemon→IPC→REPL→tool、memory、KB、MCP pool 是否各一套；测试 testkit/g0-terminal-combo/run.py、repl-smoke/run.py、mcp-persistent/run.py。不能用“两个兼容适配器”掩盖第二运行时。
  G0-03 data boundary：核对 MemoryStore、profile paths、KnowledgeBase、transfer DataUnit；复跑 profile prompt-only、memory/KB 双向删除隔离、marker 联合回滚。profile/secret 进入 embedding 或跨库即 HARD_STOP。
  G0-04 prompt/cache contract：核对 persona_paths、prompt/setup、InstructionSource、PrefixChain、compact；改动前后跑 request_shape_probe、两轮 cache/fossil/replay。任何 system 前缀逐字节漂移或 profile 泄漏即停。
  G0-05 test matrix：继续补当前证据缺口：最新 HEAD workspace/黑盒复跑、transfer export TOCTOU、锁/权限/磁盘满/父目录替换/半写/跨库提交故障表；MCP 权限、既有断连/超时、Unix import 父目录竞态和非法 manifest 已有独立证据，但仍必须在退出审计中区分历史证据与当前 HEAD。每项 run-id/SHA/UTC/env/命令/退出码/未验证项。不能把历史摘要混作当前证据。
  G0-06 rename compatibility：只修复 Miyu→YunXi 产品层兼容/提示/资源命名，不改变底座行为；跑全量 grep、cargo fmt、workspace tests、privacy scan。发现源代码仍存在产品命名歧义要记录路径和是否属于历史兼容，不盲目替换协议/数据库字段。
  G0-07 privacy/docs：运行 testkit/privacy/g0_scan.py、自检和公开仓库扫描；更新 evidence index/release note/README 中真实状态。任何 token、个人路径、profile、生成索引或未脱敏日志进入 diff 即停。
  G0-08 MCP/Skills/permissions：核对 MCP protocol/connection/pool/scope、Skills discovery/registration、host_grants/turn_restrictions；补 request-shape、启动失败隔离、断连/超时、权限矩阵的真实测试。不得用 prompt 替代权限。
  G0-09 black-box：运行 fish PTY、daemon reload/IPC、terminal-combo、REPL、TUI、daemon-orphan、MCP persistent；WSL/Arch/macOS 证据分开。SIGINT、断连、重启、长 Unicode、危险操作失败必须保留真实输出摘要。
  G0 EXIT：只有当 G0-01..09 的证据可复现、所有失败已修复或明确登记 owner/阶段、privacy/permission/prompt/cache/transfer/replay 无已知破坏、G1-G9 都有真实代码落点/测试入口且未被写成“已完成”时，Lead 才能请求用户确认退出 G0；未获用户确认不得进入 G1。

DECISION_MODEL_MAINLINE
  D0/G0：只读盘点 compact、memory admission、recall/rerank、terminal intent、proactive ranking；记录 owner、输入、输出、隐私、权限和 deterministic fallback；不创建 provider、不加载权重。
  D1/G3-G4：先实现确定性 salience/admission/rerank/intent 规则，规则是模型关闭时的权威基线；模型建议不得成为事实、权限或写入凭证。
  D2/G5-00：research worker 核对 Laya 的官方来源、版本/tag、artifact/checkpoint SHA、许可证、runtime、CPU/Linux/macOS ARM、中文/Linux 术语质量、p50/p95、RAM；任何一项未知就停止，不下载权重。证据见 `2026-10-04-g5-00-laya-source-audit.md`。
  D3/G5-01：Lead 设计版本化 DecisionRequest/DecisionResult（task、schema_version、candidate_ids、scope、input_fingerprint、deadline、capabilities、choice、abstain、reason_code、confidence/provider 可选）；worker 只能实现批准的窄 trait，不泄漏 Laya 类型进核心领域；契约已固化在 `2026-10-04-g5-01-decision-port-contract.md`。
  D4/G5-02：先 deterministic provider；模型缺失/超时/断连/低置信度/非法 JSON/非法 choice/id/越界 score/过期响应，行为必须与基线逐字/逐序一致；本阶段不得下载模型或创建 shadow/Laya provider。
  D5/G5-03：shadow provider 可关闭、无副作用、可回放，只发送最小化脱敏元数据；不得执行工具、提权、写删 memory/KB/profile、改变 scheduler 或主动发消息。
  D6/G5-04：按 context salience→memory admission→recall/rerank→terminal intent→proactive ranking 顺序接入，每个消费者独立开关、预算、fallback、指标和回滚。
  D7/G5-05：评测重要约束保留、闲聊不长期收录、敏感拒绝、矛盾/低置信 abstain、重复记忆、memory/KB 隔离、意图澄清；记录拒绝率、混淆矩阵、p50/p95、RAM、超时、非法输出和断连回退。
  D8/G5-06：只有所有消费者质量/延迟/资源门通过才允许建议采纳；保留 reason、实际影响审计和一键关闭回到 deterministic baseline。

G1_TO_G9_LONG_PLAN
  G1 CompanionContext：复用 persona_paths::system_prompt_with、agent/prompt.rs、InstructionSource；Lead 先定 source/version/scope 和 prompt 字节合同，worker 只做最小 fixture/source，verifier 跑 prompt/request-shape/cache/fossil/replay/privacy。停：profile 泄漏、无旧版本回退、字节漂移。
  G2 profile/relationship：复用 paths profile_file/user_profile_file、config/persona、state migrations；追加 confirmed/inferred/source/time/scope 与关系事件；worker 只做增量 migration、导出/删除/恢复测试。停：推断冒充确认、进入向量库、迁移不可回滚。
  G3 layered memory：复用 MemoryStore、write/dedup/organizer/recall/semantic；先 deterministic admission，再接 DecisionPort 协议测试；worker 实现 transient→short/candidate→committed/rejected/expired 的最小切片。停：闲聊无限增长、敏感未拒绝、非 committed 向量化、跨 KB。
  G4 independent KB/RAG：复用 KnowledgeBase files/index/store/dashboard；先 Linux terminal command namespace，再授权私有 namespace；worker 做 source/metadata/semantic reindex、换模型、失效、导入/删除/恢复。停：KB 触碰 memory、权限扩大、partial index 可见。
  G5 DecisionPort/Laya：严格 D2→D8；模型只作建议，所有 provider 可关闭并回退。停：来源/许可证不明、模型改变权限/工具/写删/调度、基线不等价。
  G6 companion/auto-summary：复用 idle/closing/job owner，不新建 scheduler；分开生成短期摘要、长期候选、知识候选、profile 提案、关系事件；可取消、幂等、去重、可拒绝/删除。停：抢前台、无授权主动发送、跨域直接写入。
  G7 natural-language terminal：复用 fish hook→shell classifier/intercept→IPC→existing tool guards；支持理解、澄清、预览、执行、解释、回放，失败回 fish。停：普通 fish 语义改变、绕过权限、第二 router。
  G8 native TUI：复用 repl/tail/screen、session/event replay；YunXi 状态成为原生字段；verifier 覆盖窄屏、Unicode、ANSI/kitty、滚动、并发重绘、键盘、恢复和截图。停：外挂第二状态机或 replay 副作用。
  G9 hardening/release：建立启动/CPU/RAM/首 token/检索/决策/质量基线；注入锁、磁盘满、权限、断网、模型缺失、重启、半写、非法输出；完成安装/升级/卸载/恢复、迁移、秘密扫描、WSL/Arch/macOS 证据和独立复核后才允许结束长期 goal。

TEST_AND_CLEANUP_CONTRACT
  每个 Rust slice 至少执行 cargo fmt --all -- --check、cargo metadata --no-deps --format-version 1、相关 cargo test --locked -- --test-threads=1、git diff --check；涉及 prompt/agent/registry 执行 refactor-check.sh、arch_dep_check.py、request_shape_probe；涉及黑盒执行对应 testkit。
  一次只跑一个 cargo；WSL 大测试使用 `systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0` 或唯一 CARGO_TARGET_DIR；不要在另一个 agent 正在编译时删除其 target。
  测试完成后删除临时 target、`target/`、隔离 home、临时 venv、测试数据库、fixture 输出、daemon/MCP/child 进程；检查 `find /tmp -maxdepth 1 -name 'yunxi-*'`、进程列表和 `git status --short --branch`。
  隐私门禁：`PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test` 与 `... --repo .`；结果中的个人路径、token、profile、完整私有原文不写进 evidence。

COMMIT_AND_PUSH
  只有 Lead/Verifier 共同确认验证通过后才提交；只 add 施工单允许的明确路径；commit message 必须带阶段/任务号；立即 push `origin codex/yunxi-product-rename`；推送后核对 `git ls-remote` 与本地 SHA。不得 force-push、rebase、reset --hard 或把多个未验证 slice 合成一个提交。

EVIDENCE_SCHEMA
  每个 run 必须记录 run_id、stage/task、commit SHA、UTC 时间、环境（WSL/Arch/macOS 分开）、命令、退出码、稳定计数/耗时、失败原因、未验证项、后续 owner 和清理证明。原始 stdout 只保留脱敏产物；历史摘要必须标 historical，不能冒充当前复跑。

STOP_RULES
  prompt/cache 字节、隐私、权限、迁移、回放、跨域删除、故障恢复、测试或 push 失败，立即停在当前 slice；修复后重新 failing-test→verify→fault/replay→review。
  行号/trait/调用方向与施工单不一致，停止并让 Lead 重定位；不要猜测文件名或创建第二实现。
  任一阶段未退出不得实现其后续阶段；G4-02-01 未通过前停在设计审计，未达到完整长期目标时保持
  goal active，不用“当前阶段通过”宣称产品完成。除额度耗尽或 G0–G9 全部完成外，不得
  调用 goal complete/blocked，也不得自行暂停长期执行。
```

## 3. 当前交接基线（供新模型压缩后恢复）

- 当前分支：`codex/yunxi-product-rename`。
- 当前 HEAD/远端：`444e156a`。
- 最近已推送：`444e156a`（G5-04-02 observer 证据与阶段推进）、`3c4aed3a`（G5-04-02 默认关闭 observer 接入）、`ac04a71e`（G5-04-01 adapter 证据与阶段推进）、`14c627e6`（G5-04-01 raw-free memory admission adapter）、`486994f8`（goal 推进 G5-04 设计）、`2d05575e`（G5-04 memory admission consumer 设计合同）、`4d48f236`（G5-03 退出审计与阶段边界文档）、`d4c0b5d5`（G5-03 stale/consumer 边界文档）、`6c5352ee`（G5-03 stale/consumer invariants）、`3bb11f12`（G5-03 fault matrix regression）、`a941db50`（G5-03 default disabled regression）、`a735444b`（G5-03-D cooperative budget/queue hardening）、`a5f48982`（G5-03-C shadow budget/cancel/queue/replay）、`65f9319d`（G5-03-A/B record-only shadow observer）、`2b458d39`（G5-02 DecisionPort/abstain-only deterministic provider）、`13f1b43b`（G5-00 Laya 来源审计）、`5399b8aa`（G4-05 退出审计）、`2dc988bc`（G4-05-04 重复 revision 收敛）、`fb2f09da`（G4-05-03 文档）、`69698f8d`（G4-05-03 更新锁）；更早证据仍以 Git 历史和阶段文档为准。
- MCP 当前完整套件：37/37；最新 HEAD 代码门禁 `e12ffa74` 已通过，root 504/0/4、base 396/0/6、core 651/0/8、engine 673/0/13、hosts 920/0/10，doctest 全过；当前 HEAD 的 fish/daemon/IPC/REPL/MCP/TUI 黑盒也已有证据；相关临时 target、日志和进程已清理。
- 当前 G0 残余：transfer export 输出/source/SQLite 路径 TOCTOU、锁/磁盘满/权限撤销/组合故障矩阵、跨 SQLite 文件提交非原子性、legacy `state/profile.md` 迁移策略、Arch 实机和 macOS M-series 证据。最新 HEAD workspace 与 fish/daemon/IPC/REPL/MCP/TUI 黑盒已有证据；MCP 权限、既有断连/超时、Unix import 父目录竞态和非法 manifest 已有对应证据，不再重复列为未覆盖项。
- 当前绝不能写成已实现：Laya provider、DecisionPort 消费者接入和 G1-G9 其余业务模块；当前已有 G5-02 的协议/校验器/abstain-only baseline、G5-03-A/B 的无模型 record-only observer 与 G5-03-C/D 的预算/队列/回放 seam，G5-03 已在同步、无模型、无真实消费者边界内退出。

## 4. 启动新 Goal 前的验收

1. 用户删除旧 goal 后，将本文件 `GOAL_COMMAND` 原样作为新 goal objective。
2. 新模型第一轮只读 `AGENTS.md`、本文、evidence index、当前 `git status/log/remote`，不得直接写 G1 代码。
3. 新模型先恢复当前阶段状态表和未决风险，再向用户报告“当前阶段、已证据、下一最小 slice、预计验收命令”，之后才派 GPT-6Astra worker。
4. 若 worker 没有按施工单返回文件/函数/测试/清理/commit/push 证据，Lead 不得采纳其“完成”结论。
