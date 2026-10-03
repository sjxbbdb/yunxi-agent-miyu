# G4-03：知识库 source revision 生命周期

本切片把 G4-01 的来源版本字段变成可用的索引生命周期合同，同时保持 G4-02
的 capability 边界。版本字段只表示来源新旧，不是鉴权凭证。

## 变更

- 默认知识库更新和快照导入从 manifest 的 `shorinwiki.commit` 读取 revision，缺失
  时回退到 `release_hash`；导入的 `default-kb` 文件和语义块携带同一 revision。
- 语义检索在计算相似度前，将 chunk 的 `(content_sha256, namespace,
  source_revision)` 与当前文件元数据比对。后台重建尚未完成时，旧向量不会冒充当前
  知识；没有匹配的文件也不会出现在结果里。
- reindex 的工作快照在 embedding `await` 返回后再次校验同名文件的哈希、namespace
  和 revision。若期间文件被替换、更新或删除，旧任务不执行 DELETE/INSERT，而是记
  失败并留下 rerun 标记；下一趟负责收敛到新版本。空文本清理也经过同一 freshness
  检查。
- `bundled_maintenance` 仅拥有 `default-kb` 的读、写、删和 bundled replace 能力。
  测试用的 `bundled_admin` 是独立 fixture，不代表生产权限。
- reindex 的每个文件替换仍在 semantic SQLite 的单文件事务内完成；本切片不声称
  meta DB 与 semantic DB 的跨文件原子提交，也不把 source revision 当授权机制。

## 代码入口

- `crates/yunxi-engine/src/default_kb.rs`：默认知识库 revision 读取和维护 capability。
- `crates/yunxi-engine/src/tools/knowledge_base/files.rs`：带 revision 的导入入口。
- `crates/yunxi-engine/src/tools/knowledge_base/index.rs`：freshness gate、reindex
  invalidation 和并发更新回归。
- `crates/yunxi-engine/src/tools/knowledge_base/mod.rs`：维护 capability 与权限回归。

## 验收证据

WSL Ubuntu-24.04 disposable ext4 checkout（Windows DrvFS 不作为运行证据）：

- `cargo fmt --all -- --check`：通过；
- `cargo test -p yunxi-engine tools::knowledge_base --lib --locked -- --nocapture --test-threads=1`：25 passed、0 failed、2 ignored；
- `cargo test -p yunxi-engine tools::apply_patch --lib --locked -- --nocapture --test-threads=1`：18 passed、0 failed；
- `cargo metadata --no-deps --format-version 1`：通过；
- `git diff --check`：通过；
- 隐私自检和仓库扫描：passed，tracked_files=1890，credential/private_key/personal_path/unreadable 均为 0；
- 受控 embedding 延迟下的更新回归证明旧 revision 不会发布，维护 capability 回归证明
  user namespace 不能被 default maintenance 写入或删除；
- disposable checkout、target、patch、metadata 文件和残留进程已清理。

## 未覆盖与下一步

- 语义查询的 metadata 快照与后续 SQL 读取之间仍是标准读线性化，不扩大为跨 SQLite
  严格 post-commit 保证；这属于后续故障矩阵边界。
- Web identity/dashboard 的授权映射、MCP grant、换 embedding 模型的迁移策略不在
  本切片；下一步进入 G4-04，先做 source update/import failure matrix、旧版本恢复
  和可观测状态审计，再按合同进入 G5 DecisionPort/Laya 可行性门。
