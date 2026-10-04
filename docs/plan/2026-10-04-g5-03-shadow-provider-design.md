# G5-03 Shadow Decision Provider 设计合同

日期：2026-10-04  
阶段：G5-03  
性质：设计合同；当前实现仍无模型、无网络、无运行时 provider。
前置：G5-00 Laya 来源审计、G5-01 DecisionPort 契约、G5-02 deterministic provider；前置实现锚点为 `2b458d39`，G5-02 的测试证据由阶段验收文档补充。

## 1. 目标与边界

本阶段定义一个可关闭、无副作用、可回放的 shadow decision provider 观测支路。它用于回答“候选 provider 与 deterministic baseline 的建议有何不同、延迟和失败情况如何”，而不是让模型参与 YunXi 的实际决策。

硬性不变量：

1. `deterministic baseline` 是唯一权威结果；主路径只返回它的结果。
2. shadow 默认 `disabled`，即使启用也只能记录观测，不得替换、修正或延迟主结果。
3. shadow 只读、无副作用、可回放；调用前可取消/限时，已进入 provider 的同步调用
   只能通过 `ShadowCallContext` 协作式取消/限时，模块不强行中断阻塞 provider；不持有任何写句柄。
4. 输入必须先经过 DecisionPort 的最小化和隐私校验；shadow 不得绕过校验。
5. 当前不下载或加载 Laya 权重，不引入 Python、Node、ONNX、HTTP 或其他模型依赖。

## 2. 不做事项

- 不修改 memory、profile、relationship、knowledge base 或向量索引的 schema、写入和召回；
- 不修改 fish 捕获、daemon/IPC、工具执行、MCP、host grant、权限确认、scheduler 或主动消息；
- 不把 shadow 结果提供给用户回复、工具参数、权限判断或写入资格判断；
- 不下载模型、创建模型缓存、启动 sidecar，或把 Laya/Python/ONNX 类型带入 `yunxi-core`；
- 不改变现有消费者的调用顺序、输出字节、回放事件或 deterministic fallback；
- 不记录原始 payload、个人档案全文、memory/KB 文本、终端命令、token、密码、私钥或 API key；
- 不通过重试、自动改 candidate、放宽 deadline 或猜测来掩盖 shadow 失败。

## 3. 运行拓扑

```text
consumer
   │
   ▼
request 脱敏 / 规范化 / fingerprint / DecisionPort 校验
   │
   ├── deterministic baseline ─────────► 主结果（唯一权威）
   │                                      │
   │                                      └─ consumer 继续既有规则
   │
   └── optional shadow provider（默认关闭、只读、best effort）
                 │
                 └─ validate_result ─► comparison ─► 最小化 observation
```

主结果不等待 shadow。实现可以并行启动以测量延迟，但必须先建立主结果并保留其独立性。shadow 晚到、失败或被取消时，只丢弃本次观测，不得覆盖新 request 或再次触发消费者。

## 4. 开关与预算

建议配置抽象为 `disabled | record_only`，默认值为 `disabled`。开关至少按 provider、`DecisionTask` 和 `DecisionScope` 隔离；payload、模型输出和自然语言提示不得改变开关。

- `deadline_ms = 0` 立即跳过 shadow；
- 正值不得超过调用方为主请求保留的预算；
- 取消、零有效 deadline 或队列满在 provider 调用前立即结束 shadow；已进入 provider
  的同步调用必须自行检查 `ShadowCallContext`，本模块不启动线程或强行中断阻塞调用；
- 迟到响应必须带有原始 `input_fingerprint`，不匹配时丢弃；
- 关闭 shadow 前后，主结果、消费者写入资格、权限结果、回放字节必须相同。

配置变更只影响后续请求，不得让正在执行的 observation 改变主路径。

## 5. 接口边界（当前实现仍不授权模型接入）

接口应复用 G5-01 的 `DecisionRequest`、`DecisionResult` 和 `validate_result`，不引入 Laya 专用类型：

```text
ShadowDecisionProvider::observe(
    request: &DecisionRequest,
) -> Result<DecisionResult, ShadowError>

ShadowDecisionProvider::observe_with_context(
    request: &DecisionRequest,
    context: ShadowCallContext,
) -> Result<DecisionResult, ShadowError>

observe_with_budget(mode, request, primary, provider, budget, cancelled)
observe_with_queue(mode, request, primary, provider, budget, cancelled, queue)
```

`observe_with_budget` 是兼容的无队列入口；需要并发容量保证的调用方必须持有并传入
`ShadowQueue`。queue 使用原子 in-flight 计数和 RAII permit，正容量会真实消耗并在
所有返回路径释放。provider 只能读取已通过隐私门禁的 request，不能取得 `MemoryStore`、
知识库、host grant、MCP pool、fish executor 或 scheduler 的写句柄。

建议的最小观测 envelope：

```text
ShadowObservation {
    schema_version, task, scope, input_fingerprint,
    primary_digest, shadow_digest, match_kind,
    provider_id, provider_version, reason_code,
    elapsed_ms, primary_elapsed_ms, timed_out, cancelled,
}
```

摘要只对规范化结果 envelope 计算，不保存结果原文。`match_kind` 仅允许：`exact_match`、`outcome_match`、`outcome_mismatch`、`both_abstain`、`invalid_shadow`、`timeout`、`cancelled`、`unavailable`、`privacy_rejected`、`queue_full`、`stale_fingerprint`。分类优先级固定为：先判 `both_abstain`，再判完整 envelope 的 `exact_match`，再判结构化 outcome 的 `outcome_match`，否则为 `outcome_mismatch`。

## 6. 结果比较规则

1. 先校验 request，再由 deterministic provider 产生权威 baseline。
2. shadow 结果必须完整通过 `validate_result`，否则只记录 `invalid_shadow`。
3. 只比较结构化 outcome 和安全诊断字段，不比较原始 payload、解释文本或模型内部状态。
4. confidence、score 和延迟仅用于评测，不得单独改变权限、写入资格或用户可见回复。
5. baseline 为 `abstain` 时，shadow 不能把结果升级为 choice/ranking；仍记录差异。
6. candidate 未知、重复、score 越界、fingerprint 过期或 schema 不匹配时不自动修正。
7. 同一 fingerprint 的重复观测可以去重或采样，但主结果仍保持幂等。

## 7. 隐私、审计与指标

请求进入 shadow 前必须复用 DecisionPort 的 privacy gate 和 canonical fingerprint。DecisionPort 的 key gate 不是对原文脱敏的证明；未来消费者必须按 task 使用 allowlist/脱敏 marker，只把最小 metadata 放入 payload，不能把“字段名未命中敏感词”当作授权。允许记录的最小字段只有：`task`、`scope`、`provider_id/version`、`schema_version`、`input_fingerprint`、结果摘要、`reason_code`、耗时、错误分类、开关与取消标志、计数器。

禁止记录或发送原始用户消息、完整 profile、relationship、memory/KB 内容、终端原始命令、个人路径、secret、token、password、credential、私钥、API key、未经脱敏的模型输入/输出/prompt/cache。错误信息只能引用稳定协议类别，不得回显 caller-controlled candidate ID、payload 片段或路径。provider identity 先经过 ASCII 安全字符和长度校验，非法值只记录 `unknown`。

请求本身未通过 DecisionPort privacy gate 时，observer 返回稳定的
`DecisionError::PrivacyRejected` 且不调用 provider；provider 主动返回
`ShadowError::PrivacyRejected` 时，才在已通过 gate 的 request 上记录
`ShadowMatchKind::PrivacyRejected`。两者不能混写成“原文已被记录”。

指标只存计数和延迟：`shadow_started`、`shadow_completed`、`shadow_exact_match`、`shadow_outcome_match`、`shadow_mismatch`、`shadow_invalid`、`shadow_timeout`、`shadow_cancelled`、`shadow_unavailable`、`shadow_privacy_rejected`、`shadow_queue_full`，以及 shadow/主路径各自的 p50/p95/p99。队列、采样、环形缓存有硬上限，满载时丢弃观测；指标失败不得使主请求失败。禁用时不启动后台任务或创建模型缓存。

## 8. 故障、超时与回放矩阵

| 场景 | 主路径 | shadow 记录 | 重试/修正 |
| --- | --- | --- | --- |
| disabled | deterministic baseline | 无记录 | 否 |
| provider 缺失/断连 | deterministic baseline | `unavailable` | 否 |
| deadline 到期/取消 | deterministic baseline | `timeout`/`cancelled` | 否 |
| 非法 JSON/schema | deterministic baseline | `invalid_shadow` | 否 |
| 未知/重复 candidate、越界 score | deterministic baseline | `invalid_shadow` | 否，不修正 ID |
| stale fingerprint | deterministic baseline | `invalid_shadow`/`stale_fingerprint` | 新建 request |
| privacy gate 拒绝 | 按现有 fail-closed 规则 | `privacy_rejected` | 禁止重试原始 payload |
| 队列满 | deterministic baseline | `queue_full` | 否 |
| 迟到旧响应 | 新 request 的 baseline | 当前同步 seam 分类为 `stale_fingerprint`；异步 provider 丢弃另立施工单 | 否 |

回放必须证明：shadow 开启、关闭、缺失、超时和非法结果时，同一 request 的主结果、
consumer probe 行为和 replay bytes 均相同；当前没有真实 consumer 接入，因此 memory/KB
写入资格、权限判定、scheduler 与工具调用只记录为未接入/未验证，不得伪造为已覆盖。

## 9. Laya 后续边界

真正的 Laya provider 另立 G5-04 或后续施工单，不由本合同自动授权。接入前必须固定 Git/Hugging Face revision、许可证、工件清单、供应链扫描、运行时、冷启动、CPU/RAM、p50/p95、中文及 Linux 终端术语数据，并保留 deterministic fallback。

未来 adapter 只负责把 Laya typed `choice`、`ranking`、`score`、`abstain` 映射到 `DecisionResult`；自然语言生成、工具执行、权限、memory/KB 写入、scheduler 和主动消息仍在 YunXi 现有层完成。Python、Node、ONNX、模型 cache 和进程生命周期必须隔离在 adapter/sidecar 边界，不得泄漏到核心领域模型。

## 10. 分阶段施工单

### G5-03-A：合同与状态机

固化开关、预算、错误、观测枚举和主路径不变量；不增加依赖、不改变消费者；为每个状态补充无模型测试向量。

### G5-03-B：无模型测试夹具

注入 match、mismatch、slow、cancel、断连、非法 schema、未知 candidate、越界 score 和 stale fingerprint 的 fake provider；验证 fake provider 无法取得写句柄；仅测试协议与 orchestrator。

### G5-03-C：record-only dry-run（已推送 `a5f48982`）

默认 disabled；显式启用后只记录最小 observation。`observe_with_budget` 在 provider
调用前执行取消、零 deadline、队列满预检；provider 返回的 `elapsed_ms` 超过 shadow
budget 时分类为 `timeout`。`observation_replay_bytes` 使用固定的 observation
envelope 字段序列化，`observation_replay_digest` 用 SHA-256 生成稳定摘要。该实现是
同步 best-effort：不会强行中断一个已经进入 provider 的阻塞调用；provider 可通过
`ShadowCallContext` 协作式检查 deadline。需要并发容量保证时使用 `ShadowQueue`，不启动
后台线程或模型 runtime。主结果仍直接来自 deterministic
provider；不接入 memory、KB、terminal、companion 或用户可见路径。

### G5-03-D：权威环境验收（已完成，代码硬化与 fault matrix 已推送 `6c5352ee`）

Windows 只做格式、metadata、架构依赖和隐私扫描；WSL ext4 disposable checkout 运行定向测试，并分别记录完整测试的通过、忽略和既有失败；测试后删除临时 checkout、日志、缓存和模型工件。

### G5-03-E：证据与推进门（已完成，退出审计见 `2026-10-04-g5-03-exit-audit.md`）

记录默认关闭、主路径等价、故障矩阵、隐私扫描、回放和清理证据；每批代码/文档变更单独验收并推送；证据完整后另立 G5-04 consumer admission 设计施工单，仍不授权 Laya 权重。

## 11. 验收命令

Windows：

```powershell
cargo fmt --all -- --check
cargo metadata --no-deps --format-version 1
python test_scripts/arch_dep_check.py
python testkit/privacy/g0_scan.py --self-test
python testkit/privacy/g0_scan.py --repo .
git diff --check
```

WSL Ubuntu-24.04 ext4 disposable checkout：

```bash
cargo test -p yunxi-core decision --lib --locked -- --nocapture --test-threads=1
cargo test -p yunxi-core shadow --lib --locked -- --nocapture --test-threads=1
```

若执行完整 `yunxi-core` 测试，必须分别记录通过、忽略和已有的无关失败；不得以“命令启动”代替证据。完成后删除临时 checkout、构建目录、缓存、日志和任何模型文件，确认仓库没有新增大文件或隐私内容。

## 12. 完成标准与停止条件

在本阶段“无真实消费者、无异步 provider、无模型”的边界内，G5-03 只有在以下条件
全部满足时完成：默认 disabled；deterministic baseline 永远唯一权威；shadow 失败、
超时、取消、非法结果、隐私拒绝和同步 stale fingerprint 均不改变主路径；观测字段
最小化、无原文、可回放、有限容量且指标与主路径分离；无模型、无网络、无新增运行时
依赖；定向测试和隐私/架构门禁有证据；临时环境和缓存已清理，文档、代码、测试结果
与实际状态一致。真实消费者的写入资格/权限/调度等行为和异步 provider 的迟到响应
丢弃不属于本阶段完成证据，转入 G5-04 前置设计。

若无法证明任一不变量，停止在设计或测试夹具阶段，不进入 Laya 权重接入、消费者接入或生产开关；不得用模型结果补齐缺失证据。
