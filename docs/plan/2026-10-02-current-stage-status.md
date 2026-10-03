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

## 下一处施工边界

下一施工单继续留在 **G3-05**，不提前进入 G4/G5：

1. 补 provenance 在 concurrent delete/recall 的更强 overlap 语义上的决策
   或测试 seam；当前只证明了并发调用后的 convergence；
2. 对 transcript `run_command` 的变量、命令替换、重定向、symlink/hardlink
   和 `cd` 后相对路径语法做明确的 fail-closed 边界决策；当前 dev 子代理
   前台 fresh registry 已绑定并有 WSL 回归覆盖，dev 面的 read/grep/glob
   仍按既有设计不注册；
3. 只在这些证据通过后，才评估 G3-05 的阶段退出。

现有 read barrier 对未知 legacy carrier 默认拒绝；它对手工拼接的相对 shell
   路径尚未宣称覆盖。不得用“结构化 tool-call 可拒绝”替代任意 shell 语法
   的完整验收。

## 证据与环境约束

Rust 验收以 WSL Ubuntu-24.04 的 ext4 disposable checkout 为准；Windows
DrvFS 只用于编辑和只读检查，不能冒充 Linux 运行证据。每个 validated slice
必须执行定向测试、`cargo fmt --all -- --check`、`git diff --check`、隐私扫描，
清理 disposable checkout/target，并立即提交、推送当前分支。

G4–G9 尚未完成，goal 必须保持 active。
