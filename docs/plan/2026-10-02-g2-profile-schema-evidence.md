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
