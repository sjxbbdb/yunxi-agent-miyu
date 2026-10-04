# G5-04 Memory Admission Consumer 设计合同

日期：2026-10-05  
阶段：G5-04-DESIGN  
前置：G5-03 退出审计 `2026-10-04-g5-03-exit-audit.md`、G5-02 DecisionPort、现有
`MemoryStore` 生命周期与 deterministic admission 规则。  
性质：设计与无模型测试夹具合同；本文件不授权下载/加载 Laya 权重，不授权真实模型
provider、异步 runtime 或生产写入语义变更。

## 1. 选择的首个消费者

首个 DecisionPort consumer 选择 **memory admission**，而不是 context salience，理由是：

1. `crates/yunxi-core/src/memory/admission.rs` 已经有明确的
   `deterministic_admission`、`AdmissionDecision`、`AdmissionClass`、
   `AdmissionSensitivity` 和无原文的 `AdmissionCandidateMetadata`；
2. `memory/write.rs` 已经把 admission 放在 organizer batch 的单点入口，且有 mixed-source
   all-of、generated-sensitive 和 force-long-term 硬门；
3. `memory/lifecycle.rs` 已经规定 `MemoryOrganizer`/`AdmissionRules` 才能完成
   `Candidate → Committed/Rejected`，DecisionPort 不能成为终态写入者；
4. 可先验证“模型建议只观察、不写库、不改变 deterministic 结果”，不需要引入新的
   scheduler、prompt 链、向量索引或模型运行时。

## 2. 目标与非目标

### 目标

- 在 `apply_organized_batch` 为每条 diary 的 deterministic admission 建立一个 memory-local
  adapter；每条 diary 至多构造一次 DecisionRequest。
- 让 G5-03 的同步 `record-only` seam 能接收严格脱敏的 admission 元数据。
- 验证 disabled、provider 缺失、超时、取消、队列满、privacy、非法结果和 stale 都不会
  阻断写入，也不会改变 primary admission、DB 状态、lifecycle owner、embedding 或 KB。
- 固化未来异步 provider 所需的 request fingerprint、batch generation 和 consumer epoch
  绑定规则，但本阶段不创建 async task。

### 非目标

- 不把 shadow/provider 结果转换成 `admitted_ids`、`promoted_ids`、lifecycle event、
  embedding、semantic index、KB entry 或任何权限凭证。
- 不修改 memory schema、迁移、organizer 输出协议、profile 存储、短期/长期分层规则、
  memory↔KB 目录或删除/恢复语义。
- 不接入 Laya、Python、Node、ONNX、HTTP、sidecar、模型 cache、后台线程或第二 scheduler。
- 不修改 `decision.rs`/`decision_shadow.rs` 的公共协议；优先复用既有类型与函数。

## 3. 允许修改的文件与代码锚点

首个实现 slice 只允许以下路径；锚点开工前必须用 `rg` 重新确认：

| 文件 | 允许锚点 | 施工边界 |
| --- | --- | --- |
| `crates/yunxi-core/src/memory/admission.rs` | `AdmissionDecision`、`AdmissionCandidateMetadata`、`deterministic_admission*` | 增加 memory-local request builder/primary envelope；只输出枚举、布尔、版本和可选 digest，不带 diary 原文 |
| `crates/yunxi-core/src/memory/write.rs` | `apply_organized_batch` 中构造 `admission_by_id` 的单点位置 | 只在 primary 已算出后 best-effort 调 observer；observer 错误吞掉/计数，不改变 batch 结果 |
| `crates/yunxi-core/src/memory/tests/admission.rs` 或新 `memory/tests/decision_consumer.rs` | 现有 admission e2e/rule tests | 增加 allowlist、provider fault、primary 等价、lifecycle/embedding 隔离测试 |
| `docs/plan/*` | 本合同与证据文档 | 记录施工单、命令、计数、清理和残余风险 |

除非 Lead 另立施工单，不得修改 schema/migrations、`yunxi-base` 全局配置、TUI/Web、
`decision.rs`、`decision_shadow.rs`、KB、MCP、fish、daemon、scheduler 或 voice。

## 4. Memory-local adapter 契约

`AdmissionDecision` 不直接等同于通用 `DecisionResult`：它包含 memory 专属的 class、
sensitivity 和 score，而 G5-02 deterministic port 只提供通用 abstain baseline。因此
新增的只是 memory-local adapter，不是 Laya adapter：

1. 从已计算的 `AdmissionDecision` 构造 primary `DecisionResult`，候选 id 固定为短常量
   `admit`、`reject`、`abstain`；`Admit/Reject` 映射为受限 `Choice`，`Abstain` 保持
   `Abstain`；provider/reason 仍标识 deterministic baseline。
2. 构造 `DecisionRequest` 时 task=`MemoryAdmission`、scope=`Memory`，capability 只允许
   `Abstain` 与该 consumer 明确需要的只读 choice/ranking 能力；deadline 使用本 consumer
   budget，不扩大主 turn 预算。
3. provider 观察结果必须经过既有 `validate_result`；任何未知 candidate、重复 id、越界
   confidence/score、schema/fingerprint 不匹配均只记录 `invalid_shadow`。
4. adapter 不持有 `MemoryStore`、SQLite connection、KB handle、host grant、MCP pool、
   fish executor 或 scheduler；它只能接收不可变、脱敏的 request。

## 5. Request allowlist 与隐私

payload 只允许以下固定字段：

```text
schema_version: "memory.admission.v1"
lifecycle_state: "candidate"
source_class: enum(standalone|mixed|unknown)
sensitivity: enum(none|sensitive)
force_long_term: boolean
admission_rules_version: bounded ASCII string
```

首个实现故意省略 source digest，避免在脱敏字段中引入可链接性；未来若要增加 digest，
必须另立隐私施工单。严禁 diary/user/assistant/generated content、owner principal/display name、profile、session id、文件路径、命令、
credential、token、password、KB 文本或自然语言 prompt 进入 payload。candidate id 不得
使用数据库自增 id、用户 id 或可识别主体，必须使用上述固定短常量。

DecisionPort 的通用 key gate 不是 task allowlist 的替代品；adapter 必须先做独立字段、
类型、长度、digest 形状检查。privacy rejection 时 provider 调用计数必须为 0。

## 6. Primary、fallback 与生命周期不变量

每批 diary 的顺序固定为：

```text
diary
  └─ deterministic_admission (唯一 primary)
       ├─ existing all-of / sensitive / force-long-term gates
       ├─ apply_organized_batch 写入与 lifecycle owner
       └─ optional record-only observer（不等待、不写入、不改结果）
```

以下所有情况均保持 deterministic primary 和现有 DB 结果逐字/逐序等价：disabled、provider
None、Unavailable、Timeout、Cancelled、QueueFull、PrivacyRejected、InvalidShadow、Stale。
observer 失败只能变成脱敏计数/观测，不能使 `apply_organized_batch` 失败；不重试、不猜测、
不自动修正 candidate id。

敏感 source、generated-sensitive、force-long-term 与 mixed-source all-of 是硬门，优先级
高于任何未来模型建议。`MemoryLifecycleOwner::DecisionPort` 不能执行
`Candidate → Committed/Rejected`；事件 owner 继续由 `MemoryOrganizer`、`AdmissionRules`
或现有 `MemoryGc` 负责。shadow 不得触碰 embedding 或独立 KB。

## 7. 开关、预算与异步边界

- consumer 独立使用 `ShadowMode`，默认 `Disabled`；首个实现不扩大为全局配置或 TUI 开关。
- 每 diary 使用有界 `ShadowBudget`；有效 deadline 取 request/budget 最小值；queue 由调用方
  提供容量；observer 不等待 shadow、不增加主路径预算。
- 当前只允许同步 cooperative provider；取消仅在调用前 fail-closed，已进入 provider 后
  没有强制终止或异步 cancellation。阻塞 provider 在生产接入前必须另立 async 施工单。
- 未来异步响应必须携带私有 token：
  `(task, scope, input_fingerprint, diary_id, batch_database_id, batch_generation, consumer_epoch)`。
  回调时同时校验 token、fingerprint、当前 batch generation 和 mode；任一不匹配即只计
  `stale_fingerprint` 并丢弃，不调用 apply、不写 DB、不触发 organizer/permission/scheduler。
- 本阶段不创建 async runtime/task；异步迟到丢弃只写为设计边界，不能冒充 G5-03 已验证。

## 8. 测试矩阵

### 请求与隐私

- allowlist 接受固定字段，拒绝 raw text、owner/profile/path/secret marker、未知字段、超长
  字段和错误 digest；fingerprint 稳定且不含原文。
- 三态 `AdmissionDecision` 映射的 primary envelope 可通过 `validate_result`；候选集合固定。
- 非法 provider identity 被归一为 `unknown`；replay bytes/digest 固定。

### 主路径与 fault matrix

- disabled/provider None 的调用计数为 0；fake provider 的 admit/reject/abstain 不能改变
  admission_by_id、admitted/promoted 集合、DB/lifecycle/embedding 结果。
- unavailable、timeout、cancelled、queue-full、invalid schema、unknown candidate、越界
  score、stale、privacy rejection 均不阻断 primary。
- zero deadline/effective timeout 与 RAII permit 释放有独立断言；同步 stale 只分类，不宣称
  async response drop。

### 数据边界与回归

- sensitive credential/secret/OTP、generated-sensitive、force-long-term、mixed-source all-of
  继续由 deterministic hard gate 决定。
- consumer probe 只读 primary；不存在 DecisionPort→DB/KB/permission/tool 写路径。
- 重复 batch 不二次写入；memory 与 KB 的目录/schema/index/删除/恢复隔离保持不变。
- 若实现异步 token harness，补 stale generation/fingerprint 丢弃测试；否则在证据中明确未实现。

## 9. 验收、清理与推进门

Windows：

```powershell
cargo fmt --all -- --check
cargo metadata --no-deps --format-version 1 --locked
python test_scripts/arch_dep_check.py
python testkit/privacy/g0_scan.py --self-test
python testkit/privacy/g0_scan.py --repo .
git diff --check
```

WSL Ubuntu-24.04 ext4 disposable checkout：

```bash
CARGO_TARGET_DIR=/tmp/g5-04-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
  cargo test -p yunxi-core --lib memory --locked -- --nocapture --test-threads=1
CARGO_TARGET_DIR=/tmp/g5-04-decision-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 \
  cargo test -p yunxi-core decision --lib --locked -- --nocapture --test-threads=1
```

每次只跑一个 cargo；测试后删除 disposable source/target、日志、fixture、test home、
daemon/MCP 子进程和模型工件，确认无 cargo/rustc 进程、无新增大文件/隐私内容，
`git status --short --branch` 只剩施工单允许路径。完整 core 若执行，必须把既有 5 个
`llm::openai_compatible` 失败单独标为历史基线。

G5-04-DESIGN 只有在以下证据齐备后才可进入代码 slice：allowlist/privacy、primary-only
fallback、独立开关/预算、lifecycle/permission 不变量、replay/fault matrix、WSL 与
Windows 门禁、清理和 push 记录。即使设计通过，也不授权 Laya 权重或真实模型 provider；
模型资源必须另立来源/供应链/性能施工单。
