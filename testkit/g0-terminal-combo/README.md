# G0 terminal combo blackbox

This is the smallest offline composition of the existing terminal seams:

1. a real `fish -i` PTY loads `fish-init`, accepts natural-language input, and
   sends it through `--shell-intercept --shell fish --stdin`;
2. the shell client and a second real REPL PTY share one isolated daemon/home;
3. the stub OpenAI endpoint asks for `run_command` and returns a fixed marker;
4. the script checks both PTY replies and the completed `turns.tool_flow` rows in
   the isolated `conversation.db`.

Run in WSL/Linux after building the binary:

```sh
cargo build --locked
python3 testkit/g0-terminal-combo/run.py
```

The script never uses the real home or the production daemon. It creates a fresh
`/tmp/yunxi-g0-terminal-combo-*` directory, prints its path, and leaves
`out/fish.raw`, `out/repl.raw`, `out/daemon.log`, `out/stub.log`, and
`out/report.json` for diagnosis. Override `YUNXI_BIN`, `G0_COMBO_PORT`,
`G0_COMBO_STUB_PORT`, or `G0_COMBO_TIMEOUT` when needed.

This intentionally verifies two adjacent client routes, not a fictitious direct
`fish → REPL` call: the fish hook uses the one-shot shell-intercept IPC client,
while the interactive REPL is a separate IPC client. Both must reach the same
daemon and execute `run_command` successfully.
