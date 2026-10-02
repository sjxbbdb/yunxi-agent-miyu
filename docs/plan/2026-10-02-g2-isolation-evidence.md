# G2-03 profile/relationship scope isolation

This slice keeps structured profile and relationship metadata separate from
`profile.md`, prompt assembly, memory/embedding tables, and DecisionPort/Laya.
It closes the user-facing scope boundary before lifecycle integration:

- profile reads and revokes bind `claim_id` **and** `owner_scope`;
- relationship reads and revokes bind `event_id` **and** `persona_scope`;
- wrong-scope operations return the existing `Option`/`false` result and leave
  the owning scope active;
- revoked rows remain available only through `include_revoked` or an exact
  scope-aware lookup, and survive a database reopen;
- the global profile (`owner_scope = ''`) is never returned by a persona scope.

The unscoped `*_by_id` and revoke methods remain for internal migration/repair
compatibility only. New user-facing callers must use the `*_in_scope` methods
exposed by both `ConversationDb` and the `StateStore` facade.

`ProfileClaimCertainty::Confirmed` is accepted only with explicit provenance:
`user_edit`, `user_confirmation`, or `manual_import`. Conversation/model
provenance remains valid for `Inferred` claims, and an upsert cannot promote an
inferred claim to confirmed without changing its source to an explicit one.

No profile row is copied to memory or embeddings in this slice.

## Verification

Run from a WSL Ubuntu ext4 checkout (the repository's D: DrvFS path can hang
inside the `yunxi-base` build script):

```text
cargo fmt --all -- --check
cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1
cargo metadata --no-deps --format-version 1
git diff --check
python test_scripts/arch_dep_check.py
python test_scripts/refactor_size_report.py --check
python testkit/privacy/g0_scan.py --repo .
```

## EVIDENCE_SCHEMA

- `run_id`: `g2-03-20261002-ext4-01`
- `stage/task`: `G2-03 profile/relationship scope isolation`
- `implementation_commit`: `60fe8e67`
- `recorded_at_utc`: `2026-10-02 06:20:25 UTC`
- `evidence_owner`: `/root` (Lead review); implementation worker model was
  requested as `GPT-6.1-Sol`, runtime model id not exposed
- `environment`: WSL Ubuntu-24.04, ext4-backed temporary checkout at
  `/tmp/yunxi-g2-isolation-src`; build output at
  `/tmp/yunxi-g2-isolation-target`; single cargo job
- `test_command`: `CARGO_TARGET_DIR=/tmp/yunxi-g2-isolation-target CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p yunxi-core --lib state::tests::profile --locked -- --test-threads=1`
- `test_exit_code`: `0`
- `stable_counts`: `6 passed; 0 failed; 667 filtered out`
- `static_checks`: `cargo fmt --all -- --check`, `cargo metadata --no-deps --format-version 1 --locked`, `git diff --check`, `arch_dep_check.py`, `refactor_size_report.py --check`, and `g0_scan.py` all exited `0`
- `privacy_findings`: `credential_shape=0`, `personal_path=0`,
  `private_key=0`; fixture/public allowlists contained only pre-existing paths
- `repair_history`: an initial run exposed a test-fixture source-kind mistake
  (5 passed, 1 failed); the fixture was corrected to mark the inferred claim as
  `conversation`, then the exact ext4 command above passed
- `unverified`: native Arch Linux, macOS, Windows cargo execution, crash/disk-full/lock recovery, and full-workspace test suite
- `cleanup`: the ext4 checkout and target are disposable and must be removed
  after the run; no raw profile values, database contents, credentials, or
  host-local paths are part of this evidence record

