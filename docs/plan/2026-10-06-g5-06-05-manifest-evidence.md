# G5-06-05 redacted measurement manifest 证据

日期：2026-10-06  
阶段：G5-06-DESIGN（test-only slice，未进入真实 provider 或生产采纳）  
代码提交：`45bbdd2f`

## 施工范围

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域加入
`MeasurementManifest` 回放夹具。它固定 run/stage/consumer/provider revision/fixture
revision/sample count/budget/mode/cleanup 字段；真实 provider 未执行时，revision 为
`unavailable`，sample count 为 0，p50/p95/p99 latency 与 peak RSS 全部保留 JSON `null`。

manifest 不含 payload、profile、context、候选正文、凭据或 provider 输出；同一批五类
consumer 的 canonical JSON replay bytes 必须稳定。

## WSL 权威验证

```text
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd <disposable-checkout> && rm -rf /tmp/g5-06-05-target && mkdir -p /tmp/g5-06-05-target && CARGO_TARGET_DIR=/tmp/g5-06-05-target cargo test -p yunxi-core --lib decision_shadow --locked -- --nocapture --test-threads=1'
```

结果：`36 passed; 0 failed; 0 ignored; 748 filtered out`。新增测试
`g5_06_measurement_manifest_is_redacted_and_marks_unavailable_metrics` 通过，G5-06-01
至 G5-06-04 的回归仍全部通过。

## 静态门禁与清理

通过 `cargo fmt --all -- --check`、`git diff --check` 与
`python testkit/privacy/g0_scan.py --repo .`；personal path/credential/private key 均为 0。
`/tmp/g5-06-05-target` 已删除并确认不存在，测试结束无该 target 的 cargo/rustc 进程。

本切片只固定“未知就 unavailable”的数据合同，不能替代真实 provider 的延迟、RAM 或
质量测量，也不授权下载 Laya、建立网络 provider 或修改生产 consumer。

