>更新内容记录在此处，每次更新release时作为releasenote发布，发布后清理已发布内容。

## 重要更新

- 完成第一轮产品层 YunXi 化迁移：主程序、Rust workspace crate、TUI/Web 文案、资源、提示词、memes 与 Linux 打包入口统一使用 YunXi 命名。
- 保留 Miyu Fork 的来源、许可证和必要兼容读取，并明确区分历史参考仓库 `YunXi-Native` 与当前产品基线 `yunxi-agent-miyu`。
- 新增 `docs/YUNXI-PRODUCT-BACKGROUND.md`，固化 Miyu Linux 底座、YunXi 陪伴层、记忆/知识库边界及候选 Laya 决策层的产品背景。

## 修复

- G3-05 memory provenance：删除提交后的 `MemoryStore` 重建会继续隐藏已删除的
  evicted carrier；`reset_all` 保留 facts/episodes 的自增高水位，避免独立 state
  清理窗口中的 tombstone ID 复用。新增重启与 ID 复用回归测试，WSL store 测试
  15/15 通过；G3-05 的跨库崩溃注入、严格 overlap 线性化和完整异步语义覆盖仍未
  宣称完成。
- G3-05 import barrier：force-import 会按 persona scope 将 live memory tombstone
  传递到 staged evicted-context，删除关联 carrier 的 provenance、embedding 和
  turn，避免旧归档复活已删除记忆；真实 export→force-import 回归与独立 fixup
  回归均在 WSL ext4 通过。

- G0 基线稳定化：修复 YunXi 产品改名后 legacy config namespace 的正/负路径兼容，校准 TUI/replay/renderer/tool-summary 的当前产品输出夹具，修正 bundled script 与 registry fixture 漂移，并让 WSL 权限位测试使用原生 Linux 文件系统临时目录。
- G0 验证：WSL Ubuntu-24.04 工作区单线程测试在 v4 当前提交上重新全绿（根包 yunxi 运行 508 项、yunxi-base 396、yunxi-core 651、yunxi-engine 与 yunxi-hosts 均完成且 0 failed；doctest 全部通过）。本结果对应 G0 evidence index 的 `G0-20261001-workspace-02`；Arch Linux 与 macOS M-series 仍未实机验证。
- G0 计划补充 Skills/MCP 生命周期、host 权限真相源、raw/display/context 三分、长期记忆删除不可召回、memory/KB 数据边界、Laya 可行性门、自动总结独立写入协议，以及不新增 YunXi 通用审批状态机的约束。
- G0 MCP 启动失败隔离：健康 MCP 不会因另一服务器启动失败而消失，健康 listing 在重复 registry 构建中复用缓存；MCP 28/28 定向测试、启动隔离 1/1 通过。
- G0 黑盒夹具兼容：`repl-smoke` 跟随当前 `YUNXI_HOME/home/<member>/conversation.db` 布局并保留 legacy fallback；TUI 配置表单在临时 pyte 0.8.2 环境复跑 16/16，daemon orphan 6/6、MCP 持久生命周期 5/5。
- G0 生产黑盒身份链：在提交 `8f8af396` 上重新构建非 testkit `yunxi 0.7.0`，SHA-256 为 `707c8c15517f7c4f8546dd27f620ca4df99535bcdcbf8438ca14cda17b5e8d3b`；daemon orphan 6/6、MCP persistent 5/5 均使用该二进制并确认无残留进程。
