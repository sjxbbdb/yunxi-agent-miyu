# homebrew-yunxi

[YunXi](https://github.com/sjxbbdb/yunxi-agent-miyu) 的 Homebrew tap。只支持 Apple Silicon（M 系列）、macOS 15 及更新。

## 安装

```sh
brew install sjxbbdb/yunxi/yunxi
```

Homebrew 6 起，第三方 tap 里的东西要显式信任才能装。像上面这样写全名安装，Homebrew 只会信任 `yunxi` 这一个 formula，不会连带信任整个 tap。

装好后运行 `yunxi` 进入初始引导。

## 升级与卸载

```sh
brew upgrade yunxi
brew uninstall yunxi
```

卸载不会删掉 `~/.yunxi`，里面是你的配置、会话和记忆。确定不要了再手动删除。

## 说明

- 依赖 `chafa`（终端里显示图片）、`ripgrep`（搜索）和 `onnxruntime`（本地语义检索），装 YunXi 时 Homebrew 会一起装上。
- Release 页面上的 `yunxi-<版本>-<修订>-aarch64-apple-darwin.tar.gz` 就是这个 formula 下载的包。它没有签名，用浏览器直接下载会被 Gatekeeper 拦下，请用 Homebrew 安装。
- 这个仓库由发布流程自动同步，`Formula/yunxi.rb` 的真相源在 YunXi 仓库的 `packaging/homebrew/`。问题请到 [YunXi 的 Issues](https://github.com/sjxbbdb/yunxi-agent-miyu/issues) 反馈。

---

Homebrew tap for YunXi, an open-source AI assistant that lives in your terminal. Apple Silicon and macOS 15 or later only.

```sh
brew install sjxbbdb/yunxi/yunxi
```

Installing by the fully qualified name trusts only this formula (Homebrew 6 tap trust).
