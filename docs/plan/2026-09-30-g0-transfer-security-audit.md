# G0 transfer/import/export 安全审计与最小修复清单

日期：2026-09-30
阶段：YXM-G0，未退出
范围：`crates/yunxi-engine/src/transfer/{manifest,registry,export,import,fixups,mod}.rs`

本文记录当前实现的审计结果、已落地修复和仍需平台化的边界。审计对象是 `yunxi export` / `yunxi import` 的归档完整性、路径安全、覆盖语义和跨机器修复；未列为“已落地”的项目不能视为完成。

## 结论

在本轮加固前，`cargo test -p yunxi-engine transfer` 的 14 个匹配测试只覆盖代表性的 round-trip、secret redaction、schema 拒绝、registry 分类和 fixup；不能证明恶意归档、manifest 完整性、失败回滚或新 home 布局的会话库安全。当前 HEAD 已新增 35 个 transfer 匹配测试，并把下面标记为“已落地”的校验、资源上限、coverage、tier 矩阵与回滚证据纳入测试。

在 G0 退出前，优先级顺序应为：

1. P0：拒绝不可信归档造成的路径/链接逃逸，并验证归档内容与 manifest 一一对应（已落地）。
2. P0：把 import 安装、marker 写入和选定 Core 的 stale 清理变成可回滚的事务；当前实现已落地，仍需平台化目录句柄后端消除父目录竞态。
3. P0：覆盖 `home/<admin>/conversation.db`，并对所有现存/恢复的会话数据库执行跨机器 fixup（已落地）。
4. P1：补齐格式版本和 tier 的明确矩阵验证、失败分类及回归测试。

## 证据与风险

### P0-A：manifest 格式版本未检查（已落地）

`import::check_versions` 只检查 `config_version` 和数据库 schema，没有检查 `Manifest.format_version`。当前只定义了 `MANIFEST_FORMAT_VERSION = 1`；版本未知时仍会继续安装。

最小修复已落地：要求 `manifest.format_version == MANIFEST_FORMAT_VERSION`，否则在任何占用检查、备份和解包之前失败。若以后支持旧版本，必须显式建立兼容转换表，不能用“大于当前版本”作为唯一判断。

### P0-B：manifest 与 tar 内容不是同一份事实（已落地）

`read_manifest` 读取第一个 `manifest.json` 后即返回；`extract` 随后解包所有 tar entry，既不要求路径位于 `home/`，也不核对 entry 数量、重复项、缺失项、tar header size、manifest `size` 或 `blake3`。因此未声明的 `home/*` 文件会被安装，声明的文件也可能缺失或内容被替换。

最小修复已落地为“完整归档预检 + staging 二次哈希确认”：

- `manifest.json` 必须恰好一个，且路径精确匹配；
- 每条 manifest entry 的 `path` 规范化后必须是相对路径、非空、无 `.`/`..`、无 `\\`、无平台前缀；
- tar 中除 manifest 外只能出现 `home/<entry.path>`，不能出现额外路径或重复路径；
- tar header 的 regular-file size、实际读取字节数、manifest `size` 三者一致；
- 实际内容 BLAKE3 必须等于 manifest `blake3`；
- manifest entry 的 `unit` 若能由 registry 唯一解析，必须与 `unit_for(path).id` 一致；未来未知 path 可以保留，但必须记录为 unknown unit，不能借已知 unit 名义覆盖。

验证应在目标 home 发生任何改变以前完成。

### P0-C：tar 链接与路径安全不足（已落地）

`extract` 只检查 archive path 的 `ParentDir`/绝对路径，然后调用 `tar::Entry::unpack`；没有拒绝 symlink、hard link、FIFO、device 等非普通文件。`install` 使用 `is_dir`、`create_dir_all` 和 `copy/rename`，会跟随 staged 或目标树中已有的链接父目录。即使归档 path 本身没有 `..`，链接仍可能把写入导向 staging 或目标 root 之外。

最小修复已落地：

- 只接受 directory 和 regular file entry；symlink、hard link、FIFO、block/char device、未知类型一律失败；
- 解包使用 `unpack_in` 或等价的逐段 root containment 校验，并在每个父目录创建后用 `symlink_metadata` 确认它是实际目录；
- 安装前后都拒绝目标 root 内已有的 symlink/hard link 父级；目标文件替换不能跟随链接；
- 在 Unix 测试中使用 `symlink`/`hard_link` 构造攻击样本，在支持的平台上再覆盖 Windows 路径分隔符和盘符样本；
- 对归档解压总字节数/entry 数量设置上限，避免压缩炸弹拖垮 import（上限应成为常量并在错误中说明）。

### P0-D：`--force` 不是事务，且是 merge-only（事务部分已落地）

当前流程先生成备份 archive，再直接把 staged 文件逐个放到 live root。任意 `install`、marker stamp 或 fixup 失败都可能留下半安装状态；备份 archive 只是给用户恢复用，并不会自动回滚。现有安装还不会删除 archive 未包含的旧 Core 文件，因此目标机可能继续保留 source 中已经删除的 profile/prompt/config 子项。

当前已落地“校验 → staging fixup → 可逆替换 → marker → commit/rollback”流程：

- 先计算本次归档实际覆盖的 unit/path 集合；
- 对每个待替换目标先 rename 到同一临时 rollback 目录，避免跨设备 copy 的半状态；
- 新树安装、marker 和 fixup 成功后再删除 rollback 目录；任一步失败按日志逆序恢复；
- 新 exporter 写入 `included_units` coverage 后，`--force` 会按 manifest 精确集合清理已覆盖的 stale Core 文件；legacy/手工 manifest（无 coverage）、未选择的 Heavy/Platform、Never、unknown、sidecar 和输入归档本身均保持不动。清理先移动到 rollback 临时目录，失败时随安装事务恢复；报告返回 `removed_stale`。
- rollback 也必须拒绝链接目标，并在测试中注入“第 N 个文件安装失败”和“fixup 失败”。

不要把整个 `YUNXI_HOME` 直接 rename 成 archive 解包目录：这样会误伤 `Never` 运行时文件，也无法表达 `--index/--platforms` 的部分归档语义。

### P0-E：新 home 布局的会话库未被占用检查和 fixup 覆盖（已落地）

`occupied` 只检查 `paths.state_dir/conversation.db`；`YunXiPaths::conversation_db_dir()` 在新布局返回 `home/<admin>`，所以 `home/<admin>/conversation.db` 已存在时可能被误判为空。`fixups::apply` 同样固定打开 `paths.state_dir/conversation.db`，导入新布局后不会清理 `home/<admin>/conversation.db` 的旧 workspace、footprint 和 owner pid。

最小修复已落地：`conversation_db_paths(paths)` 同时枚举有效管理员路径、legacy state 路径和每个真实 `home/<user>` 目录，去重后用于 occupied/fixup；导入则在 staging 对归档声明的全部会话库执行 fixup。新布局回归测试真实写入 `.home-layout-v1` 与 `home/tester/conversation.db`。

## P1：覆盖与归档矩阵

### P1-A：导出跟随 symlink（已落地）

`plan_unit`、`expand` 和 `collect_dir` 使用 `exists`/`is_dir`/`is_file`，这些 API 会跟随符号链接；导出可能读取 YUNXI_HOME 外部内容，且 manifest 的相对路径无法证明真实来源。

最小修复已落地：目录扫描统一使用 `symlink_metadata`；遇到 symlink 或非 regular file 时拒绝，SQLite source 也先确认 regular file。

### P1-B：tier 矩阵目前只有代表性断言

`registry` 目前共有 59 个 unit：Core 42、Heavy 1、Platform 2、Never 14。当前测试逐项覆盖四种 tier 开关组合、选择数量与 Never 单元，并验证 file/SQLite unit 不会错误认领子路径。

最小修复：增加一份由 registry 生成的审计矩阵，逐项断言：

| 选择 | 应包含 |
| --- | --- |
| 默认 | Core |
| `--index` | Core + Heavy |
| `--platforms` | Core + Platform |
| `--all` | Core + Heavy + Platform |
| 任意选择 | Never 永不包含 |

测试同时验证 wildcard unit（多个 persona/admin 名称）和嵌套 unit 不被父目录重复声明。

## 推荐测试名称与退出条件

建议直接在 `crates/yunxi-engine/src/transfer/mod.rs` 或拆分到 `transfer/tests/`，按以下名称建测试；名称刻意对应风险，便于后续 issue/发布说明引用：

### P0

- `unknown_manifest_format_is_refused_before_backup`
- `manifest_entries_match_archive_exactly`
- `manifest_size_and_blake3_must_match_archive_bytes`
- `duplicate_manifest_or_archive_path_is_refused`
- `archive_extra_path_is_refused`
- `archive_parent_path_windows_separator_and_prefix_are_refused`
- `archive_symlink_hardlink_and_special_file_are_refused`
- `existing_target_symlink_cannot_escape_install_root`
- `force_import_rolls_back_when_install_fails`
- `force_import_rolls_back_when_fixup_fails`
- `force_import_removes_stale_included_core_files_but_preserves_never`
- `new_home_layout_conversation_is_occupied_and_fixed_up`

### P1

- `export_refuses_or_skips_symlinked_sources`
- `tier_matrix_covers_every_registered_unit`
- `wildcard_home_and_persona_units_round_trip_without_overlap`
- `import_rejects_archive_size_limit_exceeded`

G0 的 transfer 退出条件：上述 P0 全部通过，P1-A/P1-B 有直接测试证据，且在 `cargo test -p yunxi-engine transfer --locked -- --test-threads=1` 与完整工作区测试中复跑通过。当前归档校验/路径安全/资源上限/coverage-aware stale Core 清理/回滚（含清理后 marker 失败恢复）/new-home fixup 与 tier 矩阵已有证据；install 的父目录检查与后续文件操作之间仍存在需要更强 openat/目录句柄语义才能彻底消除的本地竞态，因此不能宣称 transfer 安全闭环完成。
