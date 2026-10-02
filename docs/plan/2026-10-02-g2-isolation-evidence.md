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

