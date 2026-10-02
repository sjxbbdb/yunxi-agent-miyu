# G1 阶段退出审计（G1-03）

日期：2026-10-02  
当前提交：`b0b8747d`（与 `origin/codex/yunxi-product-rename` 一致）  
基线：`056159a0`（G0 退出审计）

## 审计范围

G1 的代码目标是提供两个可回滚、可审计的接缝：

1. `CompanionContext` 可以在不改变空上下文请求字节的前提下，为后续陪伴策略追加稳定的系统提示词块；
2. `PersonaSource` 可以观察现有人格来源、作用域、revision 和显式回退状态，但不提前消费人格内容。

本审计还核对工具面稳定、回放/化石规则、请求形状探针和隐私门禁。真实供应商的 token-cache 命中不在本地测试中伪造。

## 当前提交的验证证据

以下命令在 WSL Ubuntu-24.04、ext4 临时 `CARGO_TARGET_DIR=/tmp/yunxi-g1-engine-audit`、单一 cargo 进程下执行：

| 检查 | 结果 |
| --- | --- |
| `cargo test -p yunxi-core --lib companion_context::tests --locked -- --test-threads=1` | 6 passed, 0 failed（G1-01 记录） |
| `cargo test -p yunxi-engine --lib prompt --locked -- --test-threads=1` | 16 passed, 0 failed（G1-01 记录） |
| `cargo test -p yunxi-engine --lib replay_prefix --locked -- --test-threads=1` | 13 passed, 0 failed |
| `cargo test -p yunxi-engine --lib tool_face_cache --locked -- --test-threads=1` | 2 passed, 0 failed |
| `cargo test -p yunxi-engine --lib request_shape_probe --locked -- --ignored --test-threads=1` | 1 passed, 0 failed；7 个场景，统一包含 `messages/system_prompt/tool_definitions/tools/subsystems/user_index` 字段 |
| `cargo test -p yunxi-core --lib persona_source::tests --locked -- --test-threads=1` | 8 passed, 0 failed（G1-02 记录） |
| `python test_scripts/arch_dep_check.py` | 通过 |
| `python test_scripts/refactor_size_report.py --check` | 通过 |
| `python testkit/privacy/g0_scan.py --repo .` | 通过；credential、personal path、private key、unreadable 均为 0 |
| `git diff --check` | 通过 |

`request_shape_probe` 的原始 JSON 仅写入临时目录，不进入仓库。当前探针输出的 SHA-256 为
`c068716e1aba34addf2f1d9fa8a0238f5b84e7552efacccfb75337b32f03bbd2`；该摘要只用于本次审计定位，
不作为稳定协议值，也不携带原始请求内容。

## 覆盖到的行为

- 工具轮达到上限时仍保持同一工具列表，并使用 `tool_choice: none`；
- 技能目录不再拼入工具描述，而是作为回合尾部的可回放化石；目录变化只追加新块，压缩后重新建立当前块；
- 已完成工具轮、插话、工具尾巴、中断、目标通知、artifact 清单和重复轮都按既定位置回放；
- CompanionContext 空值、不支持版本和有效值的 prompt 字节边界已有定向测试；
- PersonaSource 的来源优先级、sidecar 版本、缺失/空/目录/坏 UTF-8 回退，以及序列化不泄露路径已有定向测试。

## 未验证项与处理边界

### 请求形状 baseline 的逐字节 diff

当前仓库没有提交 G0 的原始 request-shape JSON；它可能包含环境相关的 prompt、工具描述和本机目录，不能为了审计把原文写入仓库。
本次保留了当前探针的场景数、字段集合和临时摘要，但没有声称完成 G0→G1 的逐字节 artifact diff。后续若需要严格 diff，应在脱敏规范固定后，由独立脚本只比较允许字段的规范化摘要。

### 真实供应商缓存命中

本地 HTTP fixture 证明了请求前缀、工具面和回放字节稳定；它不等价于供应商侧 token-cache 命中。本阶段不提交真实 API 凭据，也不在没有明确 provider 的情况下发起外部请求。供应商 cache 命中、跨两轮实机验证登记为后续环境验收项。

### 已知仓库残留

- 核心全量测试仍有 G0 已记录的 5 个 LLM 错误文本断言失败，另有 8 个 ignored；未经过 G1 代码路径；
- `refactor-check.sh` 的 voice 步骤仍受 WSL SOCKS 下载限制，G1 没有改动语音依赖；
- PersonaSource 仍是观察性接缝，尚未接入 Agent prompt、profile、relationship 或 memory；
- CompanionContext 的 session/phase 生命周期和 prompt fingerprint 规则留给 G2/G3，在此之前不得把每回合动态画像直接塞入稳定前缀。

## 清理与仓库状态

- `/tmp/yunxi-g1-engine-audit`、request-shape 原始 JSON 和日志属于临时证据，审计完成后删除；
- 不修改仓库已有 `target/` 构建缓存和用户未相关文件；
- 提交前再次执行 `git status --short --branch`、隐私扫描与远端 SHA 校验。

## 结论

G1-01、G1-02 的实现和协议级回归证据齐全；G1-03 已补齐当前 HEAD 的工具面、回放和请求探针记录。G1 可以进入下一阶段的设计入口，但必须把“供应商真实 cache 命中”和“脱敏后的 G0→G1 request-shape 逐字节 diff”标为未验证，不能写成通过。进入 G2 前，先固定画像/关系上下文的生命周期和 fingerprint 语义，避免破坏已有缓存与回放契约。
