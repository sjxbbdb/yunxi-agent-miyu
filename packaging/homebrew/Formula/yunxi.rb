# 真相源在 YunXi 仓库的 packaging/homebrew/，tap 仓库 sjxbbdb/homebrew-yunxi 是它的镜像。
# url / version / sha256 / revision 由 packaging/ci/channel_update.py 按验收过的发布包写入，
# 不要手改；全零的 sha256 表示还没有发过带 macOS 包的版本。
class YunXi < Formula
  desc "Anime girl living in your terminal: open-source AI assistant"
  homepage "https://github.com/sjxbbdb/yunxi-agent-miyu"
  url "https://github.com/sjxbbdb/yunxi-agent-miyu/releases/download/v0.7.0/yunxi-0.7.0-1-aarch64-apple-darwin.tar.gz"
  version "0.7.0"
  sha256 "efedf097ee2e92c47c4dbd3af253310ec9b5edb28fd2e89974602190bc5266af"
  license all_of: ["MIT", "OFL-1.1"]

  livecheck do
    url :stable
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :sequoia
  depends_on "chafa"
  depends_on "onnxruntime"
  depends_on "ripgrep"

  def install
    # 发布包本身就是安装前缀的布局；YunXi 按程序所在前缀找 share/yunxi 下的资源。
    prefix.install "bin", "share"
  end

  test do
    assert_equal "yunxi #{version}", shell_output("#{bin}/yunxi --version").strip
    assert_match "#{share}/yunxi/personas", shell_output("#{bin}/yunxi paths")
  end
end
