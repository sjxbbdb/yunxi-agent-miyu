# G1 CompanionContext canonicalization evidence

日期：2026-10-04  
范围：G1 最小陪伴上下文接缝的 canonical 表示与 prompt 字节回归  
提交前基线：`ea2123e0`

## 变更

本切片只修复 CompanionContext 的边界规范化，不引入画像持久化、PersonaSource 正式接入、向量记忆或 DecisionPort/Laya 行为：

- `crates/yunxi-core/src/companion_context.rs`
  - 新增 `CompanionContext::canonicalized`；builder/JSON 输入统一 trim、过滤空边界并重新执行版本和长度校验。
  - `from_json` 改为复用同一 canonical 路径。
  - 增加 builder 与 JSON 输入产生相同对象、prompt 文本和 UTF-8 字节的回归测试。
- `crates/yunxi-engine/src/agent/setup.rs`
  - `Agent::set_companion_context` 只保存 canonical、受支持的上下文；不支持值和 `None` 都清除当前上下文。
- `crates/yunxi-engine/src/agent/tests/prompt.rs`
  - 增加精确的 `baseline + "\\n\\n" + block` 断言。
  - 覆盖重复 `prepare_for_turn` 的字节稳定性。
  - 覆盖不支持版本和显式清除后恢复原始 prompt 字节。

## 验证

以下命令在 WSL Ubuntu-24.04 的一次性 `CARGO_TARGET_DIR` 中执行，构建缓存不进入仓库：

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo test -p yunxi-core --lib companion_context --locked -- --nocapture --test-threads=1` | 7 passed, 0 failed |
| `cargo test -p yunxi-engine --lib agent::tests::prompt --locked -- --nocapture --test-threads=1` | 16 passed, 0 failed |
| `cargo test -p yunxi-engine --lib agent::tests::replay_prefix --locked -- --nocapture --test-threads=1` | 13 passed, 0 failed |
| `git diff --check` | 通过 |

## 边界与后续

这次 canonicalization 只保证同一上下文在 builder、JSON、prompt 组装和回放准备阶段使用同一表示；它不把 CompanionContext 写入 StateStore，也不宣称跨重启恢复或供应商 token-cache 命中已经完成。

`PersonaSource` 目前仍是 `yunxi-core` 的观察性值对象，尚未进入 `yunxi-engine` 的 prompt/cache/replay 链路。正式接入前需要先明确 CompanionContext 是会话级稳定前缀还是回合级 overlay，再决定 fingerprint、持久化和重启回放语义；本切片不越过该设计边界。

