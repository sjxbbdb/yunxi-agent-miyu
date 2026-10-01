# G0 证据索引（2026-10-01）

本索引把当前 G0 退出审计引用的命令、环境、提交和稳定摘要绑定起来。原始终端输出不写入仓库，避免把本机路径、环境变量或日志内容发布到公开仓库；`observed session` 是本次 Codex 运行中对应的终端会话标识，结果摘要只记录退出码、计数和边界结论。

生成时间：`2026-10-01 07:01:20 UTC`  
基线提交：`cfe91a89db247a3d2e5a2507489fcf8d542fee8e`  
环境：WSL `Ubuntu-24.04`，仓库 `/mnt/d/YunXi-Miyu`，`CARGO_BUILD_JOBS=1`，cargo 进程使用 `systemd-run --user --scope -p MemoryMax=20G -p MemorySwapMax=0`；需要中文 golden 的命令使用 `YUNXI_LANG=zh`。

## 可复核运行

| run-id | observed session | 命令 | 退出码 | 稳定摘要 |
| --- | ---: | --- | ---: | --- |
| `G0-20261001-mcp-01` | `27257` | `cargo test -p yunxi-engine tools::mcp --locked -- --test-threads=1` | 0 | 28 passed、0 failed；MCP protocol、listing cache、启动隔离、超时/断连既有测试全部通过 |
| `G0-20261001-engine-01` | `50927` | `YUNXI_LANG=zh cargo test -p yunxi-engine --lib --locked -- --test-threads=1` | 0 | 658 passed、0 failed、13 ignored |
| `G0-20261001-workspace-01` | `36559` | `YUNXI_LANG=zh cargo test --workspace --locked -- --test-threads=1` | 0 | root 504/0/4；base 396/0/6；core 643/0/8；engine 658/0/13；hosts 920/0/10；所有 doctest 通过 |
| `G0-20261001-locale-01` | current shell (completed) | `YUNXI_LANG=zh cargo test -p yunxi-engine tools::readable_names::display_name_tests::both_token_usage_tools_have_a_readable_name --locked -- --exact --test-threads=1` | 0 | 1 passed；未设置 `YUNXI_LANG=zh` 的同一既有测试曾因 locale 失败，未修改生产代码 |
| `G0-20261001-static-01` | 当前 shell | `cargo fmt --all -- --check`；`cargo metadata --no-deps --format-version 1`；`python test_scripts/arch_dep_check.py`；`git diff --check` | 0 | 格式、metadata、层序和 diff 检查通过；架构脚本的中文摘要受终端编码影响，但判定为通过 |
| `G0-20261001-privacy-01` | 当前 shell | `python testkit/privacy/g0_scan.py --self-test`；`python testkit/privacy/g0_scan.py` | 0 | 1859 tracked text files；`personal_path=0`、`private_key=0`、`credential_shape=0`；allowlist 为 1 public + 2 fixture |

## 只读架构证据

`G0-20261001-inventory-01` 使用 `rg` 检查 daemon、shell capture/router、prompt assembler/cache、scheduler 和 memory store 的真实入口，结果已写入 [`2026-09-30-g0-architecture-audit.md`](2026-09-30-g0-architecture-audit.md)：

- daemon、shell capture/router、MemoryStore 各只有一套权威入口；bash/fish/zsh hook 和 memory 子模块是同一职责的适配/拆分，不是第二运行时。
- prompt source、agent assembler/fossilization、LLM prefix tracker、provider wire adapter 分属不同层，不合并成第二 prompt 链。
- scheduler 分为会话限流、搜索冷却、定时消息和后台 job 等不同 owner；它们不是重复的总调度器。
- KB 使用独立的 source、metadata 和 semantic index；测试里构造 `MemoryStore` 只为跨域隔离断言，不代表共库。

## 当前证据限制

1. 历史黑盒命令的原始 stdout 未归档，M-07～M-16 的摘要仍依赖此前终端记录；下一次完整 G0 门禁应输出脱敏、稳定的摘要文件并绑定 run-id。
2. M-14/M-16 的非 test binary 构建尚未在本索引记录二进制 SHA-256；不得把 testkit feature 产物当作生产 daemon 黑盒输入。
3. M-18 只证明匹配调用快速失败，不独立证明 PID 退出；非法 JSON、半写 stdout 和 method+result 畸形对象仍是 G0-08 覆盖缺口。
4. Arch Linux 实机、macOS M-series、transfer 目录句柄竞态、完整跨域恢复和组合故障注入保持在后续阶段风险登记中。

该文件是证据索引，不是 G0 完成声明；G1 仍未开始。
