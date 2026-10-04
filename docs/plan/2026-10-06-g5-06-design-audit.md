# G5-06 DESIGN 审计与未退出项

日期：2026-10-06  
当前阶段：G5-06-DESIGN  
结论：不退出，不进入真实 provider

## 已有证据

| 切片 | 已证明 | 证据 |
| --- | --- | --- |
| G5-05 deterministic 评测 | 七类行为、primary 等价、memory/KB 隔离、失败分类和脱敏摘要 | `2026-10-04-g5-05-exit-audit.md` |
| G5-06-01 | 五类 consumer × 八类情况的独立 record-only/fail-closed 回放 | `2026-10-06-g5-06-01-adoption-gate-evidence.md` |
| G5-06-02 | 独立开关、zero-deadline、privacy、audit-off 不串扰 | `2026-10-06-g5-06-02-gate-matrix-evidence.md` |
| G5-06-03 | 真实评测 manifest、oracle、统计、RSS、故障和清理协议 | `2026-10-06-g5-06-03-measurement-protocol.md` |
| G5-06-04 | 任一采纳证据缺失时只能 record-only | `2026-10-06-g5-06-04-evidence-readiness-evidence.md` |

## 尚未满足的采纳门

以下项目不是“测试夹具通过”可以替代的，当前全部保持 `unavailable` 或 `not_run`：

1. 没有真实 provider revision、许可证确认后的可执行 runtime 和脱敏输入输出样本；
2. 没有五类 consumer 的真实质量分母、人工影响审计或可解释的 mismatch 价值判断；
3. 没有真实 provider seam 的 p50/p95/p99、timeout rate、invalid rate、disconnect rate；
4. 没有真实 provider 进程的 peak RSS/RAM 证据；编译内存和 deterministic fixture 不能代替；
5. 没有跨重启的生产 audit ledger，也没有任何生产 consumer 可以一键关闭回 deterministic；
6. 没有授权把 provider 结果写入 memory、KB、profile、terminal intent 或主动消息路径。

因此不能把 `AdoptionEvidence::complete_for_fixture` 当作模型采纳资格，也不能把
G5-06-01/02 的 fake match 当作模型质量。

## 下一施工单（仍在 G5-06）

下一切片必须先得到明确的 provider 来源/许可证/revision 与独立采纳授权，然后在
disposable WSL process 中执行测量协议；首批仍只 record-only，不接任何生产 consumer。
施工单必须包含：provider seam、五类样本分母、单调时钟与 RSS 采集命令、失败注入、
审计输出字段、清理方式和回滚证明。若任一输入隐私、权限、primary 等价或 replay
幂等条件失败，停止在 G5-06，不推进 G6。

## 状态不变量

- `ShadowMode::default()` 仍为 `Disabled`；
- deterministic primary 仍是唯一事实、权限和副作用来源；
- 当前没有 Laya 权重、真实 provider、网络客户端、生产 scheduler 或第二 router；
- 长期 goal 保持 active；G5-06 未退出前不得实现 G6/G7/G8/G9 业务代码。

