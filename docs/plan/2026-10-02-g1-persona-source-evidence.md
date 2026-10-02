# G1-02 人格来源解析接缝验收

## 范围

G1-02 只增加一个观察人格来源、版本和回退状态的解析接缝，不改变现有
`AppConfig::active_persona_prompt`、`custom_system_prompt`、人格作用域或
Agent prompt 组装行为。解析结果不包含提示词原文、绝对路径、用户画像或
秘密，也不写文件、不调用模型、不产生日志。

实现位于 `crates/yunxi-core/src/persona_source.rs`，由
`crates/yunxi-core/src/lib.rs` 导出。它识别内置默认、共享人格文件、私有人格
文件、旧式 inline 和旧式文件五类来源；可选 sidecar 只读取
`schema_version`，其他字段全部忽略。

## 稳定契约

- `PERSONA_SOURCE_SCHEMA_VERSION` 当前为 `1`。
- 成功 revision 是实际 prompt bytes 的 BLAKE3 十六进制值。
- 回退 revision 始终是内置默认 prompt 的 BLAKE3 值。
- 缺失、空内容、不可读文件和坏 metadata 都只返回显式 fallback 状态。
- 未知 `PersonaSource` schema 通过 `from_json` 拒绝，不会默默当作 active。
- scope 只来自现有 `AppConfig::active_persona_scope()`，不从路径推导并不暴露路径。

## 验证证据

在 WSL Ubuntu 24.04、单一 cargo 进程和独立临时 target 下执行：

```text
cargo fmt --all
cargo test -p yunxi-core --lib persona_source::tests --locked -- --test-threads=1
python test_scripts/arch_dep_check.py
python test_scripts/refactor_size_report.py --check
python testkit/privacy/g0_scan.py --repo .
```

结果：定向测试 `8 passed; 0 failed; 657 filtered out`；架构依赖、尺寸门禁和隐私扫描通过。

覆盖项：默认内置来源、共享人格、私有人格、legacy inline/file、缺失/空/目录
替代文件及无效 UTF-8 回退、sidecar 缺省/非法 JSON/非对象/旧版本、同内容跨根目录 revision 稳定、
serde roundtrip 和未知版本拒绝，以及结果序列化不包含路径。

## 未完成与边界

本切片没有把解析结果接入 Agent prompt、memory、state schema、TUI 或
`PersonaManifest`；这些属于后续切片。Windows 原生 cargo 全量测试仍受仓库
既有 Unix-only 实现限制，Lead 在 WSL 复跑本模块；仓库级门禁仍沿用 G1-01
记录的既有错误文本断言失败和 sherpa-onnx 下载环境阻塞，不在本切片重新归因。

## 后续风险登记

本解析结果暂不进入系统提示词，因此不会改变 prompt fingerprint、缓存前缀或
回放字节。后续接入时必须先决定它是稳定人格身份（需要纳入 fingerprint/兼容
契约）还是会话级 overlay（不得误写入长期记忆），不能在两者之间隐式漂移。
