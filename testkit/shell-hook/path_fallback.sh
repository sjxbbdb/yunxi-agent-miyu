#!/bin/bash
# shell hook 找得到 yunxi 吗(09-23):假 HOME + 假 ~/.local/bin/yunxi(只打印参数),
# PATH 里不放它,真的起 fish / bash / zsh 交互 shell 去调。
#
# 用法: testkit/shell-hook/path_fallback.sh <yunxi 二进制> <macos|linux>
#
# 必须在 PATH 与兜底目录里都**没有**真 yunxi 的机器上跑(脚本自己会查并 SKIP):
# 装着 /usr/bin/yunxi 的 Arch 开发机上,shell 调到的是那个真的,测不出兜底。
# macOS 上跑的是系统自带的 bash 3.2 与登录 shell(读 .bash_profile)。
# 09-23 基线:main 6 FAIL / 修复后 12 PASS(macOS 真机)。
set -u
# 跑测具的进程多半坐在某个 herdr pane 里(AI 会话的终端):它的 HERDR_* 漏给被测的 yunxi,
# 被测进程就会往那个 pane 报状态、认领它,把人正在看的侧栏搅乱(09-23)。
for __herdr_var in $(env | sed -n 's/^\(HERDR_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$__herdr_var"; done
BIN=$1
PLATFORM=$2
T=$(mktemp -d /tmp/yunxi-hook-e2e.XXXXXX)
trap 'rm -rf "$T"' EXIT
export HOME=$T/home YUNXI_HOME=$T/mh XDG_RUNTIME_DIR=$T/rt LANG=zh_CN.UTF-8
unset XDG_CONFIG_HOME ZDOTDIR
mkdir -p "$HOME/.local/bin" "$XDG_RUNTIME_DIR"
cat > "$HOME/.local/bin/yunxi" <<'EOF'
#!/bin/sh
echo "FAKE-YUNXI $*"
EOF
chmod +x "$HOME/.local/bin/yunxi"
# 本进程 PATH 里也不放 yunxi:装的时候应当打出「找不到」的提醒吗?——兜底目录里有,不该提醒。
SAFE_PATH=/usr/bin:/bin:/usr/sbin:/sbin
[ -d /opt/homebrew/bin ] && SAFE_PATH=/opt/homebrew/bin:$SAFE_PATH   # macOS 上 fish/zsh 可能在这儿
# 但 /opt/homebrew/bin 里真的不能有 yunxi,否则测的就不是 ~/.local 兜底了
for real in /opt/homebrew/bin/yunxi /usr/local/bin/yunxi /usr/bin/yunxi /bin/yunxi; do
  if [ -x "$real" ]; then echo "SKIP: a real yunxi at $real would shadow the fallback"; exit 2; fi
done

pass=0; fail=0
check() { if [ "$2" = "$3" ]; then echo "PASS $1"; pass=$((pass+1)); else echo "FAIL $1: got [$2] want [$3]"; fail=$((fail+1)); fi; }

for sh in fish bash zsh; do
  command -v $sh >/dev/null || { echo "SKIP $sh (not installed)"; continue; }
  out=$(PATH=$SAFE_PATH "$BIN" $sh-init 2>&1)
  echo "$out" | grep -q "PATH 上找不到" && check "$sh-init 不误报" "warned" "silent" || check "$sh-init 不误报" "silent" "silent"
done

# fish:conf.d 自动加载
if command -v fish >/dev/null; then
  got=$(PATH=$SAFE_PATH fish -i -c 'yunxi hello-fish' 2>/dev/null | tail -1)
  check "fish 兜底找到 ~/.local/bin/yunxi" "$got" "FAKE-YUNXI hello-fish"
  fish -n "$HOME/.config/fish/conf.d/yunxi.fish" && check "fish 语法" ok ok || check "fish 语法" bad ok
fi
# bash:Linux 读 .bashrc,macOS 登录 shell 读 .bash_profile
if [ "$PLATFORM" = macos ]; then
  check "bash 写进 .bash_profile" "$(grep -c 'yunxi bash hook' "$HOME/.bash_profile" 2>/dev/null || echo 0)" "2"
  check "bash 没写 .bashrc" "$([ -f "$HOME/.bashrc" ] && echo yes || echo no)" "no"
  got=$(PATH=$SAFE_PATH bash -l -i -c 'yunxi hello-bash' 2>/dev/null | tail -1)
else
  check "bash 写进 .bashrc" "$(grep -c 'yunxi bash hook' "$HOME/.bashrc" 2>/dev/null || echo 0)" "2"
  got=$(PATH=$SAFE_PATH bash -i -c 'yunxi hello-bash' 2>/dev/null | tail -1)
fi
check "bash 兜底找到 ~/.local/bin/yunxi" "$got" "FAKE-YUNXI hello-bash"
bash -n "$YUNXI_HOME"/config/shell/bash-hook.sh 2>/dev/null || bash -n "$(find "$YUNXI_HOME" -name bash-hook.sh | head -1)"
check "bash 语法" "$?" "0"
if command -v zsh >/dev/null; then
  got=$(PATH=$SAFE_PATH zsh -i -c 'yunxi hello-zsh' 2>/dev/null | tail -1)
  check "zsh 兜底找到 ~/.local/bin/yunxi" "$got" "FAKE-YUNXI hello-zsh"
fi

# 真 PATH 上有 yunxi 时不定义函数(直接用 PATH 上那个)
mkdir -p "$T/onpath"; printf '#!/bin/sh\necho "PATH-YUNXI $*"\n' > "$T/onpath/yunxi"; chmod +x "$T/onpath/yunxi"
got=$(PATH=$T/onpath:$SAFE_PATH bash -l -i -c 'yunxi x' 2>/dev/null | tail -1)
[ "$PLATFORM" = linux ] && got=$(PATH=$T/onpath:$SAFE_PATH bash -i -c 'yunxi x' 2>/dev/null | tail -1)
check "PATH 上有就用 PATH 上的" "$got" "PATH-YUNXI x"

# 哪儿都没有:装的时候要提醒
rm "$HOME/.local/bin/yunxi"
out=$(PATH=$SAFE_PATH "$BIN" zsh-init 2>&1)
echo "$out" | grep -q "PATH 上找不到" && check "找不到时提醒" yes yes || check "找不到时提醒" no yes

echo "RESULT pass=$pass fail=$fail"
[ "$fail" -eq 0 ]
