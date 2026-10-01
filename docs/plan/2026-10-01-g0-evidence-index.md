# G0 证据索引（2026-10-01）

本索引把当前 G0 退出审计引用的命令、环境、提交和稳定摘要绑定起来。原始终端输出不写入仓库，避免把本机路径、环境变量或日志内容发布到公开仓库；`observed session` 是本次 Codex 运行中对应的终端会话标识，结果摘要只记录退出码、计数和边界结论。

生成时间：`2026-10-01 07:01:20 UTC`；本轮黑盒复核时间：`2026-10-01 07:25:11 UTC`；v4 当前复跑：`2026-10-01`（本地时间）

测试基线提交：`cfe91a89db247a3d2e5a2507489fcf8d542fee8e`；历史复核提交：`3d16ef4a`；v4 当前 workspace 复跑提交：`c8573e26f8e9849ab0a51f0b1af8ee5088eb8f70`；生产黑盒复跑提交：`8f8af396a19bf695efc5badd9d9359b10293d2d7`；MCP 协议修复提交：`ae1a8e56`；MCP 故障边界测试提交：`0e4f9d40`
环境：WSL `Ubuntu-24.04`，仓库 `<repo-root>`，`CARGO_BUILD_JOBS=1`，cargo 进程使用 `systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0`；需要中文 golden 的命令使用 `YUNXI_LANG=zh`。

## 可复核运行

| run-id | observed session | 命令 | 退出码 | 稳定摘要 |
| --- | ---: | --- | ---: | --- |
| `G0-20261001-mcp-01` | `27257` | `cargo test -p yunxi-engine tools::mcp --locked -- --test-threads=1` | 0 | 28 passed、0 failed；MCP protocol、listing cache、启动隔离、超时/断连既有测试全部通过 |
| `G0-20261001-engine-01` | `50927` | `YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1` | 0 | 658 passed、0 failed、13 ignored |
| `G0-20261001-workspace-01` | `36559` | `YUNXI_LANG=zh cargo test --workspace --locked -- --test-threads=1` | 0 | root 504/0/4；base 396/0/6；core 643/0/8；engine 658/0/13；hosts 920/0/10；所有 doctest 通过 |
| `G0-20261001-workspace-02` | `87611` | `CARGO_BUILD_JOBS=1 YUNXI_LANG=zh systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0 cargo test --workspace --locked -- --test-threads=1` | 0 | 绑定当前提交 `c8573e26` 的新鲜复跑；root 508、base 396、core 651、engine 与 hosts 测试均完成且无失败；所有 doctest 通过。测试后仅清理了未跟踪的 `target/` 构建产物 |
| `G0-20261001-prod-build-01` | `57186` | `CARGO_BUILD_JOBS=1 systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0 cargo build --locked` | 0 | 绑定提交 `8f8af396`；版本 `yunxi 0.7.0`；`target/debug/yunxi` SHA-256 `707c8c15517f7c4f8546dd27f620ca4df99535bcdcbf8438ca14cda17b5e8d3b` |
| `G0-20261001-daemon-orphan-02` | current shell (completed) | `PYTHONDONTWRITEBYTECODE=1 python3 testkit/daemon-orphan/run.py --binary target/debug/yunxi` | 0 | 使用上行 SHA-256 对应的非 testkit 生产 binary；6/6 passed；覆盖启动者 SIGKILL/正常退出、真实 daemon 存活/停止；测试后无 YunXi daemon 孤儿 |
| `G0-20261001-mcp-persistent-02` | current shell (completed) | `PYTHONDONTWRITEBYTECODE=1 python3 testkit/mcp-persistent/run.py target/debug/yunxi` | 0 | 使用同一生产 binary；5/5 passed；覆盖同 session 状态、跨 session 隔离、system prompt、删除回收和 daemon stop；测试后无 YunXi/MCP 孤儿 |
| `G0-20261001-mcp-03` | `77497` | `CARGO_TARGET_DIR=/tmp/yunxi-g0-mcp-target CARGO_BUILD_JOBS=1 YUNXI_LANG=zh systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0 cargo test -p yunxi-engine tools::mcp --lib --locked -- --test-threads=1` | 0 | 绑定提交 `0e4f9d40`；32 passed、0 failed；覆盖非法 JSON 噪声、分段 flush、半写 EOF、`method + result` 畸形响应和异常退出 PID 回收；测试结束后已删除 `/tmp/yunxi-g0-mcp-target` |
| `G0-20261001-locale-01` | current shell (completed) | `YUNXI_LANG=zh cargo test -p yunxi-engine tools::readable_names::display_name_tests::both_token_usage_tools_have_a_readable_name --locked -- --exact --test-threads=1` | 0 | 1 passed；未设置 `YUNXI_LANG=zh` 的同一既有测试曾因 locale 失败，未修改生产代码 |
| `G0-20261001-static-01` | 当前 shell | `cargo fmt --all -- --check`；`cargo metadata --no-deps --format-version 1`；`python test_scripts/arch_dep_check.py`；`git diff --check` | 0 | 格式、metadata、层序和 diff 检查通过；架构脚本的中文摘要受终端编码影响，但判定为通过 |
| `G0-20261001-privacy-01` | 当前 shell | `python testkit/privacy/g0_scan.py --self-test`；`python testkit/privacy/g0_scan.py` | 0 | 1861 tracked text files；`personal_path=0`、`private_key=0`、`credential_shape=0`；allowlist 为 1 public + 2 fixture |
| `G0-20261001-repl-01` | current shell (completed) | `python3 testkit/repl-smoke/run.py` | 0 | 修复测试夹具读取过时 `YUNXI_HOME/state/conversation.db` 后，placeholder/reply/footer/history/alive/tool-flow 全部通过；`reply_seconds=1.57`、`78 tok/s` |
| `G0-20261001-daemon-orphan-01` | current shell (completed) | `python3 testkit/daemon-orphan/run.py --binary target/debug/yunxi` | 0 | 6/6 passed；覆盖启动者 SIGKILL/正常退出、真实 daemon 存活/停止及无孤儿 |
| `G0-20261001-mcp-persistent-01` | current shell (completed) | `python3 testkit/mcp-persistent/run.py target/debug/yunxi` | 0 | 5/5 passed；覆盖同会话状态、跨会话隔离、system prompt、删除会话回收和 daemon 停止清理 |
| `G0-20261001-tui-01` | current shell (completed) | `PYTE_VENV/bin/python testkit/tui/config_forms.py --binary target/debug/yunxi`（临时 pyte 0.8.2 venv） | 0 | 16/16 passed；覆盖主菜单、供应商/模型表单、Esc 回退和保存退出 |

### 本轮失败与修复记录

本轮首次复跑 `testkit/repl-smoke/run.py` 退出码为 1：终端交互、回复速度、历史占位符和进程存活均通过，但 `tool_flow_marker=false`。调查确认不是生产回合失败：真实记录已写入当前成员布局 `YUNXI_HOME/home/<member>/conversation.db`，其中 `tool_flow` 含 `run_command` 和 `G0_09_TOOL_OK`；测试夹具仍只读取已废弃的 `YUNXI_HOME/state/conversation.db`。

修复提交 [`249b67f5`](https://github.com/sjxbbdb/yunxi-agent-miyu/commit/249b67f5) 让夹具优先检查当前 `home/*/conversation.db`，只有当前布局不存在时才回退到 legacy `state/conversation.db`；它不硬编码成员名，也不递归打开任意 SQLite 文件。修复后同一命令退出码 0，`tool_flow_marker=true`、`passed=true`。这是 G0 黑盒证据夹具的兼容修复，不改变生产运行时。

## 只读架构证据

`G0-20261001-inventory-01` 使用 `rg` 检查 daemon、shell capture/router、prompt assembler/cache、scheduler 和 memory store 的真实入口，结果已写入 [`2026-09-30-g0-architecture-audit.md`](2026-09-30-g0-architecture-audit.md)：

- daemon、shell capture/router、MemoryStore 各只有一套权威入口；bash/fish/zsh hook 和 memory 子模块是同一职责的适配/拆分，不是第二运行时。
- prompt source、agent assembler/fossilization、LLM prefix tracker、provider wire adapter 分属不同层，不合并成第二 prompt 链。
- scheduler 分为会话限流、搜索冷却、定时消息和后台 job 等不同 owner；它们不是重复的总调度器。
- KB 使用独立的 source、metadata 和 semantic index；测试里构造 `MemoryStore` 只为跨域隔离断言，不代表共库。

## 当前证据限制

1. 历史黑盒命令的原始 stdout 未归档，M-07～M-16 的摘要仍依赖此前终端记录；下一次完整 G0 门禁应输出脱敏、稳定的摘要文件并绑定 run-id。
2. M-14/M-16 的非 test binary 已在 `G0-20261001-prod-build-01` 绑定版本与 SHA-256，并由 `*-02` 黑盒复跑使用；不得把 testkit feature 产物当作生产 daemon 黑盒输入。
3. M-18 现已覆盖可关联 id 的缺失 `jsonrpc`、非法 id、畸形 error、`method + result` 混合响应、非法 JSON 噪声、分段/半写 stdout，并证明异常退出后的 PID 与连接池回收；仍未覆盖 MCP 断连/超时和权限组合的完整矩阵。
4. Arch Linux 实机、macOS M-series、transfer 目录句柄竞态、完整跨域恢复和组合故障注入保持在后续阶段风险登记中。

该文件是证据索引，不是 G0 完成声明；G1 仍未开始。
