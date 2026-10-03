# YunXi × Miyu 长线原生化升级计划

> 该文件是 v3 历史审计记录。重新启动长线 goal 时以 [`2026-10-01-goal-command-v4.md`](2026-10-01-goal-command-v4.md) 为唯一执行合同；v4 保留本文件的 G0–G9 工作范围，并补全 DecisionPort/Laya 决策模型主线。

> 计划编号：YXM-G0
> 版本：v3 / 2026-10-01
> 当前基线：仓库 `sjxbbdb/yunxi-agent-miyu`
> 上游参考：shorin/miyu

这不是一份“把功能加上去”的愿望清单，而是一份可以逐阶段执行、测试、回滚和验收的工程合同。每个阶段都有明确的代码边界、入口、依赖、测试证据和退出条件；当前阶段没有通过时，不进入下一阶段。

## 0. 可执行 Goal 合同（v3）

下面这段是本项目长线 goal 的唯一执行口径。它把“加入一个决策模型”从普通功能需求提升为跨阶段架构主线；任何阶段都不得绕过它另起模型链、调度器或权限链。

```text
OBJECTIVE
  在 Miyu 的 Linux 原生底座上，把 YunXi 的人格/灵魂、用户画像/关系、分层记忆、独立知识库、陪伴与决策模型以内生方式接入，
  最终让用户用自然语言与 fish 终端交互；当前只执行 G0，不进入 G1。

RUN
  locate -> write invariants/failing tests -> minimal slice -> fmt/check/targeted tests
  -> fault injection/replay -> WSL (Arch when available) -> update evidence -> commit/push

STAGE_BOUNDARY
  先完成当前阶段的“可审计闭环”，再进入下一阶段；阶段退出只判断本阶段合同，
  不把后续阶段的业务实现、全量故障硬化或未拥有的硬件环境伪装成当前阶段结果。
  未实现阶段必须至少登记现有挂接 seam、未来代码落点和测试入口；“已登记”不等于“已通过”。
  G0 退出后暂停在 G1 门前，除非用户明确继续，不提前创建 CompanionContext、DecisionPort provider、
  向量 admission、自动总结或新的调度器。

HARD STOP
  任何测试、隐私、权限、回放、迁移或跨域边界失败，都停在当前阶段修复；“能启动”不算通过。
  已复现的边界破坏不能因为属于后续阶段而延后；仅尚未覆盖或未实现的硬化项可登记到所属阶段。

PHASE_CONTRACT
  G0-01~09: 真实入口/单运行时/数据边界/prompt-cache/测试/兼容修复/隐私/扩展权限/终端闭环；
            只读审计和既有兼容回归，后续阶段的实现风险进入风险登记表。
  G1-01: 在 persona_hint/prompt/PersonaLane 上设计最小 CompanionContext，不增加拼装链。
  G1-02: 接入人格与灵魂的来源/版本/作用域，缺文件/损坏/旧版本回退。
  G1-03: 验证请求字节、工具面、化石回放、两轮缓存和隐私；更新阶段证据并推送。
  G2-01: 在现有 profile/state 中记录 confirmed/inferred、来源、时间与关系事件。
  G2-02: 追加迁移、并发/幂等/重启/旧数据读取、导出和删除；profile 永不向量化。
  G2-03: 验证不同用户/人格/会话隔离及撤销恢复，不让推断伪装成用户确认。
  G3-01: 明确 transient/short/candidate/committed/rejected/expired 和每次转移的 owner。
  G3-02: 先用确定性规则筛选未来价值、稳定偏好、项目约束、敏感度；并保留 DecisionPort 消费接缝。
  G3-03: 只有 committed 进入长期向量；短期只承接语气/情景，拒绝/过期候选不入索引。
  G3-04: 验证去重、撤销、过期、删除正文/向量/关联/缓存/摘要引用、旧数据和崩溃恢复。
  G4-01: 复用现有 KB，把 Linux 命令知识的 source/版本/切片/引用和 namespace 明确化。
  G4-02: 保持 KB/memory 的数据库、索引、检索/权限/迁移/删除独立，未来支持授权私有知识。
  G4-03: 验证增量更新、重建、失效、断索引/换模型恢复和跨库隔离；预留排序接缝。
  G5-00~06: 完成下方 DECISION_MAINLINE 的模型验证、端口、基线、shadow、接入、评测和实际启用。
  G6-01: 复用现有 idle/closing/job owner，设计可取消、不抢前台、重启幂等的总结任务。
  G6-02: 分别输出短期摘要/长期记忆候选/知识候选/profile 提案/关系事件，各走自己的存储规则。
  G6-03: 决策模型参与候选价值和主动时机排序；可选陪伴信箱不影响终端主链。
  G7-01: 复用 fish/daemon/IPC 的接管入口，不新增 router/parser；普通命令原语义不变。
  G7-02: 自然语言 -> 必要澄清 -> 现有工具/权限执行 -> 解释/回放；不新增通用审批层。
  G7-03: 验证 SIGINT、TTY resize、断连、重启、交互式程序、长 Unicode 和失败回退 fish。
  G8-01: 用 Miyu 原生状态机/渲染/输入显示人格、运行阶段、记忆/知识引用、决策回退状态。
  G8-02: 验证键盘、窄屏、ANSI/kitty、滚动、并发重绘和窗口恢复，截图补充视觉验收。
  G9-01: 建立启动/CPU/RAM/首 token/检索/决策延迟、质量和退化基线。
  G9-02: 注入数据库锁/损坏、磁盘满、权限/网络/模型缺失、半写、重启、缓存与非法输出。
  G9-03: 完成安装/升级/卸载/恢复、迁移、文档、公开秘密扫描和 WSL/Arch 验收。
  G9-04: 独立复核所有退出条件和未验证项，只有整体目标完成才能结束长线 goal。

G0_EXIT_REVIEW
  G0 退出的最低证据：无未解释的第二运行时/重复入口；G1-G9 各有已定位的 seam、未来落点和测试入口；
  自然语言终端闭环有可复现实验；git diff --check、fmt、metadata、workspace check 和约定的基线测试可复现。
  Arch/macOS 只能标记未验证；transfer 的 openat/renameat、磁盘满、权限组合、非法 JSON 全量矩阵等
  属于 G4/G7/G9 的风险登记，不得为了“关闭 G0”偷偷实现，也不得把已登记风险写成通过。
  G0 退出审计必须列出：证据命令、环境、实际计数、失败原因、未验证项、后续阶段 owner；
  缺任何一项就停在 G0，但不扩大 G0 的业务范围。

DECISION_MAINLINE
  G0: 只盘点 DecisionPort 的 seam、数据脱敏边界、fallback 和评测约束，不加载模型。
  G3/G4/G6/G7: 只保留 deterministic 调用点，不把模型结果当权限/事实。
  G5-00: 验证 Laya 候选的 artifact、许可证、runtime、Linux/macOS ARM/CPU、中文/Linux 评测、延迟/RAM。
  G5-01: 定义版本化 DecisionPort（request/result/choice/abstain/reason/confidence/timeout/capabilities）。
  G5-02: 实现 deterministic provider，关闭模型时行为与基线逐字/逐序一致。
  G5-03: 以 shadow、可关闭、无副作用方式接入 Laya/ONNX/sidecar。
  G5-04: 按 context salience -> memory admission -> recall/rerank -> terminal intent
        -> proactive ranking 逐消费者接入，每个消费者独立开关、预算、回放集和 fallback。
  G5-05: 用脱敏评测集验证拒绝率、混淆矩阵、p50/p95、RAM、超时/非法输出/缺模型/断连回退。
  G5-06: 通过每个消费者的质量/延迟门后启用建议采纳模式；验证实际影响、可解释性和一键退回基线。

DECISION_BOUNDARY
  模型只能返回建议和原因，不能执行工具、鉴权/提权、写删 memory/KB/profile、改变 scheduler 或直接发送主动消息。
  输入为最小化脱敏元数据和必要的授权文本片段，不能只给模型丢失语义的计数。
  secret、credential、完整 profile、未授权原文不得进入日志或模型索引。
  缺模型、超时、低置信度、schema 非法、sidecar 断连统一回退 deterministic；模型不可用不能阻塞终端主链路。

EVIDENCE_AND_SYNC
  每个切片记录阶段/任务号/文件/实际命令与结果/风险/未验证项；不猜完成比例，不把测试零失败写成产品全完成。
  验证通过的切片立即按阶段提交并推送；阶段退出审计与提交切片是两件事，不攒到整个阶段结束才同步。
  独立子任务可委派，写明绝对路径/读写边界/验收/不提交；同一时刻只跑一个受 cgroup 限制的 cargo。
  阶段未通过继续定位、修复、复测；不要通过新增严格审批、第二运行时或虚假完成来绕过用户目标。

NEXT_G0
  1. 校准审计文档：区分 as-built/未来设计，纠正测试覆盖范围与过期计数。
  2. 补 G0-01/02：五类运行时逐项定位 owner、调用方向、搜索范围与复用结论。
  3. 补 G0-04/05：为基线和 prompt/cache 证据记录命令、语言环境、日期、耗时及失败原因。
  4. 完成退出前复跑；当前切片相关测试只跑一次，代码未变时复用同版本已成功证据，
     最终阶段门做一次全量复跑；出现失败只定向复现和修复，不无理由重复全仓。
  5. 独立复核退出条件，输出通过/未通过及证据；更新风险所属阶段，提交推送。
  6. G1-G9 尚未授权实施；保留长线 goal active，明确报出下一阶段而非标记整体完成。
```

当前活动的 Codex goal 已经在运行，goal API 不支持在未结束状态下原地改写 objective；本文件的 v3 合同因此作为可审计、可提交、可复现的细化版本，不通过结束旧 goal 再伪造新 goal 来改变长线状态。每次继续此 goal 时先读取本节，证据从下方链接按任务取用。

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

### 2.1 决策模型主线（跨阶段约束）

决策模型不是另一个 Agent，也不是第二个调度器；它是挂在现有运行时 seam 上的可选建议器。项目中以 `DecisionPort` 表示稳定接口，`Laya` 只是一个可替换 provider 名称。它必须贯穿但不侵入以下决策点：

1. **上下文压缩**：判断哪些上下文值得保留，不能改写 append-only/fossilization 或回放顺序。
2. **记忆准入**：判断 transient/short 事件是否成为 candidate；只有现有 admission 流程明确 commit 后才允许写入长期向量库。
3. **召回与知识排序**：只能对已经通过权限和边界检查的 memory/KB 候选排序，不能跨库合并或扩大可见范围。
4. **终端意图与主动候选**：提供 intent/salience/主动陪伴排序建议，不能直接执行命令、改变权限、写删数据或触发调度。

固定执行合同：先 deterministic baseline，再接入可关闭的 Laya/ONNX/sidecar shadow provider，评测通过后按消费者启用建议采纳模式，不永久停在 shadow。默认关闭时行为必须与基线一致。所有结果都要经过 schema 校验、置信度/超时/非法输出检查；缺模型、超时、低置信度、解析失败统一回退确定性规则。输入使用最小化脱敏元数据及必要授权文本片段，不能为了脱敏让语义判断失效；秘密、凭据、完整 profile 和未经授权的原文不得进入模型日志或模型索引。模型输出只产生判断建议和原因；现有 compact/admission/检索/终端/job owner 负责验证与应用，模型本身不拥有工具、权限、记忆写删或调度权。常规记忆准入不因此增加逐条人工审批，沿用用户的“记住/忘记”语义和既有存储规则。

该主线的实现顺序为：G0 只盘点 seam 与约束；G3/G4 先完成领域规则和 deterministic 接缝；G5 实现 `DecisionPort`、可选 provider、shadow 评测及 context/admission/rerank 的实际接入；G6/G7 再分别实现 proactive/intent 的消费者并复用 G5 验收合同。尚未存在的消费者只做协议测试，不能提前宣称端到端通过。Laya 的具体项目/模型标识、官方来源、可用权重和接口尚待 G5-00 核实，不能预设它能完成所有任务或捏造字段。

**接口与生命周期验收合同**：

- 请求携带任务种类、版本、候选 id、作用域、输入指纹和 deadline；响应使用版本化、任务特定的有限选择与候选 id，包含 provider、reason code、可选置信度及明确的 `abstain`。模型不支持原生置信度时不能伪造或把相似度当概率；由 adapter 显式记录能力并使用已评测的采纳规则。
- 未知 schema 版本、非法 choice/id、非有限或越界 score、缺少必要字段、已超时/取消的响应一律不采纳；取消后不得写入，重放只读已记录的采用结果，不能重新采样改变旧上下文。日志只记录 id、指纹、结果分类、耗时和 fallback reason，不落原文。
- G5-00 输出模型来源/版本与 SHA、许可证、部署依赖、最小 CPU 推理命令、脱敏 fixture 格式、baseline 和资源实测；G5-05 在启用之前写定每项质量/延迟预算并冻结评测集。没有模型、阈值或复现实验，不算通过。
- 必测样例覆盖重要约束保留、闲聊拒绝长期收录、矛盾/低置信度弃权、敏感输入拒绝、重复记忆和领域隔离；另外用同一真实输入验证“模型建议被采纳”和“模型关闭回退”，不是只检查日志字段。阈值来源和样本局限必须记录，不能以全仓单测代替模型质量评测。
- sidecar 只是推理 worker，由现有 daemon/host 管理按需启动、退出、取消、超时、资源上限与重启退避；没有独立终端接管、权限、调度、数据 store 或自主循环。无 sidecar 仍能运行；不为部署模型再造第二个 Agent。

## 3. 阶段任务与执行边界

### G0：基线冻结与架构审计（已完成的历史基线阶段）

**只读为主，不改变业务行为。**本阶段只允许修复审计发现的既有兼容性回归；不得借机开始 G1。

**G0 任务编号与交付物**：

- **G0-01 代码入口清单**：逐项记录 fish capture、daemon、IPC、REPL/TUI、prompt/cache、profile/persona、memory、KB、tools/hosts、MCP/Skills、migration、transfer、scheduler 的真实入口、所有者和调用方向；交付本文件入口表与调用图。
- **G0-02 重复运行时检查**：搜索 daemon、shell router、prompt assembler、scheduler、memory store 的第二实现；每项给出“复用/保留/删除/暂不处理”的结论，不做猜测性重构。
- **G0-03 数据边界清单**：列出 profile、conversation/state、short/long memory、KB、semantic index、credentials、cache 的目录、SQLite 表/索引、embedding 入口、迁移入口、transfer registry 分类和删除/恢复路径。
- **G0-04 提示与缓存契约**：记录 system prompt 组装顺序、append-only/fossilization、动态事实尾、工具字节稳定性、回放测试和敏感信息边界；确认新模块只能挂在现有 seam。
- **G0-05 测试矩阵**：把每个入口映射到定向单测、集成测试、testkit/黑盒脚本、WSL 实测和未来 Arch/macOS 证据；记录命令、环境、耗时、通过/失败和失败原因，交付 [`docs/plan/2026-09-30-g0-test-matrix.md`](2026-09-30-g0-test-matrix.md)。
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
- 受 `systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0` 保护的工作区分批测试最终为：根包 `yunxi` 504/0/4 ignored，`yunxi-base` 395/0/6 ignored，`yunxi-core` 642/0/8 ignored，`yunxi-engine` 628/0/13 ignored，`yunxi-hosts` 919/0/10 ignored；这是改动前的初始基线，doctest 全部通过。后续 G0 复跑结果见 release note 与测试矩阵。
- 初次失败已逐项处理：TUI changed-prefix fixture 与回放编辑断言更新为当前 YunXi 输出；权限位测试改用 WSL 原生临时目录；bundled script 清单与 `douyin-dl` 描述同步；registry fixture 重生成；renderer event 与 tool-summary 断言改为检查本地化语义而非不稳定装饰符号。
- 这些修复只校准测试夹具、环境选择和既有输出契约，没有新增业务能力；`legacy_config_dir` 另补了自定义 root 名称不应选择旧 namespace 的负向回归测试。

#### G0 基线修复记录

产品层改名提交 `7698b643` 将 `legacy_config_dir` 泛化为从 `root_dir.file_name()` 推导命名空间；当测试/迁移构造的 `root_dir` 与真实默认 `config_dir` 不同名时，旧绝对身份路径无法识别，静默回退到旧路径。修复为在真实默认根 `~/.yunxi`/`~/.miyu` 中匹配 `config_dir` 后返回对应 XDG namespace，并保留迁移兼容。该修复不增加新能力，只恢复已有兼容契约；对应 `yunxi-base` 回归测试已通过。

#### G0 退出审计与后续验证

- 工作区编译与主要分批测试已复现；入口、调用方向、所有者、不变量和测试映射见 [`2026-09-30-g0-architecture-audit.md`](2026-09-30-g0-architecture-audit.md)，命令级证据见 [`2026-09-30-g0-test-matrix.md`](2026-09-30-g0-test-matrix.md)。fish、daemon/IPC、终端组合、TUI、transfer 和隐私门禁已有通过记录，当前补充退出复核所需的逐项审计和时间锚点。G3/G4/G7/G9 仍须完成所属领域的删除/恢复、权限组合与系统化故障注入；当前证据不能替代那些阶段的验收。
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

#### G1–G9 实现与验收导航

下表登记未来工作的落点，不提前承诺尚未实现的接口形状。列出的 seam/测试文件已存在；“新增”项须在对应阶段先完成最小设计并验证层序，才能落代码。

| 阶段 | 现有 seam / 拟落点 | 测试入口与需要新增的契约 |
| --- | --- | --- |
| G1 | `yunxi-core/src/persona_hint.rs`；`yunxi-engine/src/agent/prompt.rs` | `agent/tests/{prompt,request_shape,instruction_source,tool_face_cache,context}.rs`；新增缺失/损坏/版本回退与两轮缓存案例 |
| G2 | `yunxi-base/src/config/persona_paths.rs`；`yunxi-core/src/state/migrations/named.rs` 与现有 conversation/state | `state/migrations/tests.rs`、`memory/tests/store.rs`、transfer tests；新增 profile 确认/推断、关系事件、隔离/删除/升级案例 |
| G3 | `yunxi-core/src/memory/`；`yunxi-engine/src/agent/{compact,compact_extras}.rs` | `memory/tests/{store,access,dedup,reset,semantic,ranking}.rs`；新增 admission 状态转移与 committed-only 索引案例 |
| G4 | `yunxi-engine/src/tools/knowledge_base/{store,index,search}.rs`；`default_kb.rs` | KB 模块 tests、`tools/apply_patch/tests.rs`；新增 namespace/source/version、断索引恢复与跨库重建案例 |
| G5 | 消费者：`agent/compact.rs`、memory admission、KB/memory 排序；provider 生命周期复用 host runtime/ports | 拟新增窄 DecisionPort 协议模块及 tests（归属在 G5-01 层序审查后确定）；先 G5-00 来源/CPU/中文/资源评测，再 schema、deterministic 等价、shadow 无副作用和逐消费者采用/回退案例 |
| G6 | `yunxi-hosts/src/runtime/{events,run}.rs`、`platforms/scheduling.rs`；`yunxi-engine/src/tools/jobs/` | `tools/jobs/tests/{lifecycle,output}.rs`、平台 real_context tests；新增 idle 取消/重启幂等、摘要分域、主动建议不抢前台案例 |
| G7 | `yunxi-base/src/shell/fish.rs`；`src/cli/shell_bridge.rs`；host runtime/IPC | fish PTY、`testkit/g0-terminal-combo/run.py`、daemon reload/IPC tests；新增 SIGINT、断连、交互进程和回退案例 |
| G8 | `src/cli/repl/{input,panel,input_layout}.rs` 与现有输出/事件状态 | `src/cli/tests/tui_*.rs`、`testkit/tui/`；新增人格/引用/decision 状态渲染、窄屏/Unicode/并发重绘与截图验收 |
| G9 | 现有 transfer、state/migrations、daemon/IPC、embedding worker 与发布/安装入口 | workspace tests、架构/隐私门禁及现有 testkit；新增锁/损坏/磁盘满/半写/目录竞态、性能报告、安装升级恢复及 Arch/macOS 证据 |

`yunxi-core` 等路径在上表均相对 `crates/`；G5 的新文件路径尚未定，不存在的消费者不能算端到端通过。

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

- **G5-00 可行性门**：先核实 Laya 的准确项目/模型标识、官方来源与版本，不凭名称猜模型能力。记录候选 artifact/checkpoint 与 SHA、接口、许可证、ONNX/runtime、Linux 与 macOS ARM/CPU 部署、最小推理命令、延迟/RAM、中文/Linux 术语评测集；未通过时只保留 deterministic baseline 并明确记录模型未部署。
- **G5-01 DecisionPort 契约**：定义版本化请求/结果、有限 choice、abstain、原因、可选置信度、作用域/输入指纹、超时和 provider 能力；参照 2.1 的校验/生命周期合同，不把 Laya/ONNX 类型泄漏进核心领域。
- **G5-02 deterministic provider**：先把压缩、admission、rerank、intent、主动候选统一接到可测试的确定性 provider，确保关闭模型时基线行为不变。
- **G5-03 shadow Laya provider**：以可关闭、只读、无副作用的方式接入 Laya/ONNX/sidecar；模型只给建议，不能直接执行 shell、改变权限、写删 memory/KB/profile 或触发 scheduler。
- **G5-04 逐消费者接入**：按 context salience → memory admission → recall/rerank → terminal intent → proactive ranking 的顺序接入，每个消费者单独开关、延迟预算、fallback 和回放集。
- **G5-05 评测与故障注入**：建立脱敏中文/Linux 评测集、拒绝率、混淆矩阵、p50/p95 延迟、RAM 峰值和非法输出/超时/缺模型/sidecar 断连回退证据。
- **G5-06 建议采纳与回滚**：逐消费者在质量/延迟门通过后启用实际采纳；context/admission/rerank 在 G5 验证，proactive/intent 随 G6/G7 分别验收。模型实际影响必须可观察，停用即可恢复确定性基线；不能以影子日志代替功能落地。

**验收**：关闭模型时与基线一致；shadow 零副作用；启用建议采纳后保留重要上下文、拒绝无长期价值/敏感记忆的端到端测试通过；无模型可编译/运行；每个消费者的开关、延迟、fallback 与回滚可观测。模型制品无法通过 G5-00 时记录缺口，不能把只有 deterministic 的实现报告成“决策模型已部署”。

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
6. 验证通过的独立切片立即提交并推送当前分支，提交信息包含阶段编号；只有阶段退出条件全部满足才推进下一阶段，失败留在本阶段。

## 5. 阶段状态表

| 阶段 | 状态 | 下一步 |
| --- | --- | --- |
| G0 基线审计 | 已退出（见 `2026-10-02-g0-exit-audit.md`；Arch/macOS 与部分故障注入仍未验证） | 仅维护残余风险，不重复基线劳动 |
| G1 CompanionContext | 已实现并完成退出审计（见 `2026-10-02-g1-exit-audit.md`） | 保留 provider cache hit 与脱敏 request-shape 差异为未验证项 |
| G2 Profile/关系 | 已实现并完成 schema、scope lifecycle、isolation 证据 | 保持 profile 非向量化，后续只修复已登记残余 |
| G3 分层记忆 | 已完成阶段退出：G3-01..04 与 G3-05 provenance/边界证据已固化，跨库原子性、完整 shell parser 等仍是明确非目标 | 只维护已登记残余风险，不把非目标隐性扩张回 G3 |
| G4 独立知识库 | G4-01、G4-02-01 已完成；G4-02-02 调用方接缝进行中 | 审计并实现 ToolCallContext/registry 可信注入，再处理 Web/版本生命周期 |
| G5 DecisionPort/Laya | 未开始（主线已固化，G0 不实现模型） | G3/G4 接口稳定后按 G5-00→G5-06 完成验证、shadow 和建议采纳 |
| G6 陪伴/总结 | 未开始 | G1–G5 的保存入口稳定后 |
| G7 终端自然语言层 | 未开始 | 复用 Miyu 终端路径 |
| G8 TUI | 未开始 | 终端链路稳定后 |
| G9 硬化发布 | 未开始 | 所有功能阶段完成后 |

当前允许执行 G4-02-02 的 ToolCallContext/registry 可信注入设计与失败矩阵；不得把 G4-01 provenance 当作鉴权凭证，
也不能用“能启动”替代阶段验收。
G4–G9 仍未完成，详见 [`2026-10-02-current-stage-status.md`](2026-10-02-current-stage-status.md)。
