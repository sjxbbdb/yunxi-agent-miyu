>更新内容记录在此处，每次更新release时作为releasenote发布，发布后清理已发布内容。

## 重要更新

- G4-04：默认知识库更新/导入增加失败阶段与恢复状态、全量预检、状态备份回读；dashboard 索引状态识别旧 revision/旧模型，避免 partial snapshot 或旧向量伪装成 fresh。
- G4-05（第一切片）：optimized checkout/validate 失败回退旧 revision，source cache 使用 staging 交换，default namespace 导入失败按 namespace 恢复并清理 orphan，不覆盖 user semantic DB。
- G4-05-02：重索引达到最大重试轮次仍有 rerun marker 时进入 `exhausted`，保留 marker 供重试；dashboard 暴露轮次与 queued/exhausted 状态，避免把未收敛任务显示为成功。
- G4-05-03：默认知识库更新、启动导入、检查和通知共用跨进程 `flock` lease；第二更新请求快速返回稳定 busy 错误，状态写入在锁内重新读取，避免并发覆盖。
- G4-05-04：同一来源 revision 与快照 hash 且默认文件内容一致时走 no-op；内容、revision 或文件集合变化仍进入完整替换，避免重复更新破坏有效快照。
- G5-00：完成 Laya/DecisionPort 只读来源审计，固定 Python/Node-ONNX 来源、Git/HF revision、模型 LFS OID 与许可证分界；明确零样本质量、中文/Linux 术语、资源和故障回退仍未验证，未下载权重、未接入 provider。
- G5-00 follow-up：修正 Python 仓库 `v0.3.25` tag SHA，明确 Router 只按语言自动选择 English/multilingual，补录上游 multilingual benchmark 文案差异；同步当前远端 HEAD `13f1b43b`。
- G5-01：固化版本化 `DecisionRequest`/`DecisionResult` 契约、fingerprint、candidate/scope/capability 校验和 deterministic fallback 矩阵；下一阶段只实现无模型 provider，不加载 Laya。
- G5-02（已完成，`2b458d39`）：已在 `yunxi-core` 建立无模型、无副作用的版本化协议、
  fingerprint/隐私/预算校验与 abstain-only deterministic provider；WSL decision 8/8
  通过。Laya/shadow provider 和消费者接入仍未实现，下一阶段进入 G5-03 设计。
- G5-03（限定边界已完成，`d4c0b5d5`）：在 `a5f48982` 的无模型、同步、record-only shadow
  observer 上补齐 `ShadowQueue` 原子容量/RAII permit、有效 deadline、协作式
  `ShadowCallContext`、完整错误映射、replay golden 向量，并固化 `ShadowMode` 默认关闭；
  fault matrix 已覆盖 precheck 主摘要、queue RAII、ranking/score 与非法结果，WSL
  shadow 30/30、decision 回归 39/39；另有同步 stale fingerprint 与 consumer-probe
  不变性回归。真实消费者、调度或 Laya 权重尚未接入；退出审计见
  `docs/plan/2026-10-04-g5-03-exit-audit.md`。下一阶段进入 G5-04 consumer admission
  前置设计，仍不加载 Laya 权重。
- G5-04-DESIGN：首个 DecisionPort consumer 固定为 memory admission；设计
  `2026-10-05-g5-04-memory-admission-design.md` 明确 request allowlist、primary-only
  fallback、lifecycle/KB 隔离、异步迟到 token 边界与无模型测试矩阵，尚未修改运行时代码。
- G5-04-01：推送 `14c627e6`，新增 raw-free memory admission adapter 与三态/force 一致性
  回归；WSL memory 98/98 通过。尚未接入 `apply_organized_batch`、真实 provider 或模型。
- G5-04-02：推送 `3c4aed3a`，在 `apply_organized_batch` 的 admission 单点接入默认关闭
  record-only observer；WSL memory 100/100 通过（1 ignored），生产仍为 provider=None。
- G5-04-03：推送 `b683d750`，增加 admission shadow 的 started/completed、11 类结果计数、
  有界 p50/p95/p99 延迟快照，以及只由已校验 envelope 构造的不可序列化 stale token；
  WSL admission 17/17 通过，默认 Disabled 与生产写入路径不变。异步 runtime、真实
  provider 与 Laya 权重仍未接入；证据见 `docs/plan/2026-10-04-g5-04-03-metrics-token-evidence.md`。
- G5-04-04：推送 `0e820f54`，增加 memory admission 的同步 stale-response 门禁；task、
  scope、fingerprint、diary、database、generation、epoch、mode 任一漂移均返回
  `StaleFingerprint`，不触发 callback、数据库、organizer 或 scheduler。WSL admission
  18/18、Windows 静态门禁和清理通过；仍未接入真实异步 provider、Laya 权重或生产消费者，
  证据见 `docs/plan/2026-10-04-g5-04-04-stale-gate-evidence.md`。
- G5-04-05：推送 `7a7afb3b`，新增仅测试夹具内的 memory admission consumer drain；固定
  `validate_result → stale gate → in-memory apply` 顺序，fresh fake response 恰好应用一次，
  invalid/stale 只丢弃，生产路径不变。WSL admission 20/20、Windows 静态门禁和清理通过；
  证据见 `docs/plan/2026-10-04-g5-04-05-consumer-harness-evidence.md`。
- G5-04-06：推送 `af0306f2`，在测试夹具内增加 bounded `sync_channel(1)`、Barrier 和短命
  线程，验证跨线程 fresh/stale、queue-full、取消、timeout、unavailable、关闭和单次
  drain；WSL admission 22/22、Windows 静态门禁和清理通过。真实异步 runtime、模型和
  生产 consumer 仍未接入，证据见 `docs/plan/2026-10-04-g5-04-06-async-boundary-evidence.md`。
- G5-04-07：推送 `5daf238b`，补齐 test-only fault/replay 与 closed transport 边界；新增
  metrics/replay 稳定性、私有字段排除和 mpsc disconnected 断言。实际 observer fault、
  privacy gate、RAII queue 与 primary 不变性沿用既有回归；WSL admission 23/23、Windows
  静态门禁和清理通过，证据见 `docs/plan/2026-10-04-g5-04-07-fault-replay-evidence.md`。
- G5-04-08：推送 `d52bc357`，增加 test-only replay digest 收敛夹具；相同 canonical replay
  只计一次，elapsed 变化产生新 digest，并验证 metrics snapshot、latency bucket、隐私
  字段和 primary 等价。WSL admission 24/24、Windows 静态门禁和清理通过，证据见
  `docs/plan/2026-10-04-g5-04-08-metrics-replay-evidence.md`。
- G5-04-09：完成无模型 consumer 退出审计，汇总 G5-04-01 至 G5-04-08 的提交、证据、
  WSL/Windows 门禁、故障与回放边界和清理证明。确认生产 observer 仍为
  `ShadowMode::Disabled + provider=None`，测试夹具没有变成生产 consumer；真实 Laya、
  provider、异步 runtime、持久化 replay ledger 和 Arch 实机证据明确转入后续阶段。退出
  审计见 `docs/plan/2026-10-04-g5-04-09-consumer-exit-audit.md`，下一阶段进入
  G5-05 deterministic 评测设计。
- G5-05-DESIGN：固定脱敏评测样本协议、deterministic oracle、重要约束/闲聊/敏感拒绝/
  矛盾低置信/重复记忆/memory-KB 隔离/意图澄清七类行为，以及 confusion matrix、p50/p95、
  RAM、超时、非法输出和断连回退指标。第一批只允许 `#[cfg(test)]` 夹具与证据文档，
  不创建 provider、不下载 Laya、不改变生产写入；合同见
  `docs/plan/2026-10-04-g5-05-evaluation-design.md`。
- G5-05-01：推送 `1af476ca`，新增 test-only deterministic admission matrix，覆盖项目
  约束、闲聊、敏感拒绝、稳定偏好和 force-sensitive rejection，并验证两次回放结果与
  primary envelope 一致；WSL admission 25/25、Windows 静态门禁和隐私扫描通过。recall/
  rerank、terminal intent、memory/KB 隔离、故障 fallback 和指标摘要仍待后续切片，证据见
  `docs/plan/2026-10-04-g5-05-admission-evaluation-evidence.md`。
- G5-05-02：推送 `19dfc883`，在 `decision_shadow` 测试区增加 fallback matrix，覆盖
  invalid/timeout/unavailable/closed/privacy 分类，验证 primary digest 不变且隐私拒绝不
  调用 provider；WSL decision_shadow 31/31、Windows 静态门禁和隐私扫描通过。仍未接入
  真实 provider/异步 runtime，证据见
  `docs/plan/2026-10-04-g5-05-decision-fallback-evidence.md`。
- G5-05-03：复用既有隔离/重复/semantic 回归完成评测：KB 30 passed/2 ignored、memory
  dedup 6 passed、semantic 5 passed/1 ignored；确认 namespace、删除恢复、memory/KB
  边界与无 ONNX fallback 没有交叉污染。该批无生产代码变更，证据见
  `docs/plan/2026-10-04-g5-05-isolation-evidence.md`。
- G5-05-04：推送 `72d801df`，补充 TerminalIntent/TerminalTurn deterministic abstain
  回归；decision 10/10 通过，确认 baseline 不执行命令、不产生澄清副作用。该批仍不实现
  fish/权限/真实 intent consumer，证据见
  `docs/plan/2026-10-04-g5-05-terminal-intent-evidence.md`。
- G5-05-05：推送 `df4b0f36`、`9f5e1713`，新增统一 deterministic 评测摘要（test-only），记录七类
  行为混淆矩阵、各 consumer 的 p50/p95/p99、超时/断连/非法输出/closed transport 回退、
  primary 等价、memory/KB 零交叉污染和敏感写入为零；RAM 保持 `unavailable`，不宣称
  性能达标。证据见 `docs/plan/2026-10-04-g5-05-evaluation-summary-evidence.md`。
- G5-05-06：复用当前 HEAD 的 recall/rerank 与 store 生命周期回归，ranking 4/4、store 19/19
  通过；覆盖 recall fatigue、truth/confidence 排序、tombstone、恢复、principal 过滤与
  profile prompt-only 边界。证据见 `docs/plan/2026-10-04-g5-05-recall-rerank-evidence.md`。

- 完成第一轮产品层 YunXi 化迁移：主程序、Rust workspace crate、TUI/Web 文案、资源、提示词、memes 与 Linux 打包入口统一使用 YunXi 命名。
- 保留 Miyu Fork 的来源、许可证和必要兼容读取，并明确区分历史参考仓库 `YunXi-Native` 与当前产品基线 `yunxi-agent-miyu`。
- 新增 `docs/YUNXI-PRODUCT-BACKGROUND.md`，固化 Miyu Linux 底座、YunXi 陪伴层、记忆/知识库边界及候选 Laya 决策层的产品背景。

## 修复

- G4-03 knowledge-base source revision lifecycle：默认知识库快照写入来源 revision，
  语义召回过滤旧 hash/namespace/revision 向量；embedding 等待期间若源被更新或删除，
  旧任务不会覆盖新索引，并会排队重建。默认维护 capability 收窄为 `default-kb`，
  WSL KB 25/25 与 apply_patch 18/18 通过。

- G3-05 tombstone epoch read barrier：memory 数据库新增可迁移的
  `tombstone_epoch`，真实删除/reset/过期清理在同一事务递增；evicted browse、
  keyword/semantic recall 在快照与状态物化之间进行 epoch 复核，变化时有限重试，
  避免删除提交后在 await 或跨库读取窗口重新暴露 typed carrier。WSL 定向
  lifecycle 8/8、browse 4/4、store 16/16 通过；严格跨库线性化与 crash 注入仍未
  宣称完成。

- G3-05 transcript dynamic-path barrier：`run_command` 在文件访问上下文中对
  未解析变量、命令替换、反引号和动态重定向 fail-closed，同时保留普通
  `echo "$(...)"`；带路径文件命令、浅层 wrapper/`sh -c`、追加赋值和位置参数
  也已 fail-closed，WSL transcript_guard 15/15 通过。任意嵌套 wrapper、嵌入脚本
  和 symlink/hardlink 竞态仍未宣称覆盖。

- G3-05 nested transcript dynamic-path barrier：递归检查有界的 `$()`/反引号
  子命令，嵌套动态文件访问 fail-closed，非文件 `printf` substitution 仍可用；
  WSL transcript_guard 16/16 通过。quoted `sh -c`/`eval` 脚本、任意 wrapper
  data-flow 和 symlink/hardlink 竞态仍未宣称覆盖。

- G3-05 transcript identity boundary：结构化读取要求 compact/session 范围内无
  symlink、叶文件为 regular file，Unix 上拒绝多硬链接叶文件；`run_command` 对
  已登记的 live transcript 路径 fail-closed，避免通用 `sh -lc` 在 guard 后替换
  路径。WSL transcript_guard 22/22 通过。

- G3-05 opened transcript capability：registry 通过本地 opaque
  `ToolCallContext` 将 guard 打开的只读 transcript descriptor 传给结构化
  `read` handler；Unix 叶文件使用 `O_NOFOLLOW`，路径替换回归证明 handler
  仍读取原始内容。普通文件行为保持不变。WSL Ubuntu-24.04 ext4
  transcript_guard 23/23 通过。grep/glob 的路径/子进程 seam、父目录替换、
  compact 外硬链接 alias、quoted 嵌入脚本与任意 wrapper data-flow 仍未宣称
  覆盖。

- G3-05 transcript search boundary：由于 `grep`/`glob` 仍走路径/子进程 seam，
  `glob` 仍对 live provenance transcript fail-closed；单文件 `grep` 已改为
  通过 guard 打开的 descriptor 走 stdin，叶路径替换不会切换搜索内容。受保护
  路径的 `include` 暂时明确拒绝，普通非 transcript 搜索继续放行；目录能力、
  父目录替换与跨库 overlap 仍未宣称覆盖。

- G3-05 staged evicted fixup idempotence：对已清理的 staged evicted-context
  数据库重复执行 tombstone fixup 返回 0，保证导入重试不会重复删除或改变
  结果。该回归不宣称跨库原子性或 crash injection 已完成。

- G3-05 memory provenance：删除提交后的 `MemoryStore` 重建会继续隐藏已删除的
  evicted carrier；`reset_all` 保留 facts/episodes 的自增高水位，避免独立 state
  清理窗口中的 tombstone ID 复用。新增重启与 ID 复用回归测试，WSL store 测试
  15/15 通过；G3-05 的跨库崩溃注入、严格 overlap 线性化和完整异步语义覆盖仍未
  宣称完成。
- G3-05 import barrier：force-import 会按 persona scope 将 live memory tombstone
  传递到 staged evicted-context，删除关联 carrier 的 provenance、embedding 和
  turn，避免旧归档复活已删除记忆；真实 export→force-import 回归与独立 fixup
  回归均在 WSL ext4 通过。
- G3-05 async semantic barrier：语义召回在 embedding await 完成后重新检查
  tombstone 与 `evicted_turns` 存在性，删除在等待期间提交时不会再写入或返回
  已删除 carrier；定向异步回归与 store 测试在 WSL ext4 通过。严格跨库
  overlap 线性化和 crash 注入仍属于后续边界。

- G3-05 overlap contract：本阶段固定采用标准读线性化。读取以
  `tombstone_epoch` 最终相等检查作为线性化点，变化时有限重试，预算耗尽
  fail-closed；不宣称调用方消费窗口内的严格 post-commit 排除，也不引入跨库
  全局锁。后续回归会用确定性 seam 覆盖 keyword browse、semantic/hybrid
  fallback 与 state-side evicted deletion。

- G3-05 quoted shell-wrapper barrier：`run_command` 现在把 `sh`/`bash` 等
  `-c`、组合短选项、`--command` 以及 `eval`/浅层 wrapper 的 quoted payload
  作为独立命令递归扫描；动态或不透明 payload fail-closed，静态 `printf` 等
  安全 payload 保持可用。WSL transcript_guard 25/25 通过。函数/别名、动态
  命令名、嵌入脚本、文件身份竞态与跨库 overlap 仍是后续边界。

- G3-05 deterministic overlap seam：记忆测试加入 per-store、仅
  `cfg(test)` 的暂停点，可在最终 epoch 检查前和 state-side 删除提交前确定性
  交错操作；WSL `memory::tests::browse` 6/6 通过。覆盖 hybrid keyword fallback
  的墓碑重试与 state-side 删除后的后续收敛，不把已越过最终检查的 raced result
  误写成严格 post-commit 保证，也没有引入运行时全局锁或固定 sleep。

- G3-05 semantic final-epoch overlap：在同一 per-store 测试 seam 上补齐语义
  召回的最终 epoch 检查交错回归；预置 state-side 向量后暂停、提交关联 fact
  删除并释放，验证 stale linked carrier 被丢弃、bounded retry 只返回 live
  carrier。既有 provider-await 与新增 final-epoch semantic 回归在 WSL ext4
  通过 2/2；仍不宣称调用方消费窗口内的严格 post-commit 排除。

- G3-05 evicted detail overlap：详情读取在最终 epoch 检查前暂停并交错提交
  关联 fact 删除，验证 raced snapshot 与后续详情读取都收敛为空；WSL
  `memory::tests::browse` 7/7 通过。测试只使用同步线程和 per-store
  `cfg(test)` seam，不新增运行时锁或跨库原子性承诺。

- G3-05 transfer/restore boundary audit：确认 staged import 的 tombstone
  fixup 是一次 live 快照 + 独立 staged DB 事务，不是在线 recall seam；保留
  force-import、幂等和 rollback 现有证据，不伪造跨 SQLite 原子性。专门的
  transfer/crash regression 仍是 G3-05 退出审计前置项。

- G3-05 staged fixup failure-before-commit：在本地 SQLite commit 前注入一次
  可控失败，验证 staged provenance/embedding/turn 全部回滚、live tombstone
  DB 不变，正常重试仍可清理 3 个 carrier；WSL
  `transfer::fixups::tests` 5/5 通过。真实进程 crash 时序与跨库原子性仍未
  被声称覆盖。

- G3-05 process crash-after-delete：Unix child 在 fact tombstone transaction
  提交后立即 abort，父进程重开数据库并验证 keyword/browse/semantic corpus
  都隐藏已删除关联 carrier，同时保留未关联 carrier；WSL
  `memory::tests::store` 19/19 通过。该证据只覆盖 memory-side tombstone
  read barrier，不覆盖 staged transfer 的 OS crash 或跨数据库原子性。

- G3-05 transcript parent-directory boundary：ext4 竞态审计确认叶级
  `O_NOFOLLOW` 无法防止 identity check 与 open 之间的父目录替换；完整修复
  需要 descriptor-relative `openat`/`fstatat` 链和跨平台实现，本阶段不伪造
  部分能力。

- G3-05 shell lexical barrier：补上 POSIX `source` 与 `.` 两个文件访问
  builtin；新增负例先在 baseline 复现误放行，修复后 WSL
  `tools::transcript_guard` 25/25 通过。该切片仍不宣称完整 shell parser 或
  嵌入语言脚本 data-flow 覆盖。

- G3-05 staged transfer crash-after-commit：Unix child 在 staged SQLite
  `tx.commit()` 成功后立即 abort，父进程重开数据库并验证 provenance/
  embedding/turn 三张表不会复活已删除 carrier，live tombstone DB 不变，
  重试返回 0；WSL `transfer::fixups::tests` 7/7 通过。该证据不扩大为跨库
  原子性或后续安装/回滚文件系统 crash 保证。

- G3-05 transcript `xargs` data-flow：`xargs` 从 stdin/placeholder 取得参数
  并驱动文件访问命令时 fail-closed，覆盖直接、管道、`sudo` wrapper 与
  `--` 选项；`xargs printf` 仍放行。WSL `tools::transcript_guard` 26/26
  通过；仍不宣称完整 xargs grammar、函数/别名或嵌入脚本 data-flow。

- G3-05 transcript `find` data-flow：`find -exec`、`-execdir`、`-ok` 与
  `-okdir` 将匹配路径交给 `cat`/`cp`/`grep`/`sed`/`sh -c` 等文件消费者时
  fail-closed；普通 `find` 与直接 `find -exec printf` 仍放行，并覆盖已有
  wrapper 词汇。WSL `tools::transcript_guard` 27/27 通过；仍不宣称完整
  find grammar、process substitution、嵌入脚本 data-flow 或父目录 TOCTOU
  闭合。

- G3-05 transcript process-substitution data-flow：识别未引号的 Bash
  `<(...)`/`>(...)`，文件消费者把其 opaque descriptor 按动态路径处理，
  producer body 递归复用既有 `find`/`xargs`/wrapper 屏障，未闭合括号
  fail-closed；普通 `echo <(printf hi)` 与引号/转义字面量继续可用。WSL
  `tools::transcript_guard` 29/29 通过；仍不宣称完整 Bash grammar、嵌入脚本
  data-flow 或父目录 TOCTOU 闭合。

- G3-05 transcript xargs-wrapper data-flow：`sudo --`、`sudo -n` /
  `--non-interactive`、`env --` 与多层 `sudo env --` 不再绕过 `xargs`
  opaque path barrier；未知 wrapper 选项 fail-closed，`xargs printf` 对照仍
  可用。WSL `tools::transcript_guard` 29/29 通过；仍不宣称完整 wrapper CLI
  grammar、动态命令名或嵌入脚本覆盖。

- G3-05 legacy summary provenance：旧 `replace_visible_with_summary` 路径生成的
  无 provenance 摘要，在删除同文案 fact 后仍可读且不出现 redaction marker，
  防止按文本猜测 memory 关联。WSL `agent::tests::context` 38/38 通过；transfer
  普通 rollback 的 linked/unlinked 全路径仍待专门入口。

- G3-05 transfer backup-import provenance：新增
  `marker_failure_backup_import_preserves_linked_and_unlinked_provenance`，验证
  `--force` 安装在 marker 失败后留下的 backup 可通过真实 import 入口恢复，
  summary/transcript provenance、linked evicted carrier 与同文案 unlinked carrier
  均保持。WSL transfer 48/48 通过；不宣称跨 SQLite 原子性或 OS crash 安全。

- G3-05 reset/association/compact/restore audit：通过 WSL Ubuntu-24.04
  ext4-backed disposable checkout 做权威重跑：memory reset 6/6、
  association 4/4、embedding lifecycle 11/11、state
  compact 25/25、redo 6/6 与 agent context 38/38。确认 tombstone-aware
  association、typed summary redaction、compact undo/reset provenance 清理和
  redo 恢复合同均有组件级证据；未新增生产逻辑，也未把分散证据扩大成单一
  session-reset→association→compact→restore 矩阵、跨 SQLite 原子性或 OS crash
  安全保证；source/target 已在测试后清理。

- G3-05 unified reset/association/compact/undo/redo fixture：新增
  `session_reset_association_compact_undo_keeps_deleted_memory_out`，用真实
  `Agent::wipe_session_memory`、association、typed summary refs、checkpoint
  redaction、`undo_last_turn` 与 state revision redo API 串起删除后不召回的
  接缝；WSL Ubuntu-24.04 ext4 定向测试 1/1 通过。该 fixture 不扩大为
  transfer/backup-import、跨 SQLite 原子性、OS crash 或普通历史 tool-report
  free-text scrub 保证。

- G3-05 transcript unknown-wrapper deny boundary：在既有 `find -exec` data-flow
  回归中加入 `sudo --unknown find . -exec cat {} \;`，确认未知 wrapper 场景下
  嵌套 opaque file consumer 仍被保守拒绝；仅新增测试字面量，不改生产扫描器。
  WSL Ubuntu-24.04 ext4 `tools::transcript_guard` 29/29 通过。函数/别名、动态
  命令名、嵌入脚本、父目录 TOCTOU 与完整 wrapper CLI 语义仍未宣称覆盖。

- G3-05 reset provenance direct regression：扩展
  `reset_cleans_transcript_carriers_without_deleting_files`，通过生产压缩 API
  同时写入 typed `memory_provenance` 与 `transcript_carriers`，执行
  `StateStore::reset_conversation` 后确认两类索引均清空、文件本体不被触碰。
  WSL Ubuntu-24.04 ext4 `state::tests::compact` 25/25 通过；
  `reset_persona_contexts`、跨 SQLite 原子性与 OS crash 仍未宣称覆盖。

- G3-05 persona reset provenance scope regression：新增
  `persona_reset_clears_memory_and_transcript_provenance_for_targets_only`，通过
  生产 compact API 为目标/非目标人格会话写入 typed summary/transcript provenance，
  执行 `reset_persona_contexts` 后确认只清理目标作用域。WSL Ubuntu-24.04 ext4
  `state::tests::sessions` 17/17 通过；跨 SQLite 原子性与 OS crash 仍未宣称覆盖。

- G0 基线稳定化：修复 YunXi 产品改名后 legacy config namespace 的正/负路径兼容，校准 TUI/replay/renderer/tool-summary 的当前产品输出夹具，修正 bundled script 与 registry fixture 漂移，并让 WSL 权限位测试使用原生 Linux 文件系统临时目录。
- G0 验证：WSL Ubuntu-24.04 工作区单线程测试在 v4 当前提交上重新全绿（根包 yunxi 运行 508 项、yunxi-base 396、yunxi-core 651、yunxi-engine 与 yunxi-hosts 均完成且 0 failed；doctest 全部通过）。本结果对应 G0 evidence index 的 `G0-20261001-workspace-02`；Arch Linux 与 macOS M-series 仍未实机验证。
- G0 计划补充 Skills/MCP 生命周期、host 权限真相源、raw/display/context 三分、长期记忆删除不可召回、memory/KB 数据边界、Laya 可行性门、自动总结独立写入协议，以及不新增 YunXi 通用审批状态机的约束。
- G0 MCP 启动失败隔离：健康 MCP 不会因另一服务器启动失败而消失，健康 listing 在重复 registry 构建中复用缓存；MCP 28/28 定向测试、启动隔离 1/1 通过。
- G0 黑盒夹具兼容：`repl-smoke` 跟随当前 `YUNXI_HOME/home/<member>/conversation.db` 布局并保留 legacy fallback；TUI 配置表单在临时 pyte 0.8.2 环境复跑 16/16，daemon orphan 6/6、MCP 持久生命周期 5/5。
- G0 生产黑盒身份链：在提交 `8f8af396` 上重新构建非 testkit `yunxi 0.7.0`，SHA-256 为 `707c8c15517f7c4f8546dd27f620ca4df99535bcdcbf8438ca14cda17b5e8d3b`；daemon orphan 6/6、MCP persistent 5/5 均使用该二进制并确认无残留进程。
