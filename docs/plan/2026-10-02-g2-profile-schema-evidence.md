# G2-01 profile schema evidence

Status: additive schema and isolated CRUD only. This slice does not read or
rewrite the legacy `profile.md`, assemble prompts, infer claims from messages,
or write any memory/embedding table.

## Schema and migration

`2026-10-02-g2-profile-schema` is a named, idempotent migration. It creates:

- `profile_claims`: stable text `claim_id`, explicit `owner_scope` (empty is
  the global profile), key/value, `confirmed|inferred` certainty, bounded
  source kind/reference, observed/updated timestamps, `active|revoked` status,
  positive revision, and a unique `(owner_scope, key, revision)` constraint.
- `relationship_events`: stable text `event_id`, required non-empty
  `persona_scope`, event kind, bounded summary/optional payload, timestamps and
  source metadata, plus `active|revoked` status. Events are append-only; revoke
  only changes status.

Both tables have scope/status/time indexes. The migration is recorded in
`schema_migrations` and never changes `PRAGMA user_version`. G2-02 owns atomic
rename/delete persona-scope migration; G2-03 owns Agent/prompt/phase lifecycle
integration.

## API and isolation

`ConversationDb` and the thin `StateStore` facade provide insert/upsert/list/
revoke operations. Mutating profile upserts use an immediate transaction;
single-row event inserts and revokes are SQLite-atomic. Default list methods
exclude revoked rows and require an exact scope. Validation bounds all user
metadata and rejects absolute local paths, file URLs, NULs, and common secret
assignments in source references. No path or database handle is part of the
serialized public records.

## Verification

The targeted profile tests cover global/persona scope isolation,
confirmed/inferred round trips, revoked-row visibility, required persona scope,
bounded payload/source validation, duplicate revision rollback, and the absence
of embedding tables. Named migration tests cover fresh creation, rerun/no-op,
unknown migration IDs, and unchanged `user_version`.

Run from WSL Ubuntu 24.04:

```text
cd /tmp/yunxi-g2-src
CARGO_TARGET_DIR=/tmp/yunxi-g2-ext4-target CARGO_BUILD_JOBS=1 \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1
CARGO_TARGET_DIR=/tmp/yunxi-g2-ext4-target CARGO_BUILD_JOBS=1 \
  CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 \
  cargo test -p yunxi-core --lib state::migrations::named --locked -- --test-threads=1
```

Observed results on 2026-10-02:

- `state::tests::profile`: **3 passed, 0 failed, 665 filtered**;
- `state::migrations::named`: **2 passed, 0 failed, 666 filtered**;
- `cargo fmt --all -- --check`: passed;
- `cargo metadata --no-deps --format-version 1`: passed;
- `git diff --check`: passed.

The first two WSL attempts used a target under `D:` and were stopped after the
`yunxi-base` build script entered an uninterruptible DrvFS `p9_cli` wait at
`include-flate`; they had no source error or test result and are not counted as
passes. The successful run copied only the source (excluding `.git`, `target`,
and `.tmp`) to WSL ext4 at `/tmp/yunxi-g2-src`, with the target at
`/tmp/yunxi-g2-ext4-target`. These temporary trees and logs are removed after
the audit; no raw DB, path, or secret artifact is committed.

The Windows host cannot compile this Unix-targeted crate directly; WSL ext4 is
the required verification environment for this slice.

## Evidence record

This record follows the repository `EVIDENCE_SCHEMA`; the stable summary is
kept here after the temporary source/target/log trees were removed.

```text
run_id: g2-01-20261002-ext4-01
stage/task: G2-01 profile schema and named migration
commit_sha: 507fdaa6a9ec8ce035b0fb5831c5c37980a97e98
recorded_at_utc: 2026-10-02 06:03:08 UTC (record update; historical execution window not retained)
evidence_owner: /root/g1_exit_audit (worker; requested_model=gpt-6.1-sol; actual_model=未暴露)
environment: WSL Ubuntu-24.04; source copied to /tmp/yunxi-g2-src; target /tmp/yunxi-g2-ext4-target
commands:
  cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1
  cargo test -p yunxi-core --lib state::migrations::named --locked -- --test-threads=1
  cargo fmt --all -- --check
  cargo metadata --no-deps --format-version 1 --locked
  git diff --check
exit_codes: 0, 0, 0, 0, 0
stable_counts: profile 3 passed/0 failed/665 filtered; named 2 passed/0 failed/666 filtered
failure_reason: D: DrvFS attempts stopped in yunxi-base include-flate p9_cli wait; not counted as passes
unverified: no real provider cache hit; no Arch/macOS/Windows native build; no crash/disk-full/lock contention; no independent replay after temp cleanup
cleanup: source, target, logs and workspace .tmp removed; no raw DB/path/secret artifact committed
```

The exact historical execution minute was not retained; `recorded_at_utc` is
the time this evidence record was corrected for audit. The record is a
historical stable summary, not a claim of a retained raw log. The follow-up
hardening run below is the current reproducible owner-scope regression.

### Follow-up scope hardening

```text
run_id: g2-01-20261002-ext4-hardening-01
stage/task: G2-01 owner_scope whitespace invariant
commit_sha: cc96afed7c896b9237294306575992734cdcbb85
recorded_at_utc: 2026-10-02 06:03:08 UTC
evidence_owner: /root (lead replay; worker implementation by /root/profile_scope_hardening)
environment: WSL Ubuntu-24.04; source copied to /tmp/yunxi-g2-hardening-src; target /tmp/yunxi-g2-hardening-target
command: cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1
exit_code: 0
stable_counts: 4 passed/0 failed/0 ignored/667 filtered; includes whitespace reject and global empty-scope acceptance
failure_reason: none on WSL ext4; Windows host attempt was not counted because this Unix-targeted crate emits existing std::os::unix/libc errors
unverified: full suite and native Arch/macOS/Windows builds remain out of scope for this slice
cleanup: both temporary source and target removed; no cargo/rustc remained; no raw test artifact committed
```
