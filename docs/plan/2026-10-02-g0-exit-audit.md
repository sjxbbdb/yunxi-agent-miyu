# G0 退出审计记录（2026-10-02）

状态：`G0` 退出审计完成，等待用户确认；尚未进入 `G1`。

本记录只判断 G0 的最低合同，不把后续阶段能力写成已完成。测试证据中的临时构建目录统一使用脱敏占位符，不保留本机用户路径。

## 1. G0-01 ～ G0-09 判定

| 任务 | 当前判定 | 证据 |
| --- | --- | --- |
| G0-01 inventory | 通过 | 架构审计、层序门禁和当前源码入口映射 |
| G0-02 duplicate runtime | 通过 | `G0-20261002-blackbox-head-01`：fish → daemon → IPC → REPL → tool 闭环 |
| G0-03 data boundary | 通过 | profile prompt-only、Memory/KB 双向删除隔离、KB 回滚证据 |
| G0-04 prompt/cache contract | 通过 | 既有 request-shape、cache/fossil/replay 证据；本阶段未改提示词组装 |
| G0-05 test matrix | 通过（残余已登记） | `G0-20261002-workspace-head-01`、`G0-20261002-blackbox-head-01`、M-18～M-26 |
| G0-06 rename compatibility | 通过 | 产品层命名审计、架构依赖门禁、公开树扫描 |
| G0-07 privacy/docs | 通过 | `testkit/privacy/g0_scan.py --self-test` 与 `--repo .`：personal_path/credential/private_key/unreadable 均为 0 |
| G0-08 MCP/Skills/permissions | 通过 | MCP request-shape、连接池故障、权限矩阵和 persistent blackbox 证据 |
| G0-09 black-box | 通过（平台范围有限） | fish 17/17、REPL、terminal-combo、daemon 6/6、MCP 5/5、TUI 16/16 |

## 2. 已登记但不应伪装成“已修复”的残余

这些项目已有 owner 和后续阶段，不再作为重复测试理由，但在对应阶段完成前仍保持公开风险：

1. transfer export 的输出/source/SQLite 路径级 TOCTOU：G9 hardening；Unix import 父目录竞态已有 M-21 修复。
2. 锁、磁盘满、权限撤销和组合故障矩阵：G9 hardening。
3. 两个 SQLite 文件之间的跨库提交原子性：G4/G9 hardening，当前实现明确不宣称跨库事务。
4. legacy `state/profile.md` 迁移策略：G2 profile migration。
5. Arch Linux 实机和 macOS M-series：G9 release validation；WSL 结果不替代这两类环境。

## 3. 阶段闸门

G0 的证据和残余 owner 已具备退出审计条件，但合同要求：

- 未经用户确认，不进入 G1；
- 不在 G0 加载 Laya 权重或实现 G1～G9 业务模块；
- 长线 goal 保持 active，不能把 G0 阶段通过写成整个项目完成。

用户确认后，下一步是进入 G1 `CompanionContext`，先完成 source/version/scope 与 prompt 字节契约，再委派一个受限实现切片。

## 4. 当前仓库证据

- 分支：`codex/yunxi-product-rename`
- 当前 HEAD/远端：`d2401f29`
- 最近修复：证据临时路径脱敏、交接基线同步、隐私门禁恢复为通过
- 工作树：干净

