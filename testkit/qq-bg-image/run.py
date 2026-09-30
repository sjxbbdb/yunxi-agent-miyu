#!/usr/bin/env python3
"""QQ 里她发图不再干等上传，传失败了照后台任务的样子回报给她（用户 09-26）。

沙箱 daemon + 假 NapCat（反向 WS）+ 桩模型（stub.py）。管理员 @ 她说 TKIMG，桩让她调
send_message_to_user 发一张图；假 NapCat 收到带图的那条先憋 5 秒，再回「上传失败」。

    BIN=<yunxi> python3 testkit/qq-bg-image/run.py

判定：
  tool_returned_at_once   工具结果写着在后台传（uploading），不是等到失败才回
  text_not_blocked        她那句正文在图的失败回执之前就发出去了
  notice_reached_model    失败回报（<upload-failed>，写着文件名）交到了她手上，她据此回了一条
"""
import importlib.util
import json
import os
import socket
import struct
import subprocess
import sys
import threading
import time
import urllib.request
import zlib
from pathlib import Path

for _herdr_key in [key for key in os.environ if key.startswith("HERDR_")]:
    del os.environ[_herdr_key]

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "testkit"))
import sandbox_dir  # noqa: E402

BIN = Path(os.environ.get("BIN") or REPO / "target" / "debug" / "yunxi")
SANDBOX = sandbox_dir.make("yunxi-qq-bg-image-")
print("沙箱", SANDBOX, flush=True)
HOME = SANDBOX / "home"
RUNTIME = SANDBOX / "runtime"
STUB_LOG = SANDBOX / "stub.jsonl"
IMAGE = SANDBOX / "bg-pic.png"
IMAGE_DELAY = 5.0

spec = importlib.util.spec_from_file_location("fake", REPO / "testkit" / "fake-onebot" / "run.py")
fake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fake)
ADMIN = 810000001


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


PORT, QQ_PORT, STUB_PORT = free_port(), free_port(), free_port()
fake.PORT = QQ_PORT
results = {}
LOCK = threading.Lock()
WS_LOCK = threading.Lock()
TIMELINE = []  # (秒, 事件, 文本)
T0 = time.time()


def check(name, ok, detail=""):
    results[name] = bool(ok)
    print(f"{'✅' if ok else '❌'} {name}  {str(detail)[:400]}", flush=True)


def write_png(path):
    raw = b"".join(b"\x00" + bytes([10, 200, 10]) * 4 for _ in range(4))

    def chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data))
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack("!IIBBBBB", 4, 4, 8, 2, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def write_config():
    (HOME / "config").mkdir(parents=True, exist_ok=True)
    config = {
        "active_provider": "stub",
        "active_provider_models": [{"provider_id": "stub", "model": "stub-model"}],
        "providers": [{
            "id": "stub", "display_name": "Stub", "base_url": f"http://127.0.0.1:{STUB_PORT}/v1",
            "protocol": "openai-chat", "api_key": "stub", "models": ["stub-model"],
        }],
        "memory": {"enabled": False},
        "prompt": {"active_persona": ""},
        "platforms": {"qq": {
            "enabled": True, "reverse_ws_port": QQ_PORT, "access_token": "",
            "admin_users": [ADMIN],
            "max_reply_chars": 0,
            "plugins": {"reply_processor": {"enabled": False}},
        }},
    }
    (HOME / "config" / "config.jsonc").write_text(json.dumps(config, ensure_ascii=False, indent=2), "utf-8")


def wait_for(predicate, timeout, step=0.3):
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(step)
    return None


def note(event, text=""):
    with LOCK:
        TIMELINE.append((round(time.time() - T0, 2), event, text))


def reply(ws, obj):
    with WS_LOCK:
        ws.send(obj)


def pump(ws):
    while True:
        try:
            frame = ws.recv()
        except Exception:
            return
        if not isinstance(frame, dict) or "action" not in frame:
            continue
        action, params = frame["action"], frame.get("params", {})
        segments = params.get("message") if isinstance(params.get("message"), list) else []
        if action in ("send_group_msg", "send_msg") and any(seg.get("type") == "image" for seg in segments):
            note("image-send")

            def fail_later(echo=frame.get("echo")):
                time.sleep(IMAGE_DELAY)
                note("image-failed")
                reply(ws, {"status": "failed", "retcode": 1200, "data": None,
                           "message": "upload failed", "wording": "upload failed", "echo": echo})
            threading.Thread(target=fail_later, daemon=True).start()
            continue
        if action in ("send_group_msg", "send_msg"):
            note("text-send", fake.render(params.get("message")))
        try:
            reply(ws, {"status": "ok", "retcode": 0, "data": fake.api_data(action, params), "echo": frame.get("echo")})
        except Exception:
            return


def stub_rows():
    if not STUB_LOG.exists():
        return []
    return [json.loads(line) for line in STUB_LOG.read_text(encoding="utf-8").splitlines() if line.strip()]


def timeline():
    with LOCK:
        return list(TIMELINE)


def main():
    assert BIN.exists(), f"missing binary {BIN}"
    RUNTIME.mkdir(parents=True, exist_ok=True)
    write_config()
    write_png(IMAGE)
    stub = subprocess.Popen([sys.executable, str(Path(__file__).with_name("stub.py"))],
                            env=dict(os.environ, STUB_PORT=str(STUB_PORT), STUB_LOG=str(STUB_LOG),
                                     STUB_IMAGE=str(IMAGE)))
    env = dict(os.environ, YUNXI_HOME=str(HOME), XDG_RUNTIME_DIR=str(RUNTIME), YUNXI_LOG="info")
    log_path = SANDBOX / "daemon.log"
    daemon = subprocess.Popen([str(BIN), "__daemon", "--port", str(PORT)], env=env, cwd=str(HOME),
                              stdin=subprocess.DEVNULL, stdout=log_path.open("w"), stderr=subprocess.STDOUT)
    try:
        assert wait_for(lambda: _http_ok(f"http://127.0.0.1:{PORT}/"), 40), "daemon not up"
        time.sleep(1.5)
        ws = fake.WS.connect("")
        threading.Thread(target=pump, args=(ws,), daemon=True).start()
        time.sleep(1.5)

        with WS_LOCK:
            fake.group_msg(ws, "TKIMG 发张图看看", sender=ADMIN, at_self=True, name="管理员")
        wait_for(lambda: any(event == "text-send" and "NOTICE-REPLY" in text for _, event, text in timeline()),
                 IMAGE_DELAY + 40)
        time.sleep(2)
        rows = stub_rows()
        events = timeline()
        tool_outputs = [row["output"] for row in rows if row.get("saw") == "tool_result"]
        check("tool_returned_at_once", any('"uploading":true' in output for output in tool_outputs), tool_outputs)
        text_at = next((at for at, event, text in events if event == "text-send" and "TEXT-AFTER" in text), None)
        failed_at = next((at for at, event, _ in events if event == "image-failed"), None)
        check("text_not_blocked", text_at is not None and failed_at is not None and text_at < failed_at, events)
        notices = [row["text"] for row in rows if row.get("saw") == "notice"]
        check("notice_reached_model",
              any("bg-pic.png" in notice for notice in notices)
              and any(event == "text-send" and "NOTICE-REPLY" in text for _, event, text in events),
              notices)
    finally:
        daemon.terminate()
        try:
            daemon.wait(15)
        except subprocess.TimeoutExpired:
            daemon.kill()
        stub.terminate()
    passed = sum(results.values())
    print(f"\n{passed}/{len(results)} passed")
    sys.exit(0 if passed == len(results) else 1)


def _http_ok(url):
    try:
        urllib.request.urlopen(url, timeout=2)
        return True
    except Exception:
        return False


if __name__ == "__main__":
    main()
