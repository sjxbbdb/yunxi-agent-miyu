>更新内容记录在此处，每次更新release时作为releasenote发布，发布后清理已发布内容。

## 重要更新

- 完成第一轮产品层 YunXi 化迁移：主程序、Rust workspace crate、TUI/Web 文案、资源、提示词、memes 与 Linux 打包入口统一使用 YunXi 命名。
- 保留 Miyu Fork 的来源、许可证和必要兼容读取，并明确区分历史参考仓库 `YunXi-Native` 与当前产品基线 `yunxi-agent-miyu`。
- 新增 `docs/YUNXI-PRODUCT-BACKGROUND.md`，固化 Miyu Linux 底座、YunXi 陪伴层、记忆/知识库边界及候选 Laya 决策层的产品背景。

## 修复

- G0 基线稳定化：修复 YunXi 产品改名后 legacy config namespace 的正/负路径兼容，校准 TUI/replay/renderer/tool-summary 的当前产品输出夹具，修正 bundled script 与 registry fixture 漂移，并让 WSL 权限位测试使用原生 Linux 文件系统临时目录。
- G0 验证：WSL Ubuntu-24.04 工作区单线程测试最终全绿（根包 yunxi 504 passed、yunxi-base 395、yunxi-core 642、yunxi-engine 628、yunxi-hosts 919；doctest 全部通过），同时保留 Arch Linux 与 macOS M-series 尚未实机验证的状态。
- G0 计划补充 Skills/MCP 生命周期、host 权限真相源、raw/display/context 三分、长期记忆删除不可召回、memory/KB 数据边界、Laya 可行性门、自动总结独立写入协议，以及不新增 YunXi 通用审批状态机的约束。
