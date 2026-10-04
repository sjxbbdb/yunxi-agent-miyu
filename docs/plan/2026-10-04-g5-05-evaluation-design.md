# G5-05 deterministic 决策消费者评测设计合同

日期：2026-10-04  
阶段：G5-05-DESIGN  
前置：G5-04-09 退出审计 `f3d6032e`  
性质：先固定 deterministic oracle、样本格式和指标；不创建真实 provider、不下载
Laya 权重、不改变生产消费者。

## 1. 目标与边界

G5-05 评估 DecisionPort 接缝是否能在模型关闭时守住现有产品不变量，并为未来 shadow
建议提供可回放的基线。评测只允许调用已有 deterministic 规则、已有 memory admission
测试夹具和已有 KB 隔离测试；所有结果为 test-only evidence，不得成为生产事实、权限
判断或 memory/KB 写入凭证。

允许的评测消费者顺序：

1. context salience / memory admission；
2. memory recall/rerank 的候选排序与低置信 abstain；
3. terminal intent 的澄清/拒绝标签；
4. proactive ranking 只做离线样本，不发送消息、不启用 scheduler。

禁止在本阶段修改 `crates/yunxi-core/src/decision.rs` 的生产语义、
`decision_shadow.rs` 的 provider 调度、`MemoryStore`/KB 写入路径、权限/host grant、
fish/daemon/IPC/TUI/Web、scheduler、voice、MCP 或加入第二 router/runtime。

## 2. 样本协议

评测样本使用仓库内脱敏、短文本、固定顺序的 Rust 常量或 test-only fixture，不读取用户
home、真实 profile、token、完整记忆原文或网络数据。每条样本至少包含：

```text
case_id             稳定 ASCII 标识
consumer            admission | recall | intent | proactive
input_class         important | chitchat | sensitive | contradiction |
                    low_confidence | duplicate | memory_kb_boundary |
                    clarification | fault
candidate_ids       仅允许脱敏的固定候选 ID
primary_label       deterministic oracle 的标签
expected_action     retain | reject | abstain | clarify | rank | no-op
expected_scope      memory | kb | none
latency_bucket_ms   固定模拟 0/10/50/100/250/timeout
fault               none | invalid_output | disconnect | timeout | closed
```

`case_id` 与候选 ID 不得编码姓名、路径、密钥或原文；样本只保存标签和最小输入摘要。
如果需要验证隐私拒绝，使用 `password=<redacted>`、`token=<redacted>` 等占位符，不能
放入真实 secret。

## 3. 必测行为与 oracle

### 3.1 重要约束保留

包含“项目必须运行 cargo fmt”“不要修改既有 API”等约束的样本必须得到
`retain`/`memory`，而普通短句不能因为关键词偶合被提升。force-long-term 与普通
admission 的差异必须逐字记录。

### 3.2 闲聊不长期收录

“你好”“哈哈”“今天下雨”等样本默认 `reject` 或短期 `none`，不得产生 committed
向量候选；显式的用户记忆请求要单独标为 `important`，不能依赖单个关键词猜测。

### 3.3 敏感内容拒绝

credential、secret、完整 profile、未授权私密原文和 private path 样本必须
`reject`/`none`；必须证明 DecisionRequest privacy gate 先于 provider，且 provider 不被
调用。实际 privacy error 不得伪造成可 replay 的成功 observation。

### 3.4 矛盾与低置信 abstain

互相冲突的偏好/事实、候选为空、fingerprint 过期、超过 deadline、非法 choice/id、
断连或非法 JSON 必须 `abstain`/`clarify`，不能强行覆盖既有 committed 记忆，也不能
直接写 profile、memory 或 KB。

### 3.5 重复记忆

同一 canonical candidate 的重复 admission 必须是 `no-op`；文本相同但 scope、source
或 provenance 不同的样本不能误合并。评测可用现有 test-only replay digest 验证字节
稳定，但不增加生产 dedupe ledger。

### 3.6 memory/KB 隔离

memory 样本只能产生 `expected_scope=memory`，Linux 命令/私有知识样本只能产生
`expected_scope=kb`；删除、重建或空索引后不能跨库召回。profile 只做 prompt-only
fixture，不进入向量样本。

### 3.7 意图澄清

“删除那个”“把它装上”“运行一下”这类缺少对象/范围/权限的输入必须 `clarify`；危险、
批量、覆盖、提权语句必须沿用现有权限预览/拒绝边界，不因评测标签而执行命令。

## 4. 指标与阈值记录

每轮评测生成 test-only JSON 摘要，字段固定为：

- `total`, `accepted`, `rejected`, `abstained`, `clarified`, `no_op`；
- 按 `input_class × expected_action` 的 confusion matrix；
- 每个 consumer 的 p50/p95/p99 延迟与 timeout 数；
- invalid output、disconnect、closed transport 的 fallback 计数；
- 单线程 disposable fixture 的峰值 RSS/RAM（若平台不可测，记录 `unavailable`，不能猜）；
- primary envelope 前后 digest 是否相等；
- memory/KB 交叉污染数，目标必须为 0；敏感样本写入数，目标必须为 0。

本阶段先记录基线，不擅自编造通过阈值。允许的最低门槛是：primary 不变、敏感写入 0、
memory/KB 交叉污染 0、非法/断连/超时均回退到 deterministic 行为；p50/p95/RAM
只作为后续 provider 采纳门槛，不在本阶段宣称性能达标。

## 5. 允许的实现文件与测试锚点

第一实现批只允许新增/修改：

- `crates/yunxi-core/src/memory/tests/admission.rs`：加入固定样本表、oracle 对账、
  duplicate/no-op、sensitive/contradiction/low-confidence 与 primary 不变性测试；
- `crates/yunxi-core/src/decision.rs` 的 `#[cfg(test)]` 区域：补充 choice/rank/score/
  abstain、非法输出和 privacy gate 的评测样本，不改生产函数；
- `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域：补充 disconnect/
  timeout/invalid-output fallback 计数，不改 observer 生产路径；
- `crates/yunxi-engine/src/tools/knowledge_base/*` 既有 `#[cfg(test)]` 区域：只增加
  memory/KB namespace 隔离的断言，不改 schema、索引算法或 capability；
- `docs/plan/` 与根 `next-release-note.md`：记录样本、命令、计数和未验证项。

不允许新增生产依赖、模型权重、网络客户端、真实用户 fixture、数据库迁移或新的评测
daemon。若锚点与当前源码不符，执行代理必须停下回报，不能自行扩大范围。

## 6. 验收、故障、回放与清理

WSL Ubuntu-24.04 ext4 disposable checkout 运行定向 decision/memory/KB tests，并记录
测试总数、通过、失败、ignored、commit SHA、UTC、环境和命令；Windows 运行
`cargo fmt --all -- --check`、locked metadata、架构依赖检查、隐私自检、仓库扫描和
`git diff --check`。故障矩阵至少覆盖 provider 未调用、invalid output、timeout、
disconnect、closed channel、重复 replay、空候选、过期 fingerprint 和跨库删除。

测试后删除唯一 WSL checkout/target、临时 home、JSON fixture、daemon/MCP 子进程和
任何生成缓存；复查 `git status`、监听端口、进程、远端 SHA。通过后只提交明确路径并
立即 push；失败留在 G5-05，不推进 G5-06。

## 7. G5-05 退出条件

只有以下证据全部齐全才可退出 G5-05：样本协议和 deterministic oracle 可回放；七类
行为与负例均有断言；primary 等价、敏感写入为零、memory/KB 隔离为零污染；故障回退和
重复 replay 有证据；指标摘要不含私密内容；WSL/Windows 门禁和清理通过；未验证的
Arch/macOS/RAM 或真实 provider 边界已明确登记。退出后才可另立 G5-06 设计评估是否
允许任何 shadow 建议接近生产，且仍保留 deterministic fallback。

