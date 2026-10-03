# G4-05：知识库更新事务恢复（第一切片）

本记录对应 G4-05 的第一条代码切片。它只收紧默认知识库更新链的旧版本恢复和快照交换，不把跨 SQLite 文件的操作伪装成全局事务。

## 已完成

- `default_kb::update_inner` 在 optimized repository 路径保存旧 `HEAD`。checkout 失败或新内容校验失败时，尝试强制 checkout 回旧 revision；恢复失败会附加到原始错误，不会静默继续导入。
- `build_update_source` 改为在 `cache/default-kb` 同级临时目录中构建完整 source，成功后才交换到 `update-source`；复制/读取失败只清理 staging，旧完整缓存保留。
- `replace_default_files_with_revision` 使用 namespace 级备份：只备份 `default-kb` 文件和来源 revision，导入失败后清除新 default 文件/孤儿文件并恢复旧 default 文件/元数据，再排队 semantic 重建。不会复制或覆盖整个 `semantic_index.db`，user namespace 与 memory 数据保持不动。
- 回归覆盖：缺失 staging 的 cache swap 恢复、导入前非法文件预检、导入删除后注入元数据失败、旧 default 文件恢复和 user 文件保留。

## 明确非目标

- 不声称 meta SQLite 与 semantic SQLite 的跨文件 OS-crash 原子性；恢复后 semantic 由现有 reindex/freshness gate 重新收敛。
- 不实现第二 scheduler、第二 reindex runtime 或新的权限入口。
- 尚未处理并发 update lock、reindex marker 达到最大 pass 后的 exhaustion 状态、以及 dashboard 对 queued/exhausted 的完整暴露；这些仍是本 G4-05 的下一切片。

## 代码入口

- `crates/yunxi-engine/src/default_kb.rs`：旧 HEAD 回退、source staging/swap。
- `crates/yunxi-engine/src/tools/knowledge_base/files.rs`：default namespace 级备份、恢复和 orphan 清理。
- `crates/yunxi-engine/src/tools/knowledge_base/mod.rs`：注入导入失败回归。

## 验收证据

WSL Ubuntu-24.04 disposable ext4 checkout：

- `cargo fmt --all -- --check`：通过；
- `cargo test -p yunxi-engine default_kb --lib --locked -- --nocapture --test-threads=1`：7 passed、0 failed；
- `cargo test -p yunxi-engine tools::knowledge_base --lib --locked -- --nocapture --test-threads=1`：27 passed、0 failed、2 ignored；
- `cargo test -p yunxi-engine tools::apply_patch --lib --locked -- --nocapture --test-threads=1`：18 passed、0 failed；
- `cargo metadata --no-deps --format-version 1`：通过；
- 隐私自检/仓库扫描：passed，tracked_files=1892，credential/private_key/personal_path/unreadable 均为 0；
- disposable checkout、target 和残留测试进程已清理。

提交：`f905f538`（已推送到 `codex/yunxi-product-rename`）。
