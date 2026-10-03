# 当前阶段状态对账（2026-10-02）

本文是长期升级合同的当前状态覆盖记录。它只对账已经落地且可定位的
commit、证据和残余风险，不把单个阶段通过误写成产品整体完成。

## 当前结论

- **G0：已退出。** 退出审计见
  [`2026-10-02-g0-exit-audit.md`](2026-10-02-g0-exit-audit.md)。Arch 实机、
  macOS、部分故障注入和跨 SQLite 文件原子性仍是明确的未验证项。
- **G1：已实现并完成退出审计。** `CompanionContext` 和
  `PersonaSource` 已挂到既有 prompt/request/cache/replay seam；实现提交为
  `dd4c024c`、`b0b8747d`，退出证据为
  [`2026-10-02-g1-exit-audit.md`](2026-10-02-g1-exit-audit.md)。真实 provider
  cache hit 与一份脱敏后的 G0→G1 request-shape 字节差异仍未被声称为已验证。
- **G2：已实现并完成范围隔离审计。** profile/relationship 使用结构化、非向量化
  存储；scope rename/delete 使用 SQLite immediate transaction；证据见
  [`2026-10-02-g2-profile-schema-evidence.md`](2026-10-02-g2-profile-schema-evidence.md)、
  [`2026-10-02-g2-scope-lifecycle-evidence.md`](2026-10-02-g2-scope-lifecycle-evidence.md)
  和 [`2026-10-02-g2-isolation-evidence.md`](2026-10-02-g2-isolation-evidence.md)。
- **G3：进行中。** G3-01 至 G3-04 已有生命周期、admission、committed-only
  vector、embedding 生命周期和 restore barrier 证据；G3-05 已完成 evicted
  turn、summary carrier、transcript identity/write 和当前 transcript read
  barrier、session-delete/idle-sweep GC、transfer round-trip coverage、
  rollback regression、concurrent browse convergence，以及 committed-delete
  后 recall+browse convergence、历史 transcript scope barrier 的最小切片。代码与测试证据已进入当前工作树；状态文档提交可晚于代码证据提交。session
  GC 的基线提交为 `e9bdaf32`，transcript read
   barrier 的基线提交为 `83f763b0`。本轮又将 barrier 绑定到 daemon IPC
   `ToolCall`、直连 CLI `tool`/`tool-call` 与直连 MCP `call_tool` 执行入口；
   `ToolCatalog`、`tools/list` 等只读目录面仍刻意不绑定。新增的重启回归验证
  见 `2026-10-02-g3-05-memory-provenance-plan.md` 的 G3-05-13：提交删除后
  重建 `MemoryStore`，keyword/direct browse/semantic corpus 均隐藏已删除的
  typed carrier，同时保留同文案的无关联 carrier。`reset_all` 的 facts/episodes
  ID 复用窗口也已收窄：删除后保留自增高水位，回归测试确认 tombstone ID 不会
  被 replacement fact 复用；独立 state 清理仍未宣称跨库原子性。
  G3-05-15 又把同一 tombstone barrier 接到 force-import：按 persona scope
  配对 live memory 与 staged evicted-context，导入前清除 typed linked carrier
  的 provenance/embedding/turn，同时保留无关联 carrier；真实 export→import
  回归已在 WSL ext4 通过。G3-05-16 又补上语义召回 embedding await 边界：
  embedding 完成后重新检查 tombstone 与 evicted_turn 存在性，避免删除在
  await 期间提交后仍写入或返回已删除 carrier；定向异步回归与 store 测试
  已在 WSL ext4 通过。G3-05-17 又加入持久化 `tombstone_epoch`：删除、reset、
  session reset、过期清理只在实际写入 tombstone 时递增；evicted browse、直接
  lookup、keyword/semantic recall 使用快照→物化→epoch 复核，变化时有限重试并
  在预算耗尽时 fail-closed。生命周期 8/8、browse 4/4、store 16/16 已在 WSL
  ext4 通过；full core 的 5 个既有 LLM endpoint/error-message 失败与本批
  memory-only 改动无关。严格跨库 overlap 线性化仍未宣称完成。
  G3-05-18 又把 transcript `run_command` 的动态路径缺口收窄：变量、命令替换、
  反引号和重定向只有在文件访问上下文中才 fail-closed，普通 `echo` 的替换仍
  可执行；带路径文件命令、浅层 wrapper/`sh -c`、追加赋值和位置参数也有回归；
  WSL transcript_guard 15/15 通过。它仍是词法屏障，不宣称覆盖任意嵌套 wrapper、
  嵌入脚本或 symlink/hardlink 身份竞态。
  G3-05-19 又补上 `$()`/反引号内嵌的动态文件访问检测：嵌套 `cat "$p/..."`
  被拒绝、嵌套 `printf` 仍放行，WSL transcript_guard 16/16 通过；扫描深度封顶
  8 层，quoted `sh -c`/`eval` 脚本、任意 wrapper data-flow 与文件身份竞态仍未
  宣称覆盖。
  G3-05-20 又补上 transcript 身份边界：结构化 transcript 读取要求 compact/session
  范围内路径无 symlink 且叶文件为 regular file；Unix 上拒绝多硬链接叶文件；
  `run_command` 对已登记的 live transcript 路径直接 fail-closed，避免通用
  `sh -lc` 在 guard 后替换路径。WSL Ubuntu-24.04 ext4 transcript_guard 22/22
  通过，普通文件命令仍放行。G3-05-21 又把一个极窄的 opened-file
  capability 接入 registry→`read` handler：guard 在 provenance、tombstone、
  scope、身份和 Unix `O_NOFOLLOW` 检查后打开只读描述符，handler 复用该描述符，
  叶路径替换回归证明仍返回原始内容；WSL Ubuntu-24.04 ext4
  transcript_guard 23/23 通过。grep/glob 仍是原来的路径/子进程 seam，
  compact 根之外父目录替换、compact 外硬链接 alias、quoted 嵌入脚本与任意
  wrapper data-flow 仍明确未覆盖；compact 外路径的 canonicalize 只作为本地
  alias 探针，不是通用文件系统策略。
  G3-05-22 又对 live provenance transcript 的 `grep`/`glob` 做了保守收口：
  这两个 handler 仍走路径/子进程 seam，暂时拒绝直接搜索并提示改用已有
  descriptor-backed `read`；普通非 transcript 搜索继续放行。新增的双工具回归
  覆盖已登记 live transcript 的拒绝路径；这不是永久取消搜索能力，未来若做
  descriptor-aware search，需另行证明子进程继承和身份语义。
  G3-05-23 补上 staged evicted tombstone fixup 的重复执行回归：同一 staged
  数据库第一次清理后再次执行返回 0，不重复删除，证明导入重试的幂等性；这
  仍不等于跨库原子性或 crash injection 覆盖。
  G3-05-24 又把 descriptor-backed capability 接到单文件 `grep`：live
  provenance transcript 通过 guard 打开的 descriptor 走 `rg` stdin，路径
  替换回归仍返回原始匹配；`include` 在受保护路径上明确拒绝，避免伪造
  filename glob 语义；`glob` 仍拒绝，因为它需要目录能力。WSL
  transcript_guard 24/24 与 grep 定向回归通过。普通非 transcript grep
  继续走原来的路径搜索；父目录替换、目录 glob、复杂 shell wrapper 和
  跨库 overlap 仍未宣称覆盖。

  G3-05-25 已固定本阶段的 overlap 语义合同：采用标准读线性化，最终
  `tombstone_epoch` 相等检查是本次读取的线性化点；有限重试耗尽返回空结果，
  不宣称调用方消费窗口内的严格 post-commit 排除，也不引入跨数据库全局锁。
  该合同要求后续用确定性 seam 覆盖 keyword browse、semantic/hybrid fallback
  与 state-side evicted deletion；合同本身不是 G3-05 完成声明。

  G3-05-26 又补上 quoted shell wrapper 的词法边界：`sh`/`bash`/`dash`/`ksh`/
  `zsh` 的 `-c`、组合短选项和 `--command` payload，以及 `eval`/浅层 wrapper
  的引号内容会作为独立命令递归扫描；动态或不透明 payload fail-closed，
  `sh -c 'printf hi'` 等静态安全 payload 仍放行。WSL transcript_guard 25/25
  通过。任意函数/别名/动态命令名、source/嵌入脚本、文件身份竞态和跨库
  overlap 仍未宣称覆盖。

  G3-05-27 又加入 per-store、仅 `cfg(test)` 的确定性 overlap seam：可在最终
  epoch 检查前和 state-side 删除提交前暂停，不把 sleep、进程级全局锁或调度
  偶合带进生产路径。WSL `memory::tests::browse` 6/6 通过，覆盖 hybrid keyword
  fallback 在墓碑提交后的重试，以及 state-side 删除 overlap 后的后续浏览收敛；
  按 G3-05-25 合同不对已经越过最终检查的 raced result 宣称严格 post-commit 排除。

  G3-05-28 又把同一 seam 接到 semantic provider 的最终 epoch 检查：测试预置
  state-side 向量，在 semantic hits 已物化但尚未完成 epoch 复核时确定性暂停，
  提交关联 fact 删除后释放；第一次 stale semantic 结果被丢弃，有限重试只返回
  仍存活的 unlinked carrier。既有 provider-await 回归与新增最终检查回归在 WSL
  Ubuntu-24.04 ext4 均通过（semantic 2/2）；这仍是标准读线性化证据，不把
  调用方消费窗口扩大成严格 post-commit 保证。

  G3-05-29 又补齐 evicted detail 读取的 overlap 回归：详情读取在最终 epoch
  检查前暂停，主线程提交关联 fact 删除后释放，第一次 raced snapshot 被丢弃，
  重试与后续 detail 读取均返回空。该测试采用同步线程和 per-store seam，避免
  runtime 调度偶合；WSL Ubuntu-24.04 ext4 `memory::tests::browse` 7/7 通过。

  G3-05-30 对 transfer/restore overlap 做了边界审计，决定不把它伪装成在线
  recall seam：`apply_evicted_tombstones` 在导入 staging 阶段读取 live tombstone
  快照，再对独立 staged evicted DB 做一次事务清理，之后才进入安装/rollback
  流程。现有 `force_import_filters_evicted_carriers_by_persona_scope`、
  `evicted_tombstones_remove_the_whole_linked_carrier`、重复 fixup 幂等和两条
  rollback 回归已覆盖其实际契约；在这里加入“并发删除后必须严格排除”的测试会
  偷渡跨 SQLite 文件原子性。该边界保留为 transfer/crash exit 前置项，不改生产
  协议，也不宣称 G3-05 已退出。

  G3-05-31 又补上 staged fixup 的提交前失败回归：生产入口仍传入 no-op，测试在
  本地 SQLite commit 前注入一次失败，确认 staged 的 provenance/embedding/turn
  行全部回滚、live tombstone 数据库字节不变，随后正常重试仍删除 3 个关联
  carrier。WSL Ubuntu-24.04 ext4 `transfer::fixups::tests` 5/5 通过。该证据覆盖
  本地事务失败语义；真实进程级 crash 时序与跨库原子性仍不作声称。

  G3-05-32 又补上真实子进程 crash-after-delete 回归：父测试启动精确的
  Unix child test，child 在 fact tombstone transaction 提交后立即 abort，故意
  不执行 state-side carrier 清理；父进程随后重新打开同一数据库，确认
  keyword、browse、semantic corpus 都隐藏已删除关联 carrier，同时保留未关联
  carrier，且 tombstone 仍持久存在。WSL Ubuntu-24.04 ext4
  `memory::tests::store` 19/19 通过。这覆盖进程消失后的 tombstone read barrier，
  不等于跨数据库原子性或 staged transfer 的 OS crash 保证。

  G3-05-33 又收窄了 transcript shell 的一个真实词法缺口：POSIX `source` 与
  `.` 是文件访问 builtin，却不在原有命令表中，导致 `source "$p/fold.md"`
  这类动态路径在修复前误放行。先用新增负例复现 baseline 失败 1/1，再把两者
  纳入现有 `FILE_ACCESS_COMMANDS`；WSL Ubuntu-24.04 ext4
  `tools::transcript_guard` 25/25 通过。该切片仍是有限词法屏障，不宣称完整
  shell parser，也不覆盖嵌入语言脚本的任意 data-flow。

  G3-05-34 补上 staged transfer 的提交后 crash 回归：仅 cfg(test) 的
  `after_commit` seam 严格位于本地 SQLite `tx.commit()` 成功之后；Unix child
  随即 abort，父进程重开 staged DB，确认 provenance/embedding/turn 三张表均
  保留未关联 carrier、删除 carrier 不复现，live tombstone DB 字节不变，正常
  重试返回 0。WSL Ubuntu-24.04 ext4 `transfer::fixups::tests` 7/7 通过。该
  证据覆盖 staged 本地提交后的进程消失，不把 live/staged 两个 SQLite 文件
  扩大成跨库原子性保证。

  G3-05-35 收窄了 transcript shell 的 `xargs` data-flow 缺口：`xargs` 从
  stdin 或 replacement placeholder 取得参数，随后驱动 `cat`/`grep`/`sed`
  等文件访问命令时，即使命令文本没有 `$()`、反引号或重定向，也必须按
  opaque 动态路径 fail-closed；`xargs printf` 这类非文件消费者继续放行。
  覆盖直接、管道输入、`sudo` wrapper 和 `--` 选项，WSL
  `tools::transcript_guard` 26/26 通过。该切片仍是有限词法屏障，不宣称
  完整 xargs 参数/函数别名/嵌入脚本 data-flow。

  G3-05-36 又收窄了 `find` 执行谓词的 transcript data-flow 缺口：
  `find -exec`、`-execdir`、`-ok` 与 `-okdir` 会把匹配到的路径交给嵌套
  命令，因此对 `cat`/`cp`/`grep`/`sh -c` 等文件消费者按 opaque 动态路径
  fail-closed；不带执行谓词的 `find` 与直接 `find -exec printf` 继续放行。
  既有 wrapper 词汇也纳入扫描，WSL Ubuntu-24.04 ext4
  `tools::transcript_guard` 27/27 通过。该切片仍是有界词法屏障，不宣称
  完整 find grammar、shell parser、process substitution 或嵌入脚本
  data-flow。

  G3-05-37 又收窄了 Bash process substitution 的 transcript data-flow 缺口：
  对未引号的 `<(...)`/`>(...)` 做有界括号扫描；外层文件消费者会把其中的
  opaque descriptor 当成动态路径，producer body 也递归经过既有 `find`/`xargs`/
  shell/变量屏障，未闭合括号 fail-closed。普通 `echo <(printf hi)` 仍可用，
  引号或转义文本保持字面量。WSL Ubuntu-24.04 ext4
  `tools::transcript_guard` 29/29 通过。该切片仍不宣称完整 Bash grammar、函数/别名、
  嵌入脚本或父目录 TOCTOU。

  G3-05-38 又收窄了 `xargs` 外层 wrapper 的选项绕过：`sudo --`、`sudo -n` /
  `--non-interactive`、`env --` 以及多层 `sudo env --` 现在会继续走到
  `xargs` 专用 opaque data-flow 检查；未知或疑似带参数的 wrapper 选项直接
  fail-closed，不把选项参数误当成安全命令。新增拒绝与 `xargs printf` 对照回归
  在 WSL Ubuntu-24.04 ext4 的 `tools::transcript_guard` 29/29 通过。该切片仍
  不宣称完整 sudo/env/wrapper CLI grammar。

  G3-05-39 补上 legacy summary 的退出矩阵证据：通过旧的
  `replace_visible_with_summary` API 写入无 `memory_provenance` 的摘要，并让
  摘要正文与随后删除的 fact 完全相同；删除后 checkpoint 仍保留原摘要且不出现
  redaction marker，证明 unknown/legacy carrier 不会按文本猜测被清理。WSL
  Ubuntu-24.04 ext4 `agent::tests::context` 38/38 通过；transfer rollback 的
  linked/unlinked 组合随后由 G3-05-40 的最小 backup-import 入口补齐。

  G3-05-40 补上 transfer backup-import 的最小 provenance 矩阵：
  `marker_failure_backup_import_preserves_linked_and_unlinked_provenance` 让
  `--force` import 在 marker stamping 失败前生成 backup，确认失败后 live
  tree 回滚，再把该 backup 通过真实 import 入口导入空 home；summary/transcript
  provenance、带 provenance 的 linked evicted carrier 和同文本但无 provenance
  的 unlinked carrier 均保持。WSL Ubuntu-24.04 ext4 `transfer` 48/48 通过，
  并通过 `cargo fmt --all -- --check`。该证据只覆盖 backup/archive 与文件系统
  rollback，不扩大为 live/staged SQLite 跨库原子性或 OS crash 保证。

  G3-05-41 对 reset/association/compact/restore 接缝做了只读审计：通过 WSL
  Ubuntu-24.04 ext4-backed disposable checkout 的权威运行，
  `memory::tests::reset` 6/6、`memory::tests::association` 4/4、
  `memory::tests::embedding_lifecycle` 11/11、`state::tests::compact` 25/25、
  `state::tests::redo` 6/6 与 `agent::tests::context` 38/38 全部通过。现有
  证据确认 tombstone-aware association、typed summary redaction、compact
  undo/reset provenance 清理和 redo 既有恢复合同；没有发现需要改生产逻辑的
  缺口。该轮计入 WSL ext4 权威证据，但仍未证明把 session reset、association、compact 与 undo/redo 串在一个
  真实 fixture 中的统一矩阵，普通历史 tool-report 文本仍按 append-only 策略
  保留；跨 SQLite 原子性与 OS crash 安全不在本轮保证内。

  G3-05-42 新增统一接缝回归
  `agent::tests::context::session_reset_association_compact_undo_keeps_deleted_memory_out`：
  通过真实 `Agent::wipe_session_memory`、`MemoryStore::association`、typed
  summary refs、checkpoint redaction、`StateStore::undo_last_turn` 与已有 redo
  revision API 串起 reset→association→compact→undo→redo，确认删除后的 fact
  不再召回且 provenance carrier 在 history restore/revision rewrite 后清理。
  WSL Ubuntu-24.04 ext4 disposable checkout
  执行 `cargo fmt --all -- --check` 与该测试定向命令，结果 1/1 通过。此证据
  不覆盖 transfer/backup-import、跨 SQLite 原子性、OS crash 或普通 historical
  tool-report 的 free-text scrub。

  G3-05-43 明确 transcript unknown-wrapper 边界：在既有
  `find_exec_file_access_dataflow_is_denied` 中加入
  `sudo --unknown find . -exec cat {} \;`，验证未知 wrapper 场景下嵌套的
  opaque file consumer 仍被保守拒绝。该改动只新增回归字面量，不改生产扫描器；
  WSL Ubuntu-24.04 ext4 组合运行
  `tools::transcript_guard` 29/29 通过。函数/别名、动态命令名、嵌入脚本与
  父目录 TOCTOU，以及通用 wrapper option walker 的完整 CLI 语义，仍是明确边界。

  G3-05-44 补上 `StateStore::reset_conversation` 的非空 provenance 直接回归：
  `state::tests::compact::reset_cleans_transcript_carriers_without_deleting_files`
  先写入一个 typed `memory_provenance` 与一个 `transcript_carriers`，再执行
  reset，确认两张表均清空且 transcript 文件语义仍只由数据库索引控制，未触碰
  文件本体。WSL Ubuntu-24.04 ext4 的 `state::tests::compact` 25/25 通过；这
  只补齐 reset 的证据，不扩大为 `reset_persona_contexts`、跨 SQLite 原子性或
  OS crash 保证。

  G3-05-45 又补上 `StateStore::reset_persona_contexts` 的作用域回归：
  `state::tests::sessions::persona_reset_clears_memory_and_transcript_provenance_for_targets_only`
  通过生产 compact API 为目标人格与非目标人格各写入 typed summary/transcript
  provenance，执行按 persona/platform 的 reset 后确认目标两类索引均清空、非目标
  两类索引仍保留。WSL Ubuntu-24.04 ext4 的 `state::tests::sessions` 17/17
  通过；这仍不扩大为跨 SQLite 原子性或 OS crash 保证。

  本阶段逐条退出前对账见
  [`2026-10-02-g3-05-exit-audit.md`](2026-10-02-g3-05-exit-audit.md)；该文档明确
  已证明路径、跨库/legacy 未证明路径和下一施工顺序，不能当作 G3-05 完成声明。

## 下一处施工边界

下一施工单继续留在 **G3-05**，不提前进入 G4/G5：

1. 按 G3-05-25 合同继续补 provenance 在 concurrent delete/recall 的语义
   覆盖；确定性 seam 已覆盖 keyword/hybrid fallback、semantic final-epoch
   provider overlap、evicted detail 与 state-side browse 已有确定性证据；
   transfer/restore 不增加伪在线 overlap seam；staged fixup 的提交前失败、
   重试与 memory-side 子进程 crash-after-delete 已有证据，仍需完成最终退出
   审计，并继续明确真实 staged transfer crash 与跨库原子性不在本阶段保证内；
2. 对 transcript `run_command` 剩余的任意 wrapper data-flow 和嵌入脚本做
   明确的 fail-closed 边界决策；quoted `sh -c`/`eval`、变量、命令替换、重定向、
   `cd` 后动态相对路径、带路径命令名、浅层 wrapper/`sh -c`、追加赋值、位置
   参数、`xargs` opaque stdin/placeholder、`find -exec` opaque matched-path
   data-flow、process substitution 及 `$()`/反引号内嵌动态文件访问已有词法屏障与
   29/29 回归，live
   transcript shell 路径也已拒绝；legacy summary 的文本不再作为 memory 关联
   依据；当前仍不做完整 shell parser，未知 wrapper
   CLI grammar 与动态命令名继续按边界处理；
3. 继续评估 `grep` 的 `include` 过滤与 `glob` 的目录能力是否值得进入
   descriptor/openat/dirfd 设计，并单独决定父目录替换是否值得覆盖；当前审计
   已确认 `ensure_transcript_identity` 与后续 open 之间存在可复现的父目录
   TOCTOU，叶级 `O_NOFOLLOW` 不能闭合它，因此本阶段不把父目录替换写成已
   覆盖能力；完整闭合需要 Unix 目录 FD 链/openat/fstatat 及 Windows/macOS
   独立实现。继续审计任意 wrapper data-flow 与跨库 overlap，再评估 G3-05
   的阶段退出；
   `read` 与单文件 `grep` 的叶文件 capability 已落地，
   但不等于整段完成；

此外，transfer 的 staged fixup 目前仍依赖导入前的 tombstone 过滤，尚未把
epoch 递增语义扩展到独立的 staged 数据库；这不是运行时在线读写路径，已审计
为不应伪造在线 epoch，仍需专门的 transfer/crash regression 后再决定是否扩展。

现有 read barrier 对未知 legacy carrier 默认拒绝；它对手工拼接的相对 shell
   路径尚未宣称覆盖。不得用“结构化 tool-call 可拒绝”替代任意 shell 语法
   的完整验收。

## 证据与环境约束

Rust 验收以 WSL Ubuntu-24.04 的 ext4 disposable checkout 为准；Windows
DrvFS 只用于编辑和只读检查，不能冒充 Linux 运行证据。每个 validated slice
必须执行定向测试、`cargo fmt --all -- --check`、`git diff --check`、隐私扫描，
清理 disposable checkout/target，并立即提交、推送当前分支。

G4–G9 尚未完成，goal 必须保持 active。
