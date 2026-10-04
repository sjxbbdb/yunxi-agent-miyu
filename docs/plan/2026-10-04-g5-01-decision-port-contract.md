# G5-01 DecisionPort 契约设计

日期：2026-10-04  
阶段：G5-01  
前置：G5-00 来源审计及修订已推送 `cfd5c757`
状态：设计合同；本文件本身不创建 provider、不加载 Laya、不增加依赖。

## 1. 目的与范围

`DecisionPort` 是 YunXi 唯一的决策建议接缝。它把“某个消费者需要从有限候选中
做一个可审计判断”表达成稳定的 request/result，而不把 Laya、ONNX、Python、Node
或任意模型类型泄漏进核心领域。它不是 Agent、router、scheduler、permission layer
或 memory writer。

本阶段只固定：

1. 请求、结果、schema 版本与输入指纹；
2. 有限候选、作用域、能力和 deadline 的约束；
3. deterministic baseline 和 provider 结果校验；
4. 超时、断连、非法结果、旧结果和隐私违规的回退矩阵；
5. 可回放的契约测试向量。

本阶段不做：模型下载/加载、Laya provider、sidecar、HTTP 服务、memory/KB schema
修改、消费者接入、TUI/Web 改动、权限或 scheduler 改动。

## 2. 领域词汇

| 名称 | 约束 |
| --- | --- |
| `DecisionTask` | 固定枚举：`context_salience`、`memory_admission`、`recall_rerank`、`terminal_intent`、`proactive_ranking`。未知任务直接拒绝。 |
| `CandidateId` | 调用方分配的短、稳定、不含秘密的 ID；结果只能引用 request 中的 ID。 |
| `DecisionScope` | `conversation`、`session`、`memory`、`knowledge_base`、`terminal_turn`、`companion_job`；scope 只作隔离标签，不授予能力。 |
| `DecisionProvider` | `deterministic`、未来的 `shadow_laya` 或其他可替换实现；provider 是建议来源，不是权限来源。 |
| `ReasonCode` | 机器可读的稳定枚举；面向用户的解释仍由现有 YunXi 层生成。 |
| `Abstain` | 明确“不足以判断”，不是把低置信度偷偷转换成某个候选。 |

## 3. 版本化请求

逻辑字段如下；Rust 结构体和 JSON 名称必须在实现时保持 snake_case。字段增删必须
增加 `schema_version`，禁止静默改变旧字段含义。

```json
{
  "schema_version": "yunxi.decision.v1",
  "task": "memory_admission",
  "candidate_ids": ["short", "candidate", "committed", "reject"],
  "scope": "memory",
  "input_fingerprint": "sha256:…",
  "deadline_ms": 80,
  "capabilities": ["rank_only", "abstain"],
  "payload": {
    "redacted_text": "…",
    "turn_kind": "user_message",
    "existing_state": "candidate"
  }
}
```

### 请求不变量

- `schema_version` 必须是本实现支持的精确版本；未知版本不尝试猜测。
- `task`、`scope` 和 `candidate_ids` 必须非空；候选 ID 不得重复，候选数量有硬上限，
  由实现常量固定并在错误中报告。
- `input_fingerprint` 是对**脱敏、规范化、字段顺序固定**的 `payload` 和关键请求
  字段做 SHA-256 后的值。原始私密文本、token、profile 全文不能作为 fingerprint
  输入，也不能写入日志。
- `deadline_ms` 是从调用开始计算的单调预算，不使用可回拨的墙上时钟；零值表示
  立即回退，不表示无限等待。
- `capabilities` 是允许 provider 做什么的声明，最小集合只有 `rank_only`、
  `choice_only`、`abstain`。任何 `execute_tool`、`write_memory`、`write_kb`、
  `change_permission`、`schedule_job` 等能力均非法，不能被调用方或模型添加。
- payload 只允许任务所需的最小化元数据；调用方在进入 `DecisionPort` 前完成 secret、
  credential、完整 profile 和未经授权私密内容的剥离。

## 4. 版本化结果

结果使用单一 envelope；同一响应只能选择一种 outcome。排序结果只引用已请求的 ID，
不允许 provider 引入新候选。

```json
{
  "schema_version": "yunxi.decision.v1",
  "task": "memory_admission",
  "input_fingerprint": "sha256:…",
  "outcome": {
    "kind": "choice",
    "candidate_id": "candidate",
    "confidence": 0.81
  },
  "abstain": false,
  "reason_code": "provider_choice",
  "provider": "deterministic",
  "elapsed_ms": 1
}
```

允许的 `outcome.kind`：

- `choice`：唯一 `candidate_id`，只用于有限选择；
- `ranking`：`ordered_ids` 是 request 候选的无重复子序列，未列出者视为同分尾部；
- `score`：仅用于调用方明确声明的有界数值，必须同时给出 `min`/`max`；
- `abstain`：没有可接受的建议，必须给出非空 `reason_code`。

结果校验要求：`schema_version`、`task`、`input_fingerprint` 必须与 request 一致；
choice/ranking 不能引用未知 ID；confidence 和 score 必须是有限数且在声明范围内；
`abstain=true` 时不能同时携带可执行选择；`elapsed_ms` 只作诊断，不能成为权限依据。

## 5. 原因码与回退

以下原因码是 v1 的最小集合，新增原因只能向后兼容地增加枚举：

`provider_choice`、`provider_ranking`、`deterministic_baseline`、`empty_candidates`、
`invalid_request`、`unsupported_schema`、`timeout`、`cancelled`、`provider_unavailable`、
`invalid_result`、`unknown_candidate`、`stale_fingerprint`、`low_confidence`、
`privacy_rejected`、`capability_violation`、`internal_error`。

| 故障 | 对外结果 | 是否允许继续调用模型 |
| --- | --- | --- |
| provider 缺失、进程断连、加载失败 | deterministic baseline 或 abstain；记录 `provider_unavailable` | 同一请求不重试 |
| deadline 到期/取消 | deterministic baseline 或 abstain；记录 `timeout`/`cancelled` | 不等待迟到响应 |
| JSON/schema/数值解析失败 | deterministic baseline 或 abstain；记录 `invalid_result` | 不采纳部分字段 |
| 未知 candidate、重复 ID、越界 score | deterministic baseline 或 abstain；记录 `unknown_candidate`/`invalid_result` | 不自动修正 ID |
| fingerprint 不一致或响应过期 | 丢弃结果并回退；记录 `stale_fingerprint` | 必须由新 request 重新计算 |
| 低置信度或模型明确 abstain | deterministic baseline 或 abstain；记录 `low_confidence` | 不把 abstain 转成猜测 |
| 隐私/能力校验失败 | fail closed；记录 `privacy_rejected`/`capability_violation` | 禁止重试原始 payload |

deterministic baseline 是唯一权威行为。provider 关闭、模型缺失和所有上述故障下，
消费者的输出顺序、写入资格、权限判断和回放字节必须与模型从未存在时一致。

## 6. 生命周期与权限边界

1. 消费者构造 request → 脱敏/规范化 → 计算 fingerprint。
2. `DecisionPort` 校验 request，选择已启用 provider；provider 只读、不可执行工具。
3. 结果回到 `DecisionPort` → schema/候选/scope/fingerprint/deadline 校验。
4. 校验失败或低置信度回到 deterministic baseline。
5. 消费者依据自己的既有规则决定是否采用建议，再执行现有 memory/KB/tool/permission
   流程；DecisionPort 不写任何状态。
6. 记录最小化审计字段（task、scope、provider、reason_code、fingerprint、耗时、
   fallback 标记），禁止记录原始 payload。

`DecisionPort` 不能拥有 `MemoryStore`、`KnowledgeBase`、host grant、MCP pool、
fish executor 或 scheduler 的写句柄；未来实现若无法证明这一点，应停在设计阶段。

## 7. 契约测试向量

G5-02 实现前必须把下列向量固化为无模型、无网络的测试：

### 正向

- 固定候选和 payload 得到稳定 fingerprint；deterministic provider 返回同一 choice。
- ranking 只返回候选子序列；候选顺序改变会改变 fingerprint 并拒绝旧结果。
- 明确 abstain 的结果保留 abstain 和 reason，不伪造 choice。

### 负向

- 空 task/scope/candidate、重复或过长 ID、未知 schema、未知 scope 均被拒绝。
- 结果引用未知 ID、重复 ranking、NaN/Infinity、越界 score、错误 task/fingerprint
  均回退。
- payload 中出现 token、私钥、完整 profile 或未授权原文时 privacy gate 拒绝，且
  测试输出不泄漏原文。

### 故障与回放

- provider 延迟超过 deadline、提前取消、断连、返回半截 JSON，结果均可重放为同一
  deterministic baseline。
- 迟到的旧 fingerprint 响应在新 request 后永远不能覆盖新结果。
- 关闭 provider 前后，消费者输出、memory/KB 写入资格和权限判定字节相同。
- 同一 request 重放不产生第二次写入、scheduler 事件或工具调用。

## 8. 实施顺序与停止条件

G5-02 只实现上述数据类型、校验器和 deterministic provider；允许的源码范围暂定为
新增窄的 `yunxi-core` 决策协议模块及对应 tests，禁止修改 consumers。通过契约测试、
`cargo fmt`、metadata、WSL ext4 定向测试、privacy scan 和清理后，才进入 G5-03
shadow provider 设计。任何 provider 让模型结果影响权限、工具、写删数据、scheduler
或主动消息，立即退回当前阶段。

