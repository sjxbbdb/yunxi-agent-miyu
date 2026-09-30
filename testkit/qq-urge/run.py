#!/usr/bin/env python3
"""QQ 群里她回得慢、同一个人又发了一条：判了要回就并进他那一轮，只回一条（用户 09-26）。

沙箱 daemon + 假 NapCat（反向 WS）+ 桩模型（stub.py，兼扮判官、一律判回）。三段，前一段收完再开后一段：
  A  群友 @ 她问一句（TKQ1），桩慢慢写 20 秒；第 10 秒（已经过了 7 秒的覆盖窗口）他催「??」。
  B  再问一句（TKQ2），第 10 秒补一个新问题（TKNEW）——并进去重写，一条回复把两件事都答了。
  C  A、B、A 交错：他问 TKQ3，第 3 秒另一个人插一句（TKB），第 10 秒他催「??」——「??」并进 TKQ3 那一轮，
     另一个人照常排队、另起一轮。

    BIN=<yunxi> python3 testkit/qq-urge/run.py

判定：
  urge_merged / urge_one_reply            A 段：「??」并进了那一轮，整段只回一条
  new_info_merged / new_info_one_reply    B 段：新问题并进了那一轮，整段只回一条，而且是带着新问题重写的那条
  aba_merged_into_first / aba_two_replies C 段：「??」并进 TKQ3 那一轮，TKB 没并；整段两条（TKQ3 一条、TKB 一条）
  reaction_moved                          并入那一刻表情就从原问题换贴到新的那条上（回复发出之前）
  no_reaction_left                        回复发出后，所有消息上的表情都摘干净了
"""
import importlib.util
import json
import os
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

for _herdr_key in [key for key in os.environ if key.startswith("HERDR_")]:
    del os.environ[_herdr_key]

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "testkit"))
import sandbox_dir  # noqa: E402

BIN = Path(os.environ.get("BIN") or REPO / "target" / "debug" / "yunxi")
SANDBOX = sandbox_dir.make("yunxi-qq-urge-")
print("沙箱", SANDBOX, flush=True)
HOME = SANDBOX / "home"
RUNTIME = SANDBOX / "runtime"
STUB_LOG = SANDBOX / "stub.jsonl"

spec = importlib.util.spec_from_file_location("fake", REPO / "testkit" / "fake-onebot" / "run.py")
fake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fake)
ADMIN, MEMBER, OTHER = 810000001, 810000002, 810000003


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


PORT, QQ_PORT, STUB_PORT = free_port(), free_port(), free_port()
fake.PORT = QQ_PORT
results = {}
LOCK = threading.Lock()
SENDS = []
REACTIONS = []  # (message_id, set) 按到达顺序
TIMELINE = []  # ("send", 文本) / ("react", message_id, set)，两种一起按到达顺序


def check(name, ok, detail=""):
    results[name] = bool(ok)
    print(f"{'✅' if ok else '❌'} {name}  {str(detail)[:300]}", flush=True)


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
            # 一段要发好几条，别让限流把后面的吃掉。
            "group_chats": {"whitelist_rate_limit": {"max_messages": 0, "window_seconds": 60},
                            "non_whitelist_rate_limit": {"max_messages": 0, "window_seconds": 60}},
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


def pump(ws):
    while True:
        try:
            frame = ws.recv()
        except Exception:
            return
        if not isinstance(frame, dict) or "action" not in frame:
            continue
        action, params = frame["action"], frame.get("params", {})
        if action in ("send_group_msg", "send_msg"):
            with LOCK:
                SENDS.append(fake.render(params.get("message")))
                TIMELINE.append(("send", SENDS[-1]))
        if action == "set_msg_emoji_like":
            with LOCK:
                REACTIONS.append((int(params.get("message_id")), bool(params.get("set"))))
                TIMELINE.append(("react", *REACTIONS[-1]))
        try:
            ws.send({"status": "ok", "retcode": 0, "data": fake.api_data(action, params), "echo": frame.get("echo")})
        except Exception:
            return


def logs():
    text = (SANDBOX / "daemon.log").read_text(errors="replace")
    for path in (HOME / "cache" / "logs").glob("yunxi.*.log"):
        text += path.read_text(errors="replace")
    return text


def merged(message_id):
    """这条消息并进了正在跑的那一轮（日志里有它的入队行）。"""
    return any(
        ("queued as a follow-up to the active turn" in line or "已加入当前回合的后续队列" in line)
        and f"message_id={message_id}" in line
        for line in logs().splitlines()
    )


def settle(mark, count):
    """等这一段回够 count 条，再多等 10 秒：多出来的回复（另起的一轮）这时也该到了。"""
    wait_for(lambda: len(sends()) - mark >= count, 90)
    time.sleep(10)


def sends():
    with LOCK:
        return list(SENDS)


def stub_rows():
    if not STUB_LOG.exists():
        return []
    return [json.loads(line) for line in STUB_LOG.read_text(encoding="utf-8").splitlines() if line.strip()]


def main():
    assert BIN.exists(), f"missing binary {BIN}"
    RUNTIME.mkdir(parents=True, exist_ok=True)
    write_config()
    stub = subprocess.Popen([sys.executable, str(Path(__file__).with_name("stub.py"))],
                            env=dict(os.environ, STUB_PORT=str(STUB_PORT), STUB_LOG=str(STUB_LOG)))
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

        # A：只催一下
        mark = len(sends())
        start = time.time()
        q1 = fake.group_msg(ws, "TKQ1 帮我看看这个怎么装", sender=MEMBER, at_self=True, name="催催")
        wait_for(lambda: any(r["role"] == "main" for r in stub_rows()), 20)
        time.sleep(max(0, start + 10 - time.time()))
        urge = fake.group_msg(ws, "??", sender=MEMBER, at_self=True, name="催催")
        settle(mark, 1)
        out = sends()[mark:]
        check("urge_merged", merged(urge), urge)
        check("urge_one_reply", len(out) == 1, out)
        with LOCK:
            timeline = list(TIMELINE)
        first_send = next(i for i, entry in enumerate(timeline) if entry[0] == "send")
        # 换贴发生在并入那一刻：原问题上的表情要在回复发出之前就摘掉（旧行为是等它自己那条回复发出去才摘）。
        check("reaction_moved",
              ("react", urge, True) in timeline and ("react", q1, False) in timeline[:first_send], timeline)

        # B：补了新问题
        mark = len(sends())
        start = time.time()
        fake.group_msg(ws, "TKQ2 那这个呢", sender=MEMBER, at_self=True, name="催催")
        time.sleep(max(0, start + 10 - time.time()))
        extra = fake.group_msg(ws, "TKNEW 另外 Windows 上能用吗", sender=MEMBER, at_self=True, name="催催")
        settle(mark, 1)
        out = sends()[mark:]
        check("new_info_merged", merged(extra), extra)
        check("new_info_one_reply", len(out) == 1 and "NEW-REPLY" in out[0], out)

        # C：A、B、A
        mark = len(sends())
        start = time.time()
        fake.group_msg(ws, "TKQ3 还有个问题", sender=MEMBER, at_self=True, name="催催")
        time.sleep(max(0, start + 3 - time.time()))
        other = fake.group_msg(ws, "TKB 在吗", sender=OTHER, at_self=True, name="路人")
        time.sleep(max(0, start + 10 - time.time()))
        urge = fake.group_msg(ws, "??", sender=MEMBER, at_self=True, name="催催")
        settle(mark, 2)
        out = sends()[mark:]
        check("aba_merged_into_first", merged(urge) and not merged(other), (urge, other))
        check("aba_two_replies", len(out) == 2, out)

        with LOCK:
            final = {}
            for message_id, active in REACTIONS:
                final[message_id] = active
        check("no_reaction_left", final and not any(final.values()), final)
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
