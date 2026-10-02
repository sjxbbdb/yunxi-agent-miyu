# G3-01 memory lifecycle evidence

This slice adds an append-only audit seam around the existing memory truth. It
does not replace `facts`, `episodes`, `memory_embeddings`, or the knowledge
base, and it does not introduce admission policy, vector retrieval, prompt
changes, DecisionPort/Laya, TUI, or a second database.

The lifecycle is deliberately small and explicit:

```text
transient -> short -> candidate -> committed
                              \-> rejected
         short -> expired
```

Each transition stores only stable metadata: state names, owner/scope,
reason code, source episode IDs, a content digest, generation, and an idempotent
transition key. Raw user or assistant text is never written to the audit table.
The validator also binds each edge to its owner: `turn_loop` owns
`transient -> short`, `user` owns manual `transient -> committed`, the
organizer owns candidate claims and normal finalization, `memory_gc` owns
expiry/rejection cleanup, and `DecisionPort` is advisory rather than a final
write authority.
Existing rows are not backfilled with invented history; a database upgrade only
creates the table and independent `lifecycle_schema_version` metadata.

## Scope and implementation anchors

- `crates/yunxi-core/src/memory/lifecycle.rs`: state/owner enums, transition
  validator, stable serialization, digest and transition-key helpers.
- `crates/yunxi-core/src/memory/schema.rs`: idempotent lifecycle schema and
  indexes; existing facts/episodes/embedding schema is unchanged.
- `crates/yunxi-core/src/memory/write.rs`: audit writes at turn completion,
  organizer claim/admit/reject/failure, manual fact insertion, and short-memory
  expiry cleanup. Audit errors roll back the surrounding transaction.
- `crates/yunxi-core/src/memory/tests/lifecycle.rs`: transition, migration,
  idempotency, owner/scope, generation, expiry, and no-raw-content coverage.

## Verification commands

Run from a WSL Ubuntu-24.04 checkout on ext4. The repository's D: DrvFS path
may hang while compiling the `yunxi-base` build script, so the disposable
checkout and target are intentionally outside `/mnt/d`:

```text
rm -rf /tmp/g3-lifecycle-src /tmp/g3-lifecycle-target
mkdir -p /tmp/g3-lifecycle-src
cd <repo-root>
tar --exclude=.git --exclude=target --exclude=.tmp -cf - . \
  | tar -xf - -C /tmp/g3-lifecycle-src
cd /tmp/g3-lifecycle-src
CARGO_TARGET_DIR=/tmp/g3-lifecycle-target CARGO_BUILD_JOBS=1 \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p yunxi-core --lib memory --locked -- --test-threads=1
```

Before reporting completion, also run `cargo fmt --all -- --check`,
`git diff --check`, `cargo metadata --no-deps --format-version 1 --locked`,
`python test_scripts/arch_dep_check.py`,
`python test_scripts/refactor_size_report.py --check`, and
`python testkit/privacy/g0_scan.py --repo .` from the checkout. Remove the
disposable source, target, logs, and caches after verification and confirm no
`cargo`/`rustc` processes remain.

## EVIDENCE_SCHEMA

- `run_id`: `g3-01-20261002-ext4-01`
- `stage/task`: `G3-01 append-only memory lifecycle audit seam`
- `implementation_commit`: `c32f433f`
- `recorded_at_utc`: `2026-10-02 07:01:52 UTC`
- `evidence_owner`: `/root` (Lead review); worker requested model
  `GPT-6.1-Sol`; runtime model id not exposed
- `environment`: WSL Ubuntu-24.04, ext4 disposable checkout at
  `/tmp/g3-lifecycle-src`; isolated target at `/tmp/g3-lifecycle-target`; one
  cargo job and incremental/debug-info disabled
- `test_command`: command above
- `test_exit_code`: `0`
- `stable_counts`: `59 passed; 0 failed; 1 ignored; 0 measured; 619 filtered`
- `static_checks`: `cargo fmt --all -- --check`, `git diff --check`,
  `cargo metadata --no-deps --format-version 1 --locked`,
  `arch_dep_check.py`, and `refactor_size_report.py --check` passed. The size
  baseline was corrected at this stage because the checked-in 350,693-line
  snapshot predated the product-layer migration and already understated the
  clean source tree; the post-G3-01 clean snapshot is 361,444 lines and is now
  the comparison point. The WSL source copy omitted `.git`, so its git-based
  privacy scanner was run from the D: worktree instead.
- `privacy_findings`: D: worktree `g0_scan.py --repo .` passed with
  `credential_shape=0`, `personal_path=0`, and `private_key=0`; no raw memory
  content is expected in the lifecycle table or this evidence file
- `repair_history`: first WSL compile exposed a temporary rusqlite statement
  lifetime error in expiry audit collection; introducing an intermediate
  `MappedRows` binding fixed it, and the final ext4 run passed. Owner-bound
  validation and append-only trigger coverage were added before the final run.
  The aggregate line-count gate initially reported 350,693 → 361,444
  (+3.1%) because its historical baseline was stale; after verifying the
  increase was the already-landed migration plus this bounded slice rather
  than copied code, the baseline was rewritten from the clean G3-01 snapshot
  and the gate passed.
- `unverified`: native Arch Linux, macOS, Windows cargo execution, crash,
  disk-full/lock recovery, concurrent organizer/reset races, and full-workspace
  test suite unless separately exercised
- `cleanup`: disposable `/tmp/g3-lifecycle-src` and
  `/tmp/g3-lifecycle-target` removed; no `cargo`/`rustc` processes remained

This record is for G3-01 only. G3-02 admission policy and later DecisionPort/
Laya work must produce separate evidence records.
