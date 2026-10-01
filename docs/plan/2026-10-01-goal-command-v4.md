# YunXi × Miyu 长线 Goal Command v4

> 这是重新启动长线任务时使用的唯一 goal 命令。它保留最初 G0–G9 的产品目标和工程边界，并把决策模型嵌入定义为一条跨阶段主线。旧版 [2026-09-30-yunxi-miyu-long-horizon.md](2026-09-30-yunxi-miyu-long-horizon.md) 保留作 v3 历史审计记录；新 goal 启动后以本文为准。

## 使用方式

1. 在仓库根目录读取本文、`AGENTS.md`、当前工作树和远端分支状态。
2. 将下方 `GOAL_COMMAND` 原样作为新的长期 goal objective。
3. 首次运行从 `CURRENT_STAGE=G0` 开始。没有用户明确授权，不进入 G1；每次阶段退出后停在下一阶段门口。
4. 每次继续时先读取 `STATE`、证据索引和上次提交，不凭对话记忆推断完成度。

## GOAL_COMMAND

```text
GOAL_ID
  YXM-LINUX-NATIVE-V4

REPOSITORY
  root: <repo-root>
  product: sjxbbdb/yunxi-agent-miyu
  base: shorin/miyu
  historical_reference: YunXi-Native
  public_branch: codex/yunxi-product-rename

OBJECTIVE
  在 Miyu 的 Linux 原生底座上逐阶段完成 YunXi 化升级：保留 Miyu 的 fish 接管、常驻 daemon、IPC、TUI、会话/状态、缓存、工具、host ports、MCP 和 Skills，
  以内生方式加入 YunXi 的人格、灵魂、用户画像、关系阶段、分层记忆、独立知识库、陪伴策略，以及可选的 Laya 决策模型；
  最终形成一个常驻 Linux 的陪伴型通用系统级 Agent，让用户用自然语言与 fish 终端交互，由现有工具和权限边界完成可解释的系统操作。

CURRENT_STAGE
  G0

CURRENT_AUTHORIZATION
  只执行 G0 基线冻结、架构审计、兼容回归、证据补齐和必要的既有 bug 修复。
  G0 未退出前不得实施 G1 CompanionContext、G2 profile 迁移、G3 admission、G4 KB 业务扩展、G5 Laya provider、G6 自动总结、G7 新终端路由或 G8 新 TUI。
  G0 只盘点决策模型的挂接 seam、脱敏边界、fallback 和评测合同，不加载 Laya 权重、不新增第二模型链。

PRODUCT_INVARIANTS
  1. fish capture、daemon、IPC、TUI、session/state、prompt/cache、tool/host、MCP/Skills 各只保留一套权威运行时；新模块挂现有 seam，不建立第二 router、daemon、prompt 链、总调度器或 memory store。
  2. memory 与 knowledge base 永远独立：目录、schema、embedding、检索、权限、迁移、删除、恢复和审计都分开；不能通过共享表或混合索引偷换边界。
  3. profile 不向量化；短期记忆只承接当前上下文、语气和短期情景；长期记忆只能经过 transient → short/candidate → committed/rejected/expired 的明确转移后进入向量索引。
  4. `raw_content`、`display_content`、`context_messages` 三分不可破坏；原始用户文本只由既有存储规则保存，工程元数据走 sidecar，不改变 prompt/cache 的 append-only、fossilization、顺序和回放字节。
  5. secret、credential、完整 profile、未授权私密原文和敏感字段不得进入日志、embedding、memory index、KB index 或决策模型输入。
  6. 数据本地优先，必须可查看、导出、删除、恢复；公开仓库不得包含 token、个人档案、绝对本机路径或生成的个人索引。
  7. Linux 是主平台；保留 macOS M-series 的可编译边界。Arch、WSL 和 macOS 的实际证据必须区分记录，不能互相冒充。

DECISION_MODEL_MAINLINE
  DecisionPort 是唯一的决策模型接缝；Laya 只是一个可替换 provider，不是第二个 Agent、router、scheduler、permission layer 或 memory writer。

  D0 / G0 盘点：定位 compact、memory admission、recall/rerank、terminal intent、proactive ranking 的真实 seam；记录输入、输出、owner、fallback 和隐私边界，不加载模型。
  D1 / G3-G4 规则先行：先完成确定性 salience/admission/rerank/intent 规则，规则是关闭模型时的权威基线；模型建议不能成为事实、权限或写入凭证。
  D2 / G5-00 可行性门：核实 Laya 的准确项目来源、版本、artifact/checkpoint SHA、许可证、runtime、CPU/Linux/macOS ARM 部署、中文/Linux 术语质量、p50/p95 延迟和 RAM；未通过时明确记录“模型未部署”。
  D3 / G5-01 协议：定义版本化 `DecisionRequest` / `DecisionResult`，包含 task、schema_version、candidate_ids、scope、input_fingerprint、deadline、capabilities、choice、abstain、reason_code、可选 confidence 和 provider；不把 Laya 类型泄漏进核心领域。
  D4 / G5-02 基线 provider：实现 deterministic provider；模型关闭、缺失、超时、断连、低置信度、非法 JSON、非法 choice/id、越界 score、过期响应时，行为与基线逐字/逐序一致。
  D5 / G5-03 shadow：以可关闭、无副作用、可回放的 ONNX/sidecar/Laya provider 接入；只发送最小化脱敏元数据和必要授权片段；不执行工具、不鉴权/提权、不写删 memory/KB/profile、不改变 scheduler、不主动发消息。
  D6 / G5-04 消费者：按 context salience → memory admission → recall/rerank → terminal intent → proactive ranking 顺序接入；每个消费者有独立开关、预算、评测集、fallback、指标和回滚。
  D7 / G5-05 评测：验证重要约束保留、闲聊不长期收录、敏感拒绝、矛盾/低置信度 abstain、重复记忆、memory/KB 隔离、意图澄清；记录拒绝率、混淆矩阵、p50/p95、RAM、超时、非法输出和断连回退。
  D8 / G5-06 采纳：仅当每个消费者通过质量/延迟门后才启用建议采纳模式；保留可解释 reason、实际影响审计和一键关闭回到 deterministic baseline 的能力。

EXECUTION_LOOP
  对当前阶段重复以下闭环，直到该阶段退出条件全部满足：
  1. LOCATE：读取就近 AGENTS、CodeGraph（若存在）、计划、测试和当前实现；用真实入口、trait、状态、迁移和调用方向定位，不凭文件名猜功能。
  2. SPEC：把需求写成不变量、正向样例、负向样例、回放样例和故障边界；若是决策模型，先写 deterministic baseline 和 fallback。
  3. FAILING_TEST：先添加或确认能暴露问题的最小测试；测试必须绑定真实入口，不能只测日志或 mock 假成功。
  4. MINIMAL_SLICE：按 Miyu 的模块边界实现最小垂直切片；不复制已有运行时，不顺便扩大阶段范围。
  5. VERIFY：按风险运行 cargo fmt、metadata、check、定向单测、集成/PTY/testkit、隐私扫描和回放；Rust 使用 Rust 2021，WSL 中一次只运行一个受 cgroup 限制的 cargo。
  6. FAULT_AND_REPLAY：对本阶段相关的断连、超时、非法输出、半写、重启、权限、锁、删除和恢复执行故障注入；确认失败能回退且无脏状态。
  7. REVIEW：独立复核改动文件、权限边界、数据边界、日志、测试覆盖和残余风险；未验证项明确写入 evidence index。
  8. SYNC：只有验证通过的切片才提交并推送；提交消息包含阶段/任务号；任何代码、测试或文档变更不得只留在本地。
  9. REPORT：报告阶段、任务号、改动文件、真实命令、退出码/计数、失败原因、残余风险和总体进度；“能启动”不能替代验收。

HARD_STOP
  测试、隐私、权限、prompt/cache 字节、迁移、回放、跨域删除或故障恢复一旦失败，停在当前阶段修复并复测。
  已复现的边界破坏不能归类为“未来风险”；只有尚未覆盖、且不违反当前阶段最低合同的硬化项才能登记到后续阶段。
  不能用新增审批状态机、第二运行时、全局向量化、模型强制依赖或虚假 green evidence 绕过目标。

PHASES_AND_GATES
  G0 / 基线冻结与架构审计
    任务：G0-01 入口清单；G0-02 重复运行时；G0-03 数据边界；G0-04 prompt/cache 契约；G0-05 测试矩阵；
          G0-06 改名兼容修复；G0-07 隐私/文档清理；G0-08 MCP/Skills/权限入口；G0-09 fish→daemon→REPL→tool 黑盒闭环。
    允许：只读审计和既有兼容回归修复；D0 seam 盘点。
    退出：无未解释的第二运行时；G1-G9 均有代码落点/测试入口；自然语言终端闭环可复现；
          fmt、metadata、workspace check、基线测试、隐私扫描和证据索引可复现；失败原因、未验证平台和后续 owner 已记录。

  G1 / CompanionContext
    复用 `persona_hint`、`prompt.rs`、`PersonaLane`，加入带 source/version/scope 的最小人格、灵魂、关系上下文；缺失、损坏、过期按确定性默认回退。
    退出：prompt 字节、两轮 cache、fossilization、回放、隐私、缺文件和旧版本测试通过。

  G2 / Profile 与关系
    在现有 state/config 追加 confirmed/inferred、来源、时间、作用域和关系事件；profile 永不进入向量库；迁移追加且可回滚，支持导出、删除、重启、并发和隔离。
    退出：旧数据可读，推断不伪装确认，不同用户/人格/会话隔离，删除与恢复无跨域副作用。

  G3 / 分层记忆
    复用现有 MemoryStore；实现 transient、short、candidate、committed、rejected、expired 的 owner 和 admission；短期承接语气/情景，长期只保存 committed 向量。
    D1 在本阶段先实现 deterministic admission；D6 的 DecisionPort 接缝只做协议测试，不提前加载模型。
    退出：闲聊不增长、敏感拒绝、重复/矛盾处理、撤销/过期/删除正文/向量/关联/缓存/摘要引用、崩溃恢复和限额召回通过。

  G4 / 独立知识库 RAG
    复用现有 KB；先收录 Linux 终端命令，再支持授权私有知识 namespace/source；source、版本、切片、引用、元库和 semantic index 独立于 memory。
    D1 提供规则排序接缝，模型不得扩大可见范围或绕过权限。
    退出：增量更新、重建、失效、断索引、换模型、导入/删除/恢复和跨库隔离通过。

  G5 / DecisionPort 与 Laya
    依 D2→D8 顺序执行；先确定候选模型、许可证和资源门，再实现协议、deterministic provider、shadow、消费者接入、评测和建议采纳。
    退出：关闭模型等价基线；shadow 无副作用；非法/超时/低置信度/断连统一回退；每个消费者质量/延迟/资源门通过；可观察、可解释、一键回滚。

  G6 / 陪伴与自动总结
    复用现有 idle/closing/job owner，不新建 scheduler。空闲总结分别生成短期摘要、长期记忆候选、知识候选、profile 提案和关系事件，各走自己的存储规则；情书/信箱是可插拔能力。
    退出：可取消、不抢前台、重启幂等、去重、用户可拒绝/删除；D6 只参与价值和主动时机排序。

  G7 / 自然语言终端
    复用 fish/daemon/IPC/TUI/tool：理解 → 必要澄清 → 计划/预览 → 既有权限执行 → 解释/回放；普通 shell 语法保持原义，失败可退回 fish。
    退出：WSL/Arch、SIGINT、TTY resize、断连、重启、交互式程序、长 Unicode、危险/覆盖/批量操作和失败回退通过。

  G8 / TUI 原生化
    复用 Miyu 状态机、输入、渲染、通知和布局；YunXi 状态成为原生字段，不叠加外挂面板。
    退出：窄屏、Unicode、ANSI/kitty、滚动、并发重绘、键盘、窗口恢复和必要截图验收通过。

  G9 / 硬化与发布
    建立启动、CPU/RAM、首 token、检索、决策延迟、质量和退化基线；注入锁、损坏、磁盘满、权限、断网、模型缺失、重启、半写、缓存和非法输出。
    完成安装/升级/卸载/恢复、迁移、部署文档、秘密扫描、WSL/Arch/macOS 证据和最终独立复核后，才允许结束整个长期 goal。

EVIDENCE_SCHEMA
  每个 run 必须记录：run_id、stage/task、commit SHA、UTC 时间、环境、命令、退出码、稳定计数/摘要、失败原因、未验证项、后续 owner。
  原始输出只保留脱敏产物；不把个人路径、token、profile、完整私有原文写入仓库。
  历史证据只能标记为 historical；当前复跑必须绑定当前或明确兼容的提交，不得混写。

COMMIT_AND_PUSH
  代码/测试/文档每个验证通过切片立即 `git add` 明确路径、提交并推送当前分支；禁止把多个未验证阶段攒成一个提交。
  推送前至少执行 `git diff --check`、相关测试和隐私扫描；Rust 改动另执行 `cargo fmt` 和相应 cargo 测试。

STOP_AND_HANDOFF
  当前只执行 G0。G0 退出后停在 G1 门口，保留 goal active/paused 状态，等待用户明确授权 G1。
  任何未完成项都写入 evidence index 和 release note，不降低目标、不把登记写成通过、不以阶段进度宣称产品完成。
```

## 当前 G0 交接

- 当前工作树和远端应先重新核对；G0 旧证据见 `2026-09-30-g0-test-matrix.md`、`2026-09-30-g0-architecture-audit.md`、`2026-10-01-g0-evidence-index.md`。
- G0 已有 fish、daemon/IPC、终端组合、REPL、TUI、MCP/Skills、隐私和重复运行时的主要证据，但退出审计仍需按本 v4 的 `EVIDENCE_SCHEMA` 复核。
- Laya/DecisionPort 只在 G0 做 seam 和边界登记；不要因为本文出现 D2–D8 就在 G0 下载或加载模型。
- 重新启动 goal 后第一项工作是读取本文并完成 G0 退出审计，不是直接写 G1 代码。
