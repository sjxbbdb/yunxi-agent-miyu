# G0 测试矩阵与证据索引

日期：2026-09-30
阶段：YXM-G0，进行中

本文把 G0-05 的入口、命令、环境和当前证据集中列出。`通过` 只表示该命令覆盖的范围通过，不代表 G0 或整个平台已经完成；`未验证` 和 `残余` 必须保留到阶段退出审计。

## 1. 可复现命令矩阵

| 编号 | 入口/边界 | 命令或脚本 | 环境 | 当前结果 | 残余/范围 |
| --- | --- | --- | --- | --- | --- |
| M-01 | 格式与元数据 | `cargo fmt --all -- --check`；`cargo metadata --no-deps --format-version 1` | WSL Ubuntu-24.04 | 通过 | 不覆盖运行时行为 |
| M-02 | 全工作区编译 | `cargo check --workspace --all-targets --locked` | WSL Ubuntu-24.04 | 通过 | Arch/macOS 未验证 |
| M-03 | 分 crate 单测 | `cargo test -p yunxi-base --lib --locked -- --test-threads=1`；`yunxi-core`、`yunxi-hosts`、`yunxi --lib` 同格式命令 | WSL Ubuntu-24.04 | base 395/0/6；core 642/0/8；hosts 919/0/10；root 504/0/4 | 这些是既有基线，未覆盖故障注入 |
| M-04 | engine 与 transfer | `YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1`；`cargo test -p yunxi-engine transfer --locked -- --test-threads=1` | WSL Ubuntu-24.04 | engine 649/0/13；transfer 35/35 | 父目录 TOCTOU 仍未消除 |
| M-05 | workspace 单测 | `cargo test --workspace --no-fail-fast --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 最近一次记录为 0 失败，doctest 通过 | 需在 G0 退出前按同一环境复跑并留完整日志 |
| M-06 | 架构依赖 | `python test_scripts/arch_dep_check.py` | 工作区 Python | 退出码 0 | 只检查跨层引用，不替代运行时测试 |
| M-07 | fish 普通语法/接管 | `python3 testkit/fish-accept-line/run.py`；真实 fish `pty_run.py` | WSL Ubuntu-24.04 | 静态 17/17；PTY 通过 | 尚未注入断连、SIGINT、父进程替换 |
| M-08 | daemon reload 与 IPC | `cargo test --test daemon_reload --locked -- --test-threads=1`；`cargo test -p yunxi-core ipc --lib --locked -- --test-threads=1` | WSL Ubuntu-24.04 | 2/2；IPC 33/33 | 需补跨进程断连、残留 lease、半写恢复黑盒 |
| M-09 | fish → daemon → REPL → tool | `python3 testkit/g0-terminal-combo/run.py` | WSL Ubuntu-24.04 隔离 home/daemon/stub | `passed=true`，已重复两次 | 不能替代真实模型/权限故障注入 |
| M-10 | REPL 工具执行 | `python3 testkit/repl-smoke/run.py` | WSL Ubuntu-24.04 隔离 home | `passed=true`，tool flow marker 存在 | 只验证 stub tool，不覆盖真实命令危险边界 |
| M-11 | TUI 表单 | `python3 testkit/tui/config_forms.py` | WSL Ubuntu-24.04 + pyte venv | 16/16 | 不代表窄屏、ANSI/kitty、并发重绘全覆盖 |
| M-12 | 隐私门禁 | `python testkit/privacy/g0_scan.py --repo .` | 工作区 Python | 1858 tracked text files；personal/private-key/credential 为 0 | 不替代 transfer 恢复/删除审计 |

## 2. 尚未满足的 G0 证据

1. **目录句柄安全**：当前 transfer 使用路径检查兼容后端；`ensure_destination_parent` 与后续 `rename`/rollback 之间仍存在本地 TOCTOU。必须保留该风险，不能写成“已修复”。
2. **故障注入**：尚未形成覆盖锁、权限、磁盘满、父目录替换、断连、SIGINT、半写缓存和非法 manifest 的统一矩阵。已有 transfer 单测只覆盖其中一部分。
3. **跨平台**：Arch Linux 实机和 macOS M-series 尚未运行；当前 WSL 证据不能替代它们。
4. **完整 G0-03/G0-05/G0-08**：入口已定位，但 SQLite 表/索引、embedding、删除/恢复、权限组合和耗时/失败原因还没有逐项可执行报告。

## 3. 退出前复跑要求

G0 退出前必须在同一 WSL 环境重新执行 M-01、M-02、M-04、M-05、M-06、M-07、M-08、M-09、M-10、M-11、M-12，并把完整输出或稳定摘要写入 release note；任何新失败都留在 G0 修复，不能用“启动成功”替代矩阵证据。
