# G3-04 embedding lifecycle evidence

This slice keeps `memory_embeddings` aligned with the memory rows that own
them. Vector writes remain the responsibility of semantic backfill/inline
retrieval; this slice adds only transactional invalidation and delete-only
maintenance.

## Behavior contract

- `reset_all` clears every memory row and every embedding in the same
  transaction, including historical orphan vectors.
- Session reset, browse deletion, and expired short-diary cleanup remove the
  corresponding `fact`/`episode` vector before the owning row can disappear.
- Content, status, or truth-status edits invalidate the old vector. A
  forgotten/rejected row, or a recovered row, starts without a vector and is
  rebuilt by the normal backfill path.
- Automatic strength decay revokes the vector in the same memory maintenance
  pass when a row crosses into `forgotten`.
- Organizer fact updates invalidate the previous content vector in the same
  transaction as the revision and update.
- Deleting an episode also scrubs its id from active fact/episode summaries and
  revision provenance in the same transaction; append-only lifecycle audit
  events remain intact.
- `prune_stale_embeddings` is idempotent and delete-only. It removes orphan
  rows, unknown kinds, forgotten/rejected rows, non-`long_term` episodes, and
  content-digest mismatches while preserving a valid current vector.
- After an episode is deleted, keyword recall, association, historical-event
  recall, and the semantic corpus all return no hit for its former content.
- Reopening the store does not recreate stale vectors: an explicit prune pass
  removes the stale row once and the next pass removes zero rows.
- A failed browse update rolls back both the memory edit and vector deletion.

No schema, dependency, provider, profile, knowledge-base, DecisionPort, or
Laya boundary changed in G3-04.

## Implementation and test anchors

- `crates/yunxi-core/src/memory/write.rs`: reset, organizer update, and short
  diary expiry/forget cleanup.
- `crates/yunxi-core/src/memory/mod.rs`: shared transactional provenance
  scrubber for deleted episode ids.
- `crates/yunxi-core/src/memory/browse.rs`: transactional edit/delete
  invalidation.
- `crates/yunxi-core/src/memory/semantic.rs`: delete-only stale-vector
  reconciler.
- `crates/yunxi-core/src/memory/tests/embedding_lifecycle.rs`: reset/orphan,
  browse edit/delete, failed transaction, expiry, organizer update, recall
  barriers, reopen cleanup, unknown kind, digest mismatch, idempotence, and
  valid-vector preservation tests.

## Verification commands

The authoritative Rust run used a disposable WSL Ubuntu checkout on ext4. The
D: DrvFS checkout is not used for Rust execution because the existing
Linux-native `yunxi-base` code uses Unix APIs and can fail before the target
package is compiled.

```text
set -euo pipefail
rm -rf /tmp/g3-04f-src /tmp/g3-04f-target /tmp/g3-04f-log
mkdir -p /tmp/g3-04f-src
cd <repo-root>
tar --exclude=.git --exclude=target --exclude=.tmp -cf - . \
  | tar -xf - -C /tmp/g3-04f-src
cd /tmp/g3-04f-src
CARGO_TARGET_DIR=/tmp/g3-04f-target CARGO_BUILD_JOBS=1 \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p yunxi-core --lib memory --locked -- --test-threads=1 \
  2>&1 | tee /tmp/g3-04f-log
```

The final run passed `77 passed; 0 failed; 1 ignored; 619 filtered out` with
exit code `0`. An earlier run exposed an invalid three-byte test vector in the
new semantic-coverage assertion; the fixture now stores a minimal valid `f32`
payload and the final checkout compiled cleanly.

## EVIDENCE_SCHEMA

- `run_id`: `g3-04f-20261002-ext4-04`
- `stage/task`: `G3-04 embedding lifecycle and stale-vector cleanup`
- `implementation_commit`: `87288422` (tests) plus `f2fc6646`, `bf65efa0`,
  and `aa4993ea` (lifecycle implementation/follow-ups)
- `recorded_at_utc`: `2026-10-02 08:49:58 UTC`
- `evidence_owner`: `/root` (Lead review); worker requested model
  `GPT-6.1-Sol`; runtime model id not exposed
- `environment`: WSL Ubuntu-24.04 disposable checkout at `/tmp/g3-04f-src`,
  isolated target at `/tmp/g3-04f-target`; one cargo job with
  incremental compilation and debug info disabled
- `test_command`: memory-only command above
- `test_exit_code`: `0`
- `stable_counts`: `77 passed; 0 failed; 1 ignored; 619 filtered out`
- `static_checks`: formatting, diff, metadata, architecture, size, and
  privacy gates all passed. Size report: `362,767` total lines versus the
  corrected `361,444` baseline; no new over-limit file and the gate passed.
- `privacy_findings`: `credential_shape=0`, `personal_path=0`,
  `private_key=0`; existing fixture/public allowlists were unchanged.
- `cleanup`: the disposable `/tmp/g3-04f-src`, `/tmp/g3-04f-target`, and
  `/tmp/g3-04f-log` paths were removed after
  the run; a follow-up check found no cargo or rustc process.
- `unverified`: native Arch Linux, macOS, Windows cargo execution, crash,
  disk-full/lock recovery, concurrent organizer/reset races, full workspace
  tests, provider/model quality, and automatic invocation scheduling for the
  new maintenance pass remain outside this slice.

This record covers G3-04 only. Later slices can decide where maintenance is
scheduled and add crash/concurrency fault injection without weakening the
transaction boundaries established here.
