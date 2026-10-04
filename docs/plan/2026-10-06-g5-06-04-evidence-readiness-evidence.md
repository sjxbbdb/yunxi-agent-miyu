# G5-06-04 adoption evidence readiness 证据

日期：2026-10-06  
阶段：G5-06-DESIGN（test-only slice，未进入真实 provider 或生产采纳）  
代码提交：`a774768a`

## 施工范围

在 `crates/yunxi-core/src/decision_shadow.rs` 的 `#[cfg(test)]` 区域增加
`AdoptionEvidence` readiness gate。它把 provider revision、quality、latency、resources、
privacy、fallback、isolation、replay 和 manual audit 作为独立证据项；任一项缺失时，
五类 consumer 的结果只能标记 `record_only`，不能生成采纳资格。

## WSL 权威验证

在 Ubuntu-24.04 ext4 disposable target 上执行：

```text
wsl.exe -d Ubuntu-24.04 -- bash -lc 'cd <disposable-checkout> && rm -rf /tmp/g5-06-04-target && mkdir -p /tmp/g5-06-04-target && CARGO_TARGET_DIR=/tmp/g5-06-04-target cargo test -p yunxi-core --lib decision_shadow --locked -- --nocapture --test-threads=1'
```

结果：`35 passed; 0 failed; 0 ignored; 748 filtered out`。新增测试
`g5_06_missing_adoption_evidence_keeps_every_consumer_record_only` 通过，G5-06-01/02
的所有 decision_shadow 回归仍通过。

断言内容：

- provider revision 缺失时五类 consumer 全部保持 `record_only`；
- quality、latency、resources、privacy、fallback、isolation、replay、manual audit
  任一布尔证据改为 false，`eligible()` 都 fail-closed；
- primary digest 保留，side effects 为 0，replay bytes 稳定；
- 资格摘要不暴露 provider revision 或 payload 字段。

## 静态门禁与边界

通过 `cargo fmt --all -- --check`、`git diff --check` 和
`python testkit/privacy/g0_scan.py --repo .`；隐私扫描 personal path/credential/private
key 均为 0。`/tmp/g5-06-04-target` 已删除并确认不存在。

这是 test-only 资格闸门，不是生产开关，也不代表 fixture evidence 已满足真实模型质量。
没有下载或加载 Laya 权重，没有 provider、网络、数据库、scheduler、memory/KB 写入或
terminal command execution。G5-06 仍停在 DESIGN，真实 provider 只有在新的施工单和
独立采纳审计通过后才可出现。

