# G0 隐私门禁

`g0_scan.py` 扫描 Git 已跟踪的文本文件，检查公开仓库中不应出现的本机路径、私钥标记和长凭据形状。它只输出类别、计数和相对路径，不输出匹配行或匹配值。

在 WSL/Arch Linux 中运行：

```bash
PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py --self-test
PYTHONDONTWRITEBYTECODE=1 python3 testkit/privacy/g0_scan.py
```

退出码为 0 才表示门禁通过。公开的第三方客户端签名常量会归类到 `public_allowlist`；测试中使用的合成占位值会归类到 `fixture_allowlist`，两者都不会作为泄露项阻断门禁。脚本本身包含检测规则与示例，因此从扫描集合中排除自身。

该门禁只负责公开文件的快速筛查，不能替代 transfer 的逐项恢复/删除、manifest/hash/version、失败回滚和恶意归档审计。
