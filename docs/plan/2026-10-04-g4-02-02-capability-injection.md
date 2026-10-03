# G4-02-02：可信调用方注入 Knowledge Base capability

本切片把 G4-02-01 的 KB 数据面 capability 接到现有工具执行链，不新增第二套
权限系统，也不把 namespace 参数暴露给模型。`ToolRegistry` 在构造时持有可信
owner capability；调用前由 registry 将它放入 `ToolCallContext`，KB handler 只从
这个本地上下文构造 `KnowledgeBase`。

## 已实现的调用链

```text
trusted host / compose_registry
        │
        ▼
ToolRegistry.knowledge_capability
        │ call_context()
        ▼
ToolCallContext.knowledge
   ┌────┼───────────────┐
   ▼    ▼               ▼
search  kb patch        read kb:<path>
   │    │               │
   └────┴───────────────┴── KnowledgeBase::with_tool_context
                              │
                              ▼
                  G4-02-01 namespace filtering
```

`read kb:` 在把相对路径转换成绝对路径前调用 `resolve_read_path`。因此通用文件
读取器不会绕过 KB namespace 检查；隐藏 namespace 的 exact path 和 suffix path
都返回 not-found 形状。`kb` 的 add/update/delete 继续经过 `import_file`/`remove`
及语义重建，不允许通过补丁前缀写入 `linux-command`。

## Surface 边界

- owner registry：读取 `user`、`linux-command`，写入/删除 `user`；保持旧 API 兼容。
- external surface：在 trust 过滤前清空 owner KB capability；即使未来误暴露 owner
  工具，也不会获得 KB 数据面权限。
- `KnowledgeCapability`、其字段和构造器只在 crate 内可见；模型参数、JSON schema
  和路径前缀不能构造或扩大权限。
- `KnowledgeBase::new`、default-kb maintenance、dashboard 直接数据面仍保留；它们
  不属于模型工具调用链。

## 验收证据

WSL Ubuntu-24.04 ext4 disposable checkout（最新完整 diff）：

- `cargo fmt --all -- --check`：通过；
- `cargo test -p yunxi-engine tools::knowledge_base --lib --locked -- --nocapture --test-threads=1`：23 passed、0 failed、2 ignored；
- `cargo test -p yunxi-engine tools::apply_patch --lib --locked -- --nocapture --test-threads=1`：18 passed、0 failed；
- `git diff --check`：通过；
- 隐私扫描：tracked_files=1889，credential/private_key/personal_path/unreadable 均为 0；
- disposable checkout、target、patch 与残留进程已清理。

重点回归包括 owner/external capability、`read kb:` 防绕过、KB 补丁写入、人格不
分库、namespace suffix 隔离和 user-only reindex 不清理隐藏向量。

## 明确不在本切片

- 不修改 `memory.db`、memory embedding、profile、transfer、DecisionPort/Laya；
- 不实现 Web identity 到 namespace 的映射、MCP grant 或跨会话授权；这些需独立施工单；
- 不处理 `source_revision` 失效、版本切换、重建/回滚（G4-03）；
- 不改变 embedding、chunk、排序、旧数据库迁移规则。
