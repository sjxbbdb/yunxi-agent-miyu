# G5-00 Laya / DecisionPort 来源审计（只读）

日期：2026-10-04  
阶段：G5-00  
性质：只读调研，不下载权重、不新增依赖、不创建 provider、不改变 runtime、memory 或 knowledge base。

## 审计结论

Laya 可以作为后续 `DecisionPort` 的候选建议模型，但当前不能直接接入 YunXi。它是
“一次前向推理、输出 typed decision”的 System-1 模型，不是文本生成器；这与 YunXi
需要的 salience、admission、rerank、terminal-intent 和 proactive-ranking 建议接缝
相符。与此同时，官方说明也明确指出：基础 checkpoint 在 typed-decisions 零样本
任务上接近随机，真正的质量来自领域微调；中文/Linux 终端术语质量、YunXi 自有数据
上的 p50/p95、RAM 和故障回退均没有被本项目验证。因此 G5-00 只通过“来源审计”，
不通过“可接入生产”。下一步必须先完成版本化、可关闭、deterministic-first 的
`DecisionRequest`/`DecisionResult` 合同，再做离线评测。

## 候选来源与可复现锚点

| 候选 | 用途 | 官方来源 | 代码锚点（2026-10-04 查询） | 许可证 | 权重/工件锚点 |
| --- | --- | --- | --- | --- | --- |
| Python 参考实现 | 研究、离线评测、未来可选 sidecar | [NandhaKishorM/laya](https://github.com/NandhaKishorM/laya) | `HEAD=2e4d9c87e8b1621deb344eac7de5c7258f32f849`；`v0.3.25=8a976468b57c1b53541363dc86523263c9b68d34` | Apache-2.0（仓库及模型说明） | [HF model](https://huggingface.co/convaiinnovations/laya)，revision `7b928d828b7b0e022f929d9bd2e44165aa270148`；根模型 LFS OID `891102d372688fc2a094dac56a384bc537b87c63f21f9f3dac0be2b7cbc8d86c`；multilingual LFS OID `9d628fd971b700382ac6f65920a86f149777b2e748e0c955fb3b19695aa8f204`；typed-decisions LFS OID `4fa56de72383a9d3efa9cfa78955733c81b9fc8067a587ca4beb82c78107a24e` |
| Node/TypeScript ONNX wrapper | 未来 Rust/sidecar 边界的参考，不是当前依赖 | [receptron/laya](https://github.com/receptron/laya) | `HEAD=6478649e723122ca24bbf5fb69ed1010023c9750` | wrapper MIT；模型 Apache-2.0，必须分开记录 | [HF ONNX bundle](https://huggingface.co/receptron/laya-onnx)，revision `68f27dfe5a27a54fb2b1fefc432f43f972e90868`；`laya.onnx` LFS OID `a874eb254b58b0fcb1e7ad56fbb188c29d64e08c9a46b689433e1f52c66dba1e`；`laya.onnx.data` LFS OID `487746363a8da57bcadb4345352997d22a0fb90d70aa22c6856668d023242aba` |

上表的 Git 提交、HF revision 和 LFS OID 是来源审计证据，不代表 YunXi 已下载或
缓存这些文件。当前仓库没有把任何模型文件、HF cache 或 Python/Node runtime 带入
版本控制。

## 已核实事实

### 1. 接口形态

- Laya 接收一个 state 和 typed questions，返回 `choice`、`score`、`noul` 等结构化
  决策及概率；它不生成自然语言。
- Python 版本可按语言路由 English、multilingual checkpoint，也可显式选择
  `typed-decisions` checkpoint；
  Node wrapper 通过 ONNX Runtime，不需要 Python/PyTorch 运行时。
- Node wrapper 文档要求 Node 20+，默认首次使用下载约 1.7 GB fp32 ONNX bundle，
  并提示加载后约 2 GB RAM 加 batch 额外开销；其约 140 ms 的 Apple CPU 数值是
  上游示例，不是 YunXi 的实测。
- Python 项目 README 给出 T4 GPU 约 32.8–39.5 ms 的单问题数字、Python 路由和
  多语言 checkpoint；这些是上游 benchmark，不能直接当作 Linux/WSL/Arch 结果。

### 2. 质量与局限

- 上游 typed-decisions benchmark 表格报告领域微调 checkpoint 0.766 accuracy，
  基础 English/multilingual checkpoint 约 0.362/0.342；同一 README 的 Honest
  Limits 段落把 multilingual 写成 0.352，这是上游文档自身的不一致，必须在 YunXi
  评测中重新测量，不能把微调结果当成零样本能力。
- 高基数 choice、短 head budget、ordinal score 和 `noul` 存在已公开的失败模式；
  基础模型可能高置信度地答错，不能用 confidence 单独替代安全策略。
- 中文与 Linux 终端命令的具体表现没有在 YunXi 数据集上测量；即使上游宣称覆盖
  100+ 语言，也不能推断对 shell 语义、混合中英命令名或 YunXi 专属术语可靠。

## YunXi 适配判断

### 允许的后续用途

1. `context_salience`：给候选片段排序或返回 abstain。
2. `memory_admission`：建议 `keep_short / candidate / commit / reject`，最终写入
   仍由 deterministic 规则和现有 MemoryStore 决定。
3. `recall_rerank`：在已通过 scope、tombstone、committed-only 过滤后排序。
4. `terminal_intent`：在已有 fish→daemon→IPC→tool 权限链之前建议意图或澄清，不能
   直接执行命令。
5. `proactive_ranking`：只排序候选陪伴动作，不能主动发消息或改变 scheduler。

### 永久禁止的职责

- 不能执行 tool、调用 MCP、修改 host grant、提权或绕过现有确认/预览边界。
- 不能直接写删 memory、profile、relationship、knowledge base 或向量索引。
- 不能改变 fish 捕获、daemon/IPC、权限、scheduler、prompt/cache 前缀、回放字节。
- 不能把 profile、secret、credential、未经授权私密原文送入决策输入、日志或模型
  cache；输入必须先经过最小化和脱敏。
- 不能成为第二套 router、prompt 链、memory store 或常驻 daemon。

## 未验证项与停止条件

以下项目在 G5-00 仍是未验证项，故不允许进入运行时代码：

- YunXi 真实 workload 的中文/Linux 终端术语准确率、拒绝率、混淆矩阵；
- WSL Ubuntu-24.04、Arch Linux 和 macOS M-series 的 p50/p95、冷启动、CPU/RAM、
  并发与模型卸载行为；
- 固定 HF revision 后的完整文件清单、下载许可和运行时供应链扫描；
- 超时、断连、非法 JSON、未知 choice/id、越界 score、过期响应、模型缺失时，是否
  与 deterministic baseline 逐字/逐序一致；
- 中文敏感信息、个人档案、memory/KB 隔离和终端危险操作上的 abstain 行为。

任一来源、许可证、工件锚点或上述基准缺失时，G5-01 只能继续做合同设计，不能
下载权重、添加 provider 或宣称 Laya 已接入。

## 下一施工单：G5-01

只新增设计文档和契约测试计划，暂不加依赖。必须先固定：

- `DecisionRequest { task, schema_version, candidate_ids, scope, input_fingerprint, deadline, capabilities }`；
- `DecisionResult { choice, abstain, reason_code, confidence, provider }`；
- deterministic provider 为唯一权威基线；模型缺失、超时、断连、非法输出、未知
  id、越界 score、过期响应均回退；
- provider 只读、无副作用、可关闭、可回放，且所有消费者独立开关和预算；
- 只允许最小化脱敏输入，禁止把 Laya/Python/ONNX 类型泄漏进核心领域模型。

G5-01 通过并有真实契约测试后，才允许另立施工单实现 deterministic provider；
再之后才是 shadow provider 和离线评测，模型权重接入仍需单独授权和资源门禁。

## 参考链接

- [Laya Python source](https://github.com/NandhaKishorM/laya)
- [Laya Node/ONNX wrapper](https://github.com/receptron/laya)
- [Convai Innovations Laya model card](https://huggingface.co/convaiinnovations/laya)
- [Receptron ONNX bundle](https://huggingface.co/receptron/laya-onnx)

