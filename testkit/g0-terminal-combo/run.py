#!/usr/bin/env python3
"""G0 终端组合黑盒：fish hook + daemon/IPC + REPL + run_command。

这不是线上配置的 smoke test。脚本每次创建一个新的 ``/tmp`` 沙箱、桩模型、
daemon 和两个真实 PTY：先在 fish 里走 hook 接管路径，再在 REPL 里走交互路径。
两条路径共用同一个 daemon/home，工具结果最后从隔离 conversation.db 的
``turns.tool_flow`` 读取，避免只凭屏幕文字判定。

    python3 testkit/g0-terminal-combo/run.py

前置：WSL/Linux、fish、Python3、已构建的 ``target/debug/yunxi``。可用
``YUNXI_BIN``、``G0_COMBO_PORT``、``G0_COMBO_STUB_PORT``、``G0_COMBO_TIMEOUT``
覆盖二进制、端口和超时。产物目录会在结果中打印，并保留 raw PTY、daemon 日志
和 report.json 供失败复盘。
"""

import json
import os
import pty
import re
import select
import signal
import shutil
import sqlite3
import struct
import subprocess
import sys
import tempfile
import termios
import time
import urllib.error
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BIN = Path(os.environ.get("YUNXI_BIN", ROOT / "target" / "debug" / "yunxi"))
PORT = int(os.environ.get("G0_COMBO_PORT", "18523"))
STUB_PORT = int(os.environ.get("G0_COMBO_STUB_PORT", "18524"))
TIMEOUT = float(os.environ.get("G0_COMBO_TIMEOUT", "90"))
MARKER = "G0_COMBO_TOOL_OK"
REPLY_MARKER = "G0_COMBO_REPLY"
FISH_MESSAGE = "请运行命令并回复 G0_COMBO_FISH"
REPL_MESSAGE = "请运行命令并回复 G0_COMBO_REPL"


def strip_ansi(raw: bytes) -> str:
    text = re.sub(rb"\x1b\[[0-9;?]*[ -/]*[@-~]", b"", raw)
    text = re.sub(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", b"", text)
    return text.decode("utf-8", "replace")


def write_config(home: Path) -> None:
    (home / "config").mkdir(parents=True, exist_ok=True)
    config = {
        "active_provider": "stub",
        "active_provider_models": [{"provider_id": "stub", "model": "stub-model"}],
        "providers": [{
            "id": "stub",
            "display_name": "Stub",
            "base_url": f"http://127.0.0.1:{STUB_PORT}/v1",
            "protocol": "openai-chat",
            "api_key": "stub",
            "models": ["stub-model"],
        }],
        "memory": {"enabled": False},
    }
    (home / "config" / "config.jsonc").write_text(
        json.dumps(config, ensure_ascii=False, indent=2), encoding="utf-8"
    )


def wait_http(url: str, timeout: float = 30) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(url, timeout=2).close()
            return True
        except urllib.error.HTTPError:
            return True
        except Exception:
            time.sleep(0.2)
    return False


def pty_env(home: Path, runtime: Path) -> dict[str, str]:
    env = dict(os.environ)
    env.update({
        "YUNXI_HOME": str(home),
        "XDG_RUNTIME_DIR": str(runtime),
        "XDG_CONFIG_HOME": str(home / ".config"),
        "HOME": str(home),
        "TERM": "xterm-256color",
        "LANG": "C.UTF-8",
        "PATH": f"{BIN.parent}:{env.get('PATH', '')}",
    })
    for key in ("YUNXI_DIRECT", "YUNXI_SESSION", "YUNXI_TURN_MODE"):
        env.pop(key, None)
    # Herdr 状态是宿主会话私有的，不能污染这个隔离 PTY。
    for key in list(env):
        if key.startswith("HERDR_"):
            env.pop(key, None)
    return env


def spawn_tty(argv: list[str], env: dict[str, str], cwd: Path):
    master, slave = pty.openpty()
    import fcntl

    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 36, 120, 0, 0))

    def child_setup() -> None:
        os.setsid()
        fcntl.ioctl(1, termios.TIOCSCTTY, 0)

    proc = subprocess.Popen(
        argv,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        env=env,
        cwd=str(cwd),
        preexec_fn=child_setup,
        close_fds=True,
    )
    os.close(slave)
    return proc, master


def read_tty(master: int, seconds: float, sink: bytearray) -> bytes:
    collected = bytearray()
    deadline = time.time() + seconds
    queries = {
        b"\x1b[c": b"\x1b[?1;2c",
        b"\x1b[>c": b"\x1b[>0;10;1c",
        b"\x1b[6n": b"\x1b[1;1R",
        b"\x1b[?u": b"\x1b[?0u",
    }
    while time.time() < deadline:
        ready, _, _ = select.select([master], [], [], 0.2)
        if not ready:
            continue
        try:
            data = os.read(master, 65536)
        except OSError:
            break
        if not data:
            break
        collected.extend(data)
        sink.extend(data)
        for query, answer in queries.items():
            for _ in range(data.count(query)):
                try:
                    os.write(master, answer)
                except OSError:
                    pass
    return bytes(collected)


def read_until(master: int, sink: bytearray, marker: str, timeout: float) -> tuple[bytes, bool]:
    collected = bytearray()
    deadline = time.time() + timeout
    while time.time() < deadline:
        collected.extend(read_tty(master, 0.4, sink))
        if marker in strip_ansi(bytes(collected)):
            return bytes(collected), True
    return bytes(collected), False


def stop_process(proc, master: int | None = None) -> None:
    if proc is None:
        return
    if proc.poll() is None and master is not None:
        try:
            os.write(master, b"/exit\r")
        except OSError:
            pass
    try:
        proc.wait(timeout=8)
    except Exception:
        try:
            proc.send_signal(signal.SIGTERM)
            proc.wait(timeout=5)
        except Exception:
            try:
                proc.kill()
            except Exception:
                pass


def tool_marker_count(home: Path) -> int:
    count = 0
    for db_path in home.rglob("conversation.db"):
        try:
            connection = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
            rows = connection.execute(
                "SELECT status, tool_flow FROM turns WHERE status = 'completed' "
                "AND tool_flow IS NOT NULL"
            ).fetchall()
        except sqlite3.Error:
            continue
        finally:
            try:
                connection.close()
            except UnboundLocalError:
                pass
        for _, raw_flow in rows:
            try:
                flow = json.loads(raw_flow or "[]")
            except (TypeError, ValueError):
                continue
            for event in flow if isinstance(flow, list) else []:
                calls = event.get("calls", []) if isinstance(event, dict) else []
                for call in calls if isinstance(calls, list) else []:
                    if call.get("name") == "run_command" and MARKER in str(call.get("output", "")):
                        count += 1
    return count


def main() -> int:
    if not BIN.exists():
        print(f"! missing binary, run cargo build: {BIN}", file=sys.stderr)
        return 2
    if not shutil.which("fish"):
        print("! fish is required (WSL/Linux)", file=sys.stderr)
        return 2

    root = Path(tempfile.mkdtemp(prefix="yunxi-g0-terminal-combo-"))
    home = root / "home"
    runtime = root / "run"
    out = root / "out"
    runtime.mkdir(parents=True)
    out.mkdir(parents=True)
    write_config(home)
    env = pty_env(home, runtime)
    report: dict[str, object] = {"sandbox": str(root), "marker": MARKER}
    stub = daemon = fish = repl = None
    fish_master = repl_master = None
    try:
        stub = subprocess.Popen(
            [sys.executable, str(ROOT / "testkit" / "repl-smoke" / "stub_llm.py")],
            env=dict(env, STUB_PORT=str(STUB_PORT), STUB_TOOL="1",
                     STUB_TOOL_COMMAND=f"printf {MARKER}", STUB_REPLY=REPLY_MARKER),
            cwd=str(home), stdout=(out / "stub.log").open("w"), stderr=subprocess.STDOUT,
        )
        report["stub_ready"] = wait_http(f"http://127.0.0.1:{STUB_PORT}/v1/models")
        if not report["stub_ready"]:
            raise RuntimeError("stub model did not start")

        daemon = subprocess.Popen(
            [str(BIN), "__daemon", "--port", str(PORT)], env=env, cwd=str(home),
            stdout=(out / "daemon.log").open("w"), stderr=subprocess.STDOUT,
        )
        report["daemon_ready"] = wait_http(f"http://127.0.0.1:{PORT}/api/config")
        if not report["daemon_ready"]:
            raise RuntimeError("daemon did not start")

        # 1. fish hook: 真 fish PTY + fish-init + accept-line，命中 shell-intercept。
        fish_init = subprocess.run([str(BIN), "fish-init"], env=env, cwd=str(home),
                                   capture_output=True, text=True, timeout=30)
        hook = home / ".config" / "fish" / "conf.d" / "yunxi.fish"
        report["fish_hook_installed"] = fish_init.returncode == 0 and hook.exists()
        if not report["fish_hook_installed"]:
            raise RuntimeError(f"fish-init failed: {fish_init.stderr[-300:]}")
        fish, fish_master = spawn_tty(["fish", "-i"], env, home)
        fish_sink = bytearray()
        read_tty(fish_master, 2.0, fish_sink)
        os.write(fish_master, (FISH_MESSAGE + "\r").encode())
        fish_raw, fish_reply = read_until(fish_master, fish_sink, REPLY_MARKER, TIMEOUT)
        fish_plain = strip_ansi(bytes(fish_sink))
        report["fish_reply_seen"] = fish_reply
        report["fish_glob_error"] = (
            "No matches for wildcard" in fish_plain or "未找到通配符" in fish_plain
        )
        report["fish_tool_marker_count"] = tool_marker_count(home)
        (out / "fish.raw").write_bytes(bytes(fish_sink))
        if fish_master is not None:
            try:
                os.write(fish_master, b"exit\r")
            except OSError:
                pass
        stop_process(fish, fish_master)
        fish = fish_master = None

        # 2. REPL: 独立真实 PTY，共用上面的 daemon/home；/new 让 stub 的第二轮
        # 从空会话开始，确保再次看到 run_command，而不是复用 fish 轮次的 tool 结果。
        repl, repl_master = spawn_tty([str(BIN)], env, home)
        repl_sink = bytearray()
        read_tty(repl_master, 3.0, repl_sink)
        os.write(repl_master, b"/new\r")
        read_tty(repl_master, 2.0, repl_sink)
        os.write(repl_master, (REPL_MESSAGE + "\r").encode())
        repl_raw, repl_reply = read_until(repl_master, repl_sink, REPLY_MARKER, TIMEOUT)
        report["repl_reply_seen"] = repl_reply
        report["repl_alive"] = repl.poll() is None
        report["repl_tool_marker_count"] = tool_marker_count(home)
        (out / "repl.raw").write_bytes(bytes(repl_sink))
        report["tool_marker_count"] = report["repl_tool_marker_count"]
        report["passed"] = all([
            report["stub_ready"], report["daemon_ready"], report["fish_hook_installed"],
            report["fish_reply_seen"], not report["fish_glob_error"],
            int(report["fish_tool_marker_count"]) >= 1,
            report["repl_reply_seen"], report["repl_alive"],
            int(report["repl_tool_marker_count"]) >= 2,
        ])
    except Exception as error:
        report["error"] = str(error)
        report["passed"] = False
    finally:
        stop_process(repl, repl_master)
        stop_process(fish, fish_master)
        if daemon is not None:
            try:
                subprocess.run([str(BIN), "daemon", "stop"], env=env,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               timeout=30)
            except Exception:
                pass
            stop_process(daemon)
        stop_process(stub)
        (out / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({**report, "out": str(out)}, ensure_ascii=False))
    return 0 if report.get("passed") else 1


if __name__ == "__main__":
    raise SystemExit(main())
