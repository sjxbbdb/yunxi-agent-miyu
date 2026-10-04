# G5-04-06 无模型异步边界设计合同

日期：2026-10-04  
阶段：G5-04-06-DESIGN  
前置：G5-04-05 consumer harness `7a7afb3b` 及证据文档。  
性质：只设计 test-only 异步形状；不授权真实异步 runtime、模型 provider 或生产写入改造。

## 事实锚点

- `crates/yunxi-core/src/decision_shadow.rs` 的 `ShadowQueue` 是 caller-owned 的同步
  `AtomicUsize` permit；`observe_with_queue` 在一个调用栈内完成校验、取消/截止/队列预检、
  provider 调用和结果分类，不创建线程、任务、channel 或 scheduler。
- `crates/yunxi-core/src/memory/admission.rs` 的 token/context/gate 只比较 task、scope、
  fingerprint、diary、database、generation、epoch、mode；gate 成功只返回 `Ok(())`，没有
  callback、数据库或 organizer 副作用。
- `crates/yunxi-core/src/memory/write.rs` 生产调用仍是默认关闭的
  `ShadowMode::Disabled + provider=None`，不能把测试夹具接到真实写入。

## 允许的最小切片

只允许修改：

- `crates/yunxi-core/src/memory/tests/admission.rs`：`#[cfg(test)]` 内使用
  `std::sync::mpsc::sync_channel(1)`、短命 `std::thread` 和 `Barrier` 模拟 producer/consumer；
  所有结果仅为内存计数。
- `docs/plan/*`：施工单与证据。

禁止修改生产 `memory/write.rs`、`memory/admission.rs`、`decision.rs`、
`decision_shadow.rs`、schema/migration、KB、TUI/Web、fish、daemon、scheduler、voice，
禁止 tokio/async runtime、真实 provider/Laya、SQLite、MemoryStore、权限或外部进程。

## 形状与顺序

test-only pending item 只携带已脱敏 request/response、私有 token 和 fault state。consumer
drain 顺序固定为：

1. fault/cancel/timeout/unavailable 先分类并丢弃；
2. `validate_result`；
3. `admit_admission_shadow_response` stale gate；
4. 仅在两道校验成功后增加内存 `applied` 计数；
5. pop 后不得重复消费。

取消是 cooperative；不能强杀阻塞 provider。fake elapsed 只用于边界分类，不使用 wall-clock
sleep。stale gate 必须是 apply 前最后一道门；上下文漂移后迟到 token 只能 drop/record。

## 必须覆盖的矩阵

- fresh response：producer→bounded channel→consumer，恰好应用一次，primary 不变；
- context advance：task、scope、fingerprint、diary、database、generation、epoch、mode
  任一漂移时跨线程迟到 response 均 stale drop；
- invalid+stale：先计 invalid，不计 stale；valid+stale 才计 stale；
- cancel before enqueue/provider、cancel after enqueue before drain；
- `sync_channel(1)` 的第二项 `try_send` 立即 queue-full，不阻塞首项；
- fake elapsed 大于 deadline 的 timeout，以及等于 deadline 的既有边界；
- unavailable/closed/shutdown 后所有短命线程 join，无 sender/receiver 泄漏；
- single-pop/no replay：一次 drain 后第二次不再 apply；不测试跨重启或持久化 replay 幂等；
- primary envelope equality、allowlist/privacy 和 token 不序列化保持不变。

## 验收与清理

WSL Ubuntu-24.04 ext4 disposable checkout 定向运行 admission tests；Windows 运行 fmt、
locked metadata、架构依赖、privacy self/repo、diff check。禁止使用 sleep 竞态；测试后
删除 `/tmp/g5-04-06-src`、`/tmp/g5-04-06-target`、日志和 fixture，确认没有
cargo/rustc/yunxi/miyu 进程。只有 test-only 矩阵全绿，才可进入下一施工单；本阶段不宣称
真实异步、强制取消、跨重启 replay、Arch 实机或生产 consumer 已实现。
