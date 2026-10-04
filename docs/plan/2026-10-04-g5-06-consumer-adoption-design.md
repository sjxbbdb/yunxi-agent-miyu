# G5-06 consumer adoption 设计合同

阶段：G5-06-DESIGN。G5-05 只按 deterministic/test-only 边界退出；本合同不等于已接入
真实 Laya/provider，也不授权下载权重或改变生产行为。

## 目标

在 `DecisionPort` 唯一接缝上评估未来 provider 建议是否值得靠近消费者。每个消费者独立
拥有开关、预算、隐私 gate、质量指标、故障回退和审计记录；任何一个消费者失败不得影响
其它消费者或 deterministic baseline。

顺序固定为：

1. context salience（仅影响候选上下文排序）；
2. memory admission（仅建议 admit/reject/abstain，不写库）；
3. recall/rerank（仅建议候选顺序，不返回原文）；
4. terminal intent（仅建议 choice/clarify，不能执行命令）；
5. proactive ranking（仅建议是否值得排队，不能主动发消息）。

## 采纳门

在所有消费者的真实脱敏评测完成前，provider 结果保持 record-only。每个消费者必须同时
满足以下证据才可提出采纳建议：

- 与 deterministic primary 的结果差异有明确 reason、样本和人工可审计影响；
- 敏感输入、profile、raw/display/context 原文和凭据不进入 provider 请求、日志或索引；
- timeout、disconnect、invalid schema、stale fingerprint、privacy rejection、queue full
  均回退到 deterministic，且 primary digest/副作用保持不变；
- p50/p95/p99、超时率、峰值 RAM 和错误率用真实 disposable fixture 测量；未知值保持
  `unavailable`，不得以合成数字填充；
- memory/KB/profile/terminal 权限边界零交叉污染；重复 replay 幂等；
- 可一键关闭该消费者并在下一请求回到 deterministic，关闭不删除已有数据、不改变权限。

本阶段不预设质量阈值；先固定测量协议、基线、样本量、统计方法和失败分类，再由后续
审计决定是否值得采纳。任何“模型更聪明”但无法说明实际影响的结果不得进入采纳建议。

## 允许范围

- 只允许 `#[cfg(test)]` consumer harness、脱敏 fixture、指标/回放证据和设计文档；
- 可复用已有 `DecisionRequest/DecisionResult/ShadowObservation`、memory admission、
  recall/rerank、KB namespace 与 terminal intent 测试；
- 可增加 test-only 的 per-consumer gate/fallback 模拟，不得新增生产 scheduler、网络
  客户端、模型 runtime、数据库迁移或第二 router。

## 禁止范围与停止条件

- 不下载/加载 Laya 权重，不接入真实 provider，不修改生产 `DecisionPort` 调用者；
- provider 不得执行工具、提权、鉴权、写删 memory/KB/profile、改变 fish 语义、调度或
  主动消息；
- 任一隐私、权限、primary 等价、memory/KB 隔离或 replay 幂等失败，立即停在 G5-06；
- 真实 provider 只有在独立采纳审计通过并得到新的施工单后才允许出现。

## 验收与交付

每个 G5-06 slice 必须写明 consumer、文件/测试锚点、输入输出、失败矩阵、统计方法、
WSL 命令、Windows 静态门禁、清理证明和 push SHA。阶段设计完成前，不推进到 provider
实现；长期 goal 继续保持 active。
