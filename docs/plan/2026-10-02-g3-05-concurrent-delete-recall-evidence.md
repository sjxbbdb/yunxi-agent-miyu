# G3-05 concurrent delete/recall evidence

## Scope

This slice adds one deterministic regression for the provenance barrier on
evicted memory carriers. A linked assistant/tool-report carrier and an
unlinked user carrier contain the same marker. A reader thread performs a
pre-delete recall, then waits at a barrier while the main thread deletes the
fact. Only after the delete transaction commits is the reader released to run
post-delete keyword recall and direct browse. This models a reader that remains
active across the delete without relying on sleeps or scheduler timing.

The post-delete assertions require both read paths to hide the linked carrier,
while retaining the unlinked carrier. The test uses only `MemoryStore` APIs;
the provenance edge remains the typed `(fact, id)` reference rather than a
text match.

## Test anchor

- `crates/yunxi-core/src/memory/tests/store.rs::concurrent_recall_and_browse_honor_a_committed_memory_tombstone`

## Validation

Run in the required WSL Ubuntu-24.04 disposable ext4 checkout, with one cargo
job at a time:

```text
cargo test -p yunxi-core --lib memory::tests::store::concurrent_recall_and_browse_honor_a_committed_memory_tombstone --locked -- --exact --test-threads=1
cargo fmt --all -- --check
git diff --check
```

The Windows host checkout is not a valid Rust execution environment for this
repository because `yunxi-base` intentionally compiles Unix-only path and
sandbox code. The ext4 run must supply the final pass count and leave no cargo
or rustc process behind.

Observed on a disposable WSL Ubuntu-24.04 ext4 copy (one cargo job):

- the targeted `yunxi-core` test passed (`1 passed`, `706 filtered out`);
- host `cargo fmt --all -- --check` and `git diff --check` passed;
- the disposable source and target directories were removed after the run.

## Boundary

This test proves the committed-delete ordering and the two evicted read paths.
It does not claim a cross-database transaction: memory tombstones and state
carriers remain separate SQLite databases, and an in-flight read that began
before a delete may still observe its pre-delete snapshot. Crash-after-delete,
semantic embedding races, and transcript/shell carrier boundaries remain
separate G3-05 evidence slices.
