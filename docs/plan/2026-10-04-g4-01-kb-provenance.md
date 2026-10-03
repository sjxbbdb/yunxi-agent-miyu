# G4-01：知识库来源元数据与引用边界

本切片为 G4 独立 Knowledge Base/RAG 的最小纵向实现。它只给现有知识库补上一套可迁移、可回读的来源元数据，不改变现有关键词/语义检索排序、工具参数、权限判定或记忆库。

## 字段语义

`kb_meta.db.files` 与 `semantic_index.db.semantic_chunks` 追加同名字段：

| 字段 | 含义 | 默认/当前值 |
| --- | --- | --- |
| `namespace` | 来源所属知识域标签；不是权限授予 | `linux-command`（`default-kb/`）或 `user`（普通导入） |
| `source_kind` | 来源类别 | `bundled`（出厂 default KB）或 `user_upload`（用户导入） |
| `source_uri` | 稳定、无本机绝对路径的来源标识 | `builtin://default-kb` 或 `user://knowledge-base` |
| `source_revision` | 来源版本/提交标识；本切片尚未接入版本发布器 | 空字符串 |

旧数据库启动时通过幂等 `ALTER TABLE ... ADD COLUMN` 追加缺失列。SQLite 默认值负责旧行回填；以 `default-kb/` 开头的旧文件再被确定性标注为 `linux-command`/`bundled`。因此旧库可直接打开，不需要手工重建文件或向量。

## 输出契约

`search_knowledge_base` 既有 `path`、`score`、`source`、`snippets` 字段保持不变；每条结果新增 `provenance` 对象，包含上述四个元数据字段。关键词和语义结果均携带同一来源标签；无嵌入服务时仍按原关键词路径返回，只缺少语义结果，不改变 fallback。

## 明确不做

- 不接受 namespace 参数，不新增或扩大权限，不把 metadata 当作鉴权凭证。
- 不把 memory.db、facts、episodes、memory_embeddings 或 profile 接入 KB；两域仍是不同目录、schema、索引和 API。
- 不引入新的向量数据库/embedding 引擎，不改变 chunk、排序、阈值或模型选择。
- 不声称两个 SQLite 文件跨库原子提交；导入/删除现有 tomb 回滚保持原边界。
- 不接 DecisionPort/Laya、prompt、终端执行、transfer registry 或 Web API。

## 验收入口

```bash
cargo fmt --all -- --check
cargo metadata --no-deps --format-version 1
cargo test -p yunxi-engine tools::knowledge_base --lib --locked -- --nocapture --test-threads=1
git diff --check
PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test
PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --repo .
```

必须在 WSL Ubuntu-24.04 ext4 的一次性 checkout 中执行 Rust 测试；完成后删除 `/tmp/yunxi-*` 临时源、target、测试 home、数据库和残留进程。该文档只记录 G4-01 的边界，不能代替 G4-02 的 namespace-scoped 权限与 G4-03 的版本失效/重建/恢复设计。
