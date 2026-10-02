# G3-02 deterministic memory admission evidence

This slice adds a deterministic admission gate before automatic organizer output
can promote short diary material into facts or long-term episodes. It is a
bounded baseline for the later `DecisionPort`/Laya seam: no model, provider,
network call, permission elevation, or memory-writer authority is introduced.
The existing memory tables, embeddings, profile path, and knowledge-base path
remain unchanged.

The gate evaluates each source diary independently and applies an all-of rule
per organizer action. A mixed action is discarded if any referenced source is
not admitted, while unrelated actions in the same batch continue to be
processed. Organizer-generated text is checked again so a safe source cannot
promote a sensitive summary.

## Rule contract

| Signal | Decision | Stable reason/class |
| --- | --- | --- |
| Credential, secret, one-time token, or credential-shaped token | reject, before every other rule | `sensitive_*` / sensitive class |
| Explicit force-long-term request | admit unless sensitive | `force_long_term` / explicit |
| Stable preference or default behavior | admit | `stable_preference` |
| Project, repository, system, environment, or configuration constraint | admit | `project_constraint` or `system_constraint` |
| Future value, repeated recall, or “remember for later” signal | admit | `future_value` |
| Temporary/one-off signal | reject | `ephemeral` |
| No stable signal | abstain; automatic organizer promotion is not allowed | `no_long_term_signal` / ambiguous |

Credential-shaped detection covers the supported local patterns (`sk-`,
`ghp_`, `github_pat_`, `AIza`, `AKIA`) without storing or logging the value.
Audit events retain only the stable reason, source IDs, owner scope, and a
content digest; raw diary or generated content is not copied into the audit
metadata or warning logs.

## Implementation and test anchors

- `crates/yunxi-core/src/memory/admission.rs`: versioned decision types,
  deterministic classifier, sensitivity gate, and raw-content-free candidate
  metadata seam.
- `crates/yunxi-core/src/memory/write.rs`: per-action all-of admission,
  generated-output sensitivity recheck, lifecycle reason codes, and metadata
  digest usage.
- `crates/yunxi-core/src/memory/tests/admission.rs`: rule matrix, token-shaped
  secret coverage, metadata non-leak check, mixed-source isolation, generated
  output gate, and end-to-end sensitive-source rejection.
- Existing access/lifecycle fixtures now carry explicit long-term signals where
  the new contract requires them; no production behavior is weakened.

## Verification commands

The authoritative Rust run was performed from a WSL Ubuntu-24.04 disposable
checkout on ext4. The D: DrvFS checkout is not used for Rust execution because
the existing Linux-native `yunxi-base` code uses Unix APIs and can fail before
the target package is compiled.

```text
rm -rf /tmp/g3-admission-src /tmp/g3-admission-target /tmp/g3-admission-log
mkdir -p /tmp/g3-admission-src
cd <repo-root>
tar --exclude=.git --exclude=target --exclude=.tmp -cf - . \
  | tar -xf - -C /tmp/g3-admission-src
cd /tmp/g3-admission-src
CARGO_TARGET_DIR=/tmp/g3-admission-target CARGO_BUILD_JOBS=1 \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p yunxi-core --lib memory --locked -- --test-threads=1
```

Before the code push, the following gates passed from the working tree:
`cargo fmt --all -- --check`, `git diff --check`,
`cargo metadata --no-deps --format-version 1 --locked`,
`python test_scripts/arch_dep_check.py`,
`python test_scripts/refactor_size_report.py --check`, and
`python testkit/privacy/g0_scan.py --repo .`.

## EVIDENCE_SCHEMA

- `run_id`: `g3-02-20261002-ext4-01`
- `stage/task`: `G3-02 deterministic memory admission`
- `implementation_commit`: `66b07184` (includes the privacy-safe fixture
  correction after the first implementation push)
- `recorded_at_utc`: `2026-10-02 07:31:41 UTC`
- `evidence_owner`: `/root` (Lead review); worker requested model
  `GPT-6.1-Sol`; runtime model id not exposed
- `environment`: WSL Ubuntu-24.04, ext4 disposable checkout at
  `/tmp/g3-admission-src`; isolated target at `/tmp/g3-admission-target`; one
  cargo job with incremental compilation and debug info disabled
- `test_command`: memory-only command above
- `test_exit_code`: `0`
- `stable_counts`: `65 passed; 0 failed; 1 ignored; 619 filtered out`
- `static_checks`: all six formatting, diff, metadata, architecture, size, and
  privacy gates passed. Size report: `362,110` total lines versus the corrected
  `361,444` baseline; no new over-limit file and the gate passed.
- `privacy_findings`: `credential_shape=0`, `personal_path=0`,
  `private_key=0`; the scanner's existing fixture/public allowlists were
  unchanged. The new admission tests contain only visibly fake token-shaped
  samples and did not produce findings.
- `cleanup`: the disposable source, target, and log paths were removed after
  the run; a follow-up process check found no `cargo` or `rustc` process.
- `unverified`: native Arch Linux, macOS, Windows cargo execution, crash,
  disk-full/lock recovery, concurrent organizer/reset races, full workspace
  tests, provider/model quality, and the future Laya/DecisionPort implementation
  are outside this slice.

This record covers G3-02 only. G3-03 will enforce committed-only entry into
long-term vector storage; G3-04 will cover deduplication, revoke/expiry,
deletion, and crash recovery semantics.
