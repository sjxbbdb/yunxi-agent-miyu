# G3-03 committed-only semantic vector corpus evidence

This slice makes the semantic episode corpus follow the already materialized
long-term boundary. Only episode rows with `retention='long_term'` participate
in semantic coverage, embedding backfill, inline semantic search, and fused
semantic ranking. Facts keep their existing non-forgotten/non-rejected filter;
short-term recall and keyword/context recall are unchanged.

The `long_term` row is the durable boundary written by the organizer after
admission. The lifecycle writer records the candidate-to-committed audit event
for the source short diary before that materialized row can enter this corpus.
This keeps G3-03 deterministic and compatible with legacy durable episodes,
without making SQL parse lifecycle JSON or coupling the vector reader to an
organizer implementation detail.

Historical vectors attached to short, candidate, rejected, or expired episode
rows are not deleted in this slice. The corpus gate ignores them immediately;
G3-04 owns stale-vector deletion/revocation/expiry cleanup and crash recovery.

## Implementation and test anchors

- `crates/yunxi-core/src/memory/semantic.rs`: the shared episode corpus filter
  used by inline embedding, backfill, coverage, and semantic ranking.
- `crates/yunxi-core/src/memory/tests/semantic.rs`: long-term fixture helper,
  historical short-vector coverage test, and backfill test proving only the
  materialized long-term row is embedded.
- No schema, dependency, provider, `DecisionPort`, Laya, profile, or knowledge
  base boundary changes are part of G3-03.

## Verification commands

The authoritative Rust run was performed from a WSL Ubuntu disposable checkout
on ext4. The D: DrvFS checkout is not used for Rust execution because the
existing Linux-native `yunxi-base` code uses Unix APIs and can fail before the
target package is compiled.

```text
rm -rf /tmp/g3-committed-src /tmp/g3-committed-target /tmp/g3-committed-log
mkdir -p /tmp/g3-committed-src
cd <repo-root>
tar --exclude=.git --exclude=target --exclude=.tmp -cf - . \
  | tar -xf - -C /tmp/g3-committed-src
cd /tmp/g3-committed-src
CARGO_TARGET_DIR=/tmp/g3-committed-target CARGO_BUILD_JOBS=1 \
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

- `run_id`: `g3-03-20261002-ext4-01`
- `stage/task`: `G3-03 committed-only semantic vector corpus`
- `implementation_commit`: `5dcea12b882a29808a8d2e22f21183b0330c7c60`
- `recorded_at_utc`: `2026-10-02 07:56:54 UTC`
- `evidence_owner`: `/root` (Lead review); worker requested model
  `GPT-6.1-Sol`; runtime model id not exposed
- `environment`: WSL Ubuntu disposable checkout at `/tmp/g3-committed-src`,
  isolated target at `/tmp/g3-committed-target`; one cargo job with
  incremental compilation and debug info disabled
- `test_command`: memory-only command above
- `test_exit_code`: `0`
- `stable_counts`: `67 passed; 0 failed; 1 ignored; 619 filtered out`
- `static_checks`: all six formatting, diff, metadata, architecture, size,
  and privacy gates passed. Size report: `362,231` total lines versus the
  corrected `361,444` baseline; no new over-limit file and the gate passed.
- `privacy_findings`: `credential_shape=0`, `personal_path=0`,
  `private_key=0`; existing fixture/public allowlists were unchanged.
- `cleanup`: the disposable source, target, and log paths were removed after
  the run; the interrupted worker cargo process was terminated by exact PID
  and a follow-up check found no cargo or rustc process.
- `unverified`: native Arch Linux, macOS, Windows cargo execution, crash,
  disk-full/lock recovery, concurrent organizer/reset races, full workspace
  tests, provider/model quality, and G3-04 stale-vector cleanup remain outside
  this slice.

This record covers G3-03 only. G3-04 will cover deduplication,
revoke/expiry, stale-vector deletion, and crash recovery semantics.
