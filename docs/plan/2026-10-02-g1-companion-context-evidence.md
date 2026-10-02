# G1-01 CompanionContext 接缝验收记录

日期：2026-10-02
基线：`056159a0`（G0 退出审计完成）
范围：G1 的第一条最小可回滚切片；不包含画像持久化、记忆入库、向量化或 DecisionPort/Laya。

## 目标

为后续 YunXi 人格、关系和陪伴策略模块提供一个小而稳定的运行时接缝：

- 上游可以在当前 Agent 实例上注入一份可选上下文；
- 没有上下文、空上下文或不支持的版本时，既有系统提示词保持字节级不变；
- 有效上下文只进入系统提示词，不写入会话历史、记忆文件、用户档案或日志；
- 序列化字段和提示词块顺序固定，后续可以由 G2/G3 的正式模块填充；
- 输入有长度、数量、版本和 XML 转义边界，异常数据回退为不注入。

## 实现边界

### 新增核心值对象

`crates/yunxi-core/src/companion_context.rs`

- `COMPANION_CONTEXT_SCHEMA_VERSION = 1`；
- 固定元数据：`source`、`version`、`scope`；
- 可选载荷：关系阶段、稳定语气、边界列表、回复偏好、当前陪伴状态；
- `from_json` 负责解析、规范化和版本/长度校验；
- `to_prompt_block` 生成稳定顺序的 XML-style 块，并转义 XML 特殊字符；
- 空载荷返回 `None`，不会留下空标签；
- 当前限制：元数据 64 字符、普通载荷 512 字符、单条边界 256 字符、最多 16 条边界。

### Agent 接入

- `crates/yunxi-engine/src/agent/turn_state.rs`：`TurnInput` 增加可选上下文；
- `crates/yunxi-engine/src/agent/setup.rs`：初始化为 `None`，新增
  `Agent::set_companion_context`，刷新当前系统提示词；不触碰历史和记忆写入；
- `crates/yunxi-engine/src/agent/prompt.rs`：在既有运行时、宿主和记忆前言组装完成后追加上下文块；
- `crates/yunxi-engine/src/agent/tests/prompt.rs`：覆盖注入、字段顺序和不支持版本回退后的字节稳定性；
- `test_scripts/arch_dep_check.py`：将 `companion_context` 纳入“子系统”层，保持架构门禁可识别。

## 验证证据

以下命令在 WSL Ubuntu-24.04、ext4 临时构建目录中执行；`--test-threads=1` 用于避免并行占用无关资源。

| 检查 | 结果 |
| --- | --- |
| `cargo test -p yunxi-core --lib companion_context::tests --locked -- --test-threads=1` | 6 passed, 0 failed |
| `cargo test -p yunxi-engine agent::tests::prompt --locked -- --test-threads=1` | 16 passed, 0 failed |
| `cargo test -p yunxi-engine agent::tests::replay_prefix --locked -- --test-threads=1` | 13 passed, 0 failed |
| `cargo test -p yunxi-engine --lib request_shape_probe --locked -- --ignored --test-threads=1` | 1 passed, 0 failed |
| `cargo test -p yunxi-core --lib --locked -- --test-threads=1` | 644 passed；5 个既有错误文本断言失败；8 ignored |
| `cargo fmt --all -- --check` | 通过 |
| `cargo metadata --no-deps --format-version 1` | 通过 |
| `git diff --check` | 通过 |
| `python test_scripts/arch_dep_check.py` | 通过，无新增跨层引用 |
| `python test_scripts/refactor_size_report.py --check` | 通过，无新增越线文件 |
| `python test_scripts/fmt_no_regress.py` | 通过；本机未安装 CI 指定的 1.96.1，仅有工具链提示 |
| `python testkit/privacy/g0_scan.py --repo .` | 通过；凭据、本机路径、私钥均为 0 |

## 未通过项与归因

### 核心已有断言失败

核心全量测试中的 5 个失败均属于 `llm::openai_compatible` 的既有错误文本断言，涉及限流、relay 失败、单端点和全失败提示，不经过 `CompanionContext` 代码。该结果作为基线残留记录，不冒充 G1 回归通过。

### `refactor-check.sh` 的语音步骤

规范化脚本的 CRLF 副本可以进入语音步骤，但 `sherpa-onnx-sys v1.13.7` 尝试下载既有静态包时，被当前 WSL 的 SOCKS 网络能力拒绝：

`Connection Failed: Connect error: SOCKS feature disabled`

本切片没有修改语音模块，也没有为了通过检查引入下载物或改变依赖。待可用的本地归档或网络环境提供后，再单独复跑该步骤。

## 不做事项

- 不把人格/灵魂/关系内容写入长期记忆；
- 不把 CompanionContext 当作向量数据库或知识库；
- 不改变 Windows/Web/微信/语音边界；
- 不引入 Laya/DecisionPort 的决策行为；
- 不迁移历史 `state/profile.md`，该项仍由 G2 负责；
- 不修改 G0 已登记的迁移、锁、权限和跨 SQLite 原子性残留。

## 验收结论

G1-01 的代码与单元/定向回归范围满足施工单，具备提交条件。核心全量中的既有错误文本断言和语音依赖下载属于独立残留，不阻塞本切片的最小接缝验收；提交后继续保留 G1–G9 长线目标，不将整个升级计划标记为完成。

## 后续风险登记

当前上下文在系统提示词尾部追加；如果上游每一回合都生成新值，会主动改变缓存前缀。G2/G3 接入真实画像与关系数据时，必须定义生命周期（会话级或阶段级），避免把高频瞬态数据误放进稳定提示词，也避免破坏既有请求缓存收益。
