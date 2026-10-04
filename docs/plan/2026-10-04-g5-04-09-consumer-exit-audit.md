# G5-04-09 无模型 consumer 退出审计

日期：2026-10-04  
阶段：G5-04-09  
审计对象：G5-04-01 至 G5-04-08 的 memory admission DecisionPort seam  
当前功能代码基线：`d52bc357`

## 1. 审计结论

G5-04-01 至 G5-04-08 按限定的“无模型、无真实异步 runtime、无生产写入语义变更”
范围完成。memory admission seam 现在具备 raw-free request、默认关闭的 record-only
observer、同步 stale gate、测试内存 consumer 顺序、bounded async/fault/replay 夹具和
test-only metrics/replay 收敛证据。当前可以退出 G5-04-09，进入 G5-05 评测设计；
退出不授权真实 provider、Laya 权重、生产异步调度或生产 consumer 写入。

## 2. 已交付切片与证据

| 切片 | 功能提交 | 证据文档 | WSL 定向计数 |
| --- | --- | --- | ---: |
| G5-04-01 | `14c627e6` | `2026-10-05-g5-04-01-adapter-evidence.md` | memory 98/98 |
| G5-04-02 | `3c4aed3a` | `2026-10-05-g5-04-02-observer-evidence.md` | memory 100/100（1 ignored） |
| G5-04-03 | `b683d750` | `2026-10-04-g5-04-03-metrics-token-evidence.md` | admission 17/17 |
| G5-04-04 | `0e820f54` | `2026-10-04-g5-04-04-stale-gate-evidence.md` | admission 18/18 |
| G5-04-05 | `7a7afb3b` | `2026-10-04-g5-04-05-consumer-harness-evidence.md` | admission 20/20 |
| G5-04-06 | `af0306f2` | `2026-10-04-g5-04-06-async-boundary-evidence.md` | admission 22/22 |
| G5-04-07 | `5daf238b` | `2026-10-04-g5-04-07-fault-replay-evidence.md` | admission 23/23 |
| G5-04-08 | `d52bc357` | `2026-10-04-g5-04-08-metrics-replay-evidence.md` | admission 24/24 |

G5-04-08 的文档同步提交为 `dd31d9d0`；所有提交均已推送到当前公开分支。

## 3. 保持不变的生产不变量

1. `apply_organized_batch` 仍是 memory admission 的唯一生产写入接缝；测试 harness 的
   in-memory apply 不等于数据库写入。
2. 生产 observer 仍固定为 `ShadowMode::Disabled` 且 `provider=None`，默认路径不调用
   DecisionPort provider，不改变 primary envelope、organizer、scheduler 或权限判断。
3. `AdmissionShadowMetrics` 是 caller-owned、非持久化计数器；它不承担跨批次、跨进程或
   跨重启的 exactly-once/replay ledger 语义。
4. replay bytes/digest 只编码稳定的最小 observation；不包含 raw content、token、lifecycle
   私有字段或用户原文。测试夹具按完整 canonical digest 去重，elapsed 不同会得到不同
   digest；这不是生产 dedupe。
5. stale、invalid、cancelled、timeout、unavailable、closed transport 和隐私拒绝不会
   触发 apply 或 completed metrics；实际 privacy gate 返回错误，不伪造为可回放 observation。
6. memory 与 KB、profile、runtime、MCP、TUI/Web、fish、daemon、scheduler 和 voice
   均未因 G5-04 增加第二入口或跨域写入。

## 4. 故障与回放审计

- 同步层：task/scope/fingerprint/diary/database/generation/epoch/mode 任一漂移均被 stale
  gate 丢弃。
- bounded async 夹具：覆盖 fresh/stale、queue full、cancel、timeout、unavailable、
  disconnected/closed 和单次 drain；只在测试线程与内存队列中运行。
- fault/replay：observer fault、RAII queue、privacy gate 和 primary 不变性复用既有
  `decision_shadow` 回归；test-only replay 验证字节稳定、隐私字段排除和完整 digest。
- metrics convergence：同一 canonical observation 的重复 replay 只在测试夹具的
  `HashSet` 中计一次；原始 metrics 仍按调用次数计数，证明两种职责没有混淆。

## 5. 验收门禁与清理

已记录并通过：WSL Ubuntu-24.04 ext4 定向 admission/memory 测试；Windows
`cargo fmt --all -- --check`、`cargo metadata --no-deps --format-version 1 --locked`、
架构依赖检查、隐私自检/仓库扫描和 `git diff --check`。测试使用 disposable checkout
与唯一临时 target，完成后清理 `/tmp/g5-04-08-src`、`/tmp/g5-04-08-target`、临时
进程和构建产物；未留下 `target/`、模型权重、token 或个人数据。

## 6. 明确未完成项（转入后续阶段）

以下内容不能由本审计推断为已实现：

- 真实 Laya/DecisionPort provider、权重下载或模型质量/资源基准；
- 真实异步 runtime、跨进程/跨重启 replay ledger、持久化 exactly-once；
- memory admission 的生产 consumer 写入、KB consumer、recall/rerank、terminal intent 或
  proactive ranking 接入；
- metrics 的生产聚合、并发 snapshot 线性化、数据库/网络重放；
- Arch Linux 实机、macOS M-series 和真实终端长时间运行证据。

## 7. 下一阶段入口

G5-05 只先做评测设计与 deterministic 基线数据集：重要约束保留、闲聊不长期收录、
敏感拒绝、矛盾/低置信 abstain、重复记忆、memory/KB 隔离和意图澄清；评测记录拒绝率、
混淆矩阵、p50/p95、RAM、超时、非法输出和断连回退。未完成 G5-05 的评测合同与验收前，
不得创建真实 provider、下载权重或把 shadow 建议采纳为生产事实。

