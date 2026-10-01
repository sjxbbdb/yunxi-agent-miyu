# G0 测试矩阵与证据索引

日期：2026-09-30
阶段：YXM-G0，退出审计中

本文把 G0-05 的入口、命令、环境和当前证据集中列出。`通过` 只表示该命令覆盖的范围通过，不代表 G0 或整个平台已经完成；`未验证` 和 `残余` 必须保留到阶段退出审计。

## 1. 可复现命令矩阵

| 编号 | 入口/边界 | 命令或脚本 | 环境 | 当前结果 | 残余/范围 |
| --- | --- | --- | --- | --- | --- |
| M-01 | 格式与元数据 | `cargo fmt --all -- --check`；`cargo metadata --no-deps --format-version 1` | WSL Ubuntu-24.04 | 通过 | 不覆盖运行时行为 |
| M-02 | 全工作区编译 | `cargo check --workspace --all-targets --locked` | WSL Ubuntu-24.04 | 通过 | Arch/macOS 未验证 |
| M-03 | 分 crate 单测 | `cargo test -p yunxi-base --lib --locked -- --test-threads=1`；`yunxi-core`、`yunxi-hosts`、`yunxi --lib` 同格式命令 | WSL Ubuntu-24.04 | base 396/0/6；core 643/0/8（含 profile prompt-only 边界）；hosts 920/0/10；root 504/0/4 | 仍未覆盖故障注入 |
| M-04 | engine 与 transfer | `YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1`；`cargo test -p yunxi-engine transfer --locked -- --test-threads=1` | WSL Ubuntu-24.04 | engine 658/0/13（含 MCP 启动隔离、KB 双向删除隔离与 embedding 进度损坏边界）；transfer 37/37（含 usage.db SQLite 快照/导入、成员路径缺口护栏） | 父目录 TOCTOU 仍未消除 |
| M-05 | workspace 单测 | `YUNXI_LANG=zh cargo test --workspace --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 退出码 0；root 504/0/4、base 396/0/6、core 643/0/8、engine 658/0/13、hosts 920/0/10；所有 doctest 通过 | 未覆盖 Arch/macOS |
| M-06 | 架构依赖 | `python test_scripts/arch_dep_check.py` | 工作区 Python | 退出码 0 | 只检查跨层引用，不替代运行时测试 |
| M-07 | fish 普通语法/接管 | `python3 testkit/fish-accept-line/run.py`；真实 fish `pty_run.py` | WSL Ubuntu-24.04 | 静态 17/17；PTY 通过 | 尚未注入断连、SIGINT、父进程替换 |
| M-08 | daemon reload 与 IPC | `cargo test --test daemon_reload --locked -- --test-threads=1`；`cargo test -p yunxi-core ipc --lib --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 2/2；IPC 33/33 | 需补跨进程断连、残留 lease、半写恢复黑盒 |
| M-09 | fish → daemon → REPL → tool | `python3 testkit/g0-terminal-combo/run.py` | WSL Ubuntu-24.04 隔离 home/daemon/stub | `passed=true`，已重复两次 | 不能替代真实模型/权限故障注入 |
| M-10 | REPL 工具执行 | `python3 testkit/repl-smoke/run.py` | WSL Ubuntu-24.04 隔离 home | `passed=true`，tool flow marker 存在 | 只验证 stub tool，不覆盖真实命令危险边界 |
| M-11 | TUI 表单 | `PYTE_VENV/bin/python testkit/tui/config_forms.py --binary target/debug/yunxi` | WSL Ubuntu-24.04 + 临时 pyte 0.8.2 venv | 16/16；本轮已在临时 venv 复跑 | 不代表窄屏、ANSI/kitty、并发重绘全覆盖 |
| M-12 | 隐私门禁 | `python testkit/privacy/g0_scan.py --repo .` | 工作区 Python | 1861 tracked text files；personal/private-key/credential 为 0 | 不替代 transfer 恢复/删除审计 |
| M-13 | Profile/KB 跨域边界 | `cargo test -p yunxi-core memory::tests::store::user_profile_is_prompt_only_and_never_enters_memory_tables --locked -- --exact --test-threads=1`；`YUNXI_LANG=zh cargo test -p yunxi-engine tools::knowledge_base::tests::removing_a_knowledge_file_does_not_touch_memory_or_memory_embeddings --locked -- --exact --test-threads=1`；`YUNXI_LANG=zh cargo test -p yunxi-engine tools::knowledge_base::tests::resetting_memory_does_not_touch_knowledge_base_source_or_indexes --locked -- --exact --test-threads=1` | WSL Ubuntu-24.04 | 1/1 + 1/1 + 1/1 通过；profile 只进入 prompt，KB 删除不触碰 memory，memory reset 不触碰 KB 源文件、元数据或语义索引 | 仍需补完整删除/恢复、崩溃和故障注入矩阵 |
| M-14 | daemon parent/orphan 生命周期 | `cargo build --locked`；`PYTHONDONTWRITEBYTECODE=1 python3 testkit/daemon-orphan/run.py --binary target/debug/yunxi`（在仓库根运行） | WSL Ubuntu-24.04 | 先构建非 testkit 生产调试 binary 后黑盒 6/6，通过两次复跑；直接 daemon 在启动者被 SIGKILL/正常退出后退出，detached daemon 在启动命令退出后保持并可停止；测试隔离 `XDG_RUNTIME_DIR` | 不能用 `cargo test` 产物替代：testkit feature 会故意禁用自重启路径；未覆盖其它 binary、权限/延迟/断连故障注入与跨平台 |
| M-15 | 新旧 profile 路径事实边界 | `cargo test -p yunxi-base --lib config::tests::paths::home_layout_prompt_ignores_legacy_state_profile --locked -- --exact --test-threads=1`；`cargo test -p yunxi-engine transfer::tests::home_and_persona_layout_wildcards_resolve_to_their_units --locked -- --exact --test-threads=1` | WSL Ubuntu-24.04 | 1/1 + 1/1 通过；新布局 prompt 读取 `home/<user>/profile.md`，`state/profile.md` 仅作为 legacy/transfer 兼容单元；transfer wildcard 入口保持可定位 | 尚未决定 legacy state profile 的删除/迁移时机；不改变当前产品行为 |
| M-16 | MCP/Skills 会话生命周期 | `cargo build --locked`；`PYTHONDONTWRITEBYTECODE=1 python3 testkit/mcp-persistent/run.py target/debug/yunxi` | WSL Ubuntu-24.04 | 5/5 通过；同 session 复用 MCP 进程、跨 session 隔离、system prompt 注入 instructions、删除 session 回收进程、daemon stop 无 MCP 孤儿 | 启动失败隔离由 M-19 覆盖；仍未覆盖断连、超时、非法 request-shape 与权限组合 |
| M-17 | KB embedding 进度损坏/半写恢复 | `cargo test -p yunxi-engine tools::knowledge_base::dashboard::tests --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 6/6 通过；非法 JSON 明确显示 failed、停止误报 running，清理 stale lock 时保留损坏进度文件供取证；正常写入仍是临时文件 rename | 未覆盖磁盘满、权限不足、重建进程断连及 SQLite/embedding 全量恢复矩阵 |
| M-18 | MCP 非法 JSON-RPC 响应 | `CARGO_TARGET_DIR=/tmp/yunxi-g0-mcp-target CARGO_BUILD_JOBS=1 YUNXI_LANG=zh systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0 cargo test -p yunxi-engine tools::mcp --lib --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 绑定 `65deb53d`（前序协议修复、噪声/半写与连续超时切片见 `0e4f9d40`、`e17e06fb`）；33/33 MCP 测试通过，其中协议单测覆盖 4 个畸形响应分类 + 2 个合法 response shape，运行时覆盖非法 JSON 噪声、分段 flush、半写 EOF、缺失 `jsonrpc`、`method + result` 混合响应、异常退出 PID/连接池回收、连续超时后的 PID/连接池回收，以及两次调用之间自行断连后的首次重启提示；测试结束后已清理临时 target | 仍未覆盖 MCP server 启动失败之外的断连/超时剩余场景、权限组合和完整 request-shape 矩阵；断连后的首次重启提示已由本轮覆盖 |
| M-19 | MCP 启动失败隔离 | `cargo test -p yunxi-engine tools::mcp::tests::a_failed_mcp_startup_does_not_hide_a_healthy_server --locked -- --exact --test-threads=1` | WSL Ubuntu-24.04 | 1/1 通过；不存在的 MCP 可执行文件只使自身 listing 失败，健康服务器仍注册工具；重复 registry 构建命中健康 listing 缓存，不重复拉起 | 仍未覆盖 MCP 断连/超时、权限组合和完整 request-shape 矩阵；半写 stdout/EOF 已由 M-18 覆盖 |

## 2. G0 退出审计中的缺口与后续阶段风险

下列项目必须在证据索引中保留，但不自动扩张 G0 的业务范围。若它们违反本阶段最低退出合同，停在 G0；若只是后续阶段的硬化能力，则登记 owner 和阶段后移。

1. **目录句柄安全**：当前 transfer 使用路径检查兼容后端；import 的 `ensure_destination_parent` 与后续 `rename`/rollback、stale prune、marker stamp 之间仍存在本地 TOCTOU，export 的输出路径和 source `symlink_metadata`→读取之间也存在同类竞态。必须保留该风险，不能写成“已修复”。
2. **故障注入**：尚未形成覆盖锁、权限、磁盘满、父目录替换、断连、SIGINT、半写缓存和非法 manifest 的统一矩阵。已有 transfer 单测只覆盖其中一部分。
3. **跨平台**：Arch Linux 实机和 macOS M-series 尚未运行；当前 WSL 证据不能替代它们，但按 G0 合同标记为环境缺口，不阻塞 Linux 阶段推进。
4. **细化证据**：Profile prompt-only 与新旧路径事实边界已有 M-13/M-15 运行时证据，KB/Memory 双向删除隔离已有 M-13 证据，daemon parent/orphan 基本生命周期已有 M-14 黑盒证据，MCP/Skills 会话生命周期、非法 response-shape 和启动失败隔离已有 M-16/M-18/M-19 证据；SQLite 全表/索引、embedding、删除/恢复、权限组合和耗时/失败原因的逐项报告分别归入 G3/G4/G7/G9 的门禁，不在 G0 偷换成业务实现。

## 3. 退出前复跑要求

G0 退出前必须在同一 WSL 环境重新执行 M-01、M-02、M-04、M-05、M-06、M-07、M-08、M-09、M-10、M-11、M-12、M-13、M-14、M-15、M-16、M-17、M-18、M-19，并把完整输出或稳定摘要写入 release note；任何新失败都留在 G0 修复，不能用“启动成功”替代矩阵证据。
