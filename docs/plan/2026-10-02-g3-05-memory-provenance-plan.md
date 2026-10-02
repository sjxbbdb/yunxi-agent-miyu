# G3-05 memory provenance plan

## Why this slice exists

G3-04 now prevents a deleted `fact` or `episode` from being recalled from the
memory database, its embedding rows, revisions, or an older imported archive.
The remaining gap is the text copied into the state side of the system:
`evicted_turns`, compact summary rows, and compact transcript extras. Those
carriers currently store conversation text, turn ids, or a short
`remember_fact` label, but not a stable `fact_id`/`episode_id` edge. Deleting by
matching text would be unsafe because the same sentence can be legitimate in
another turn.

This plan turns that gap into an explicit follow-up instead of silently
claiming that the deletion contract is complete.

## G3-05-01 implemented slice

The first implementation slice is deliberately narrower than the eventual
schema above: it covers only `evicted_turn` carriers. The state database uses
the numeric `evicted_turns.id` as `carrier_id`, keeps the typed `fact` or
`episode` id, and writes the `tool_report` relation. `summary_turn` and
`transcript` rows are not yet written to `memory_provenance`; their redaction
and read barriers remain planned work, not an acceptance claim for this
slice.

The shipped slice now provides:

- `MemoryRef { kind, id }` on `EvictedTurn`; the engine accepts a reference
  only from the exact structured `remember_fact` tool-report wrapper.
- Same-transaction provenance writes in both the normal memory archive path
  and the state-side `archive_and_delete_visible_turns` path.
- Tombstone-aware keyword and semantic evicted-context filtering, plus
  provenance cleanup when evicted rows, embeddings, sessions, or the whole
  evicted context are removed.
- Additive migration of old state databases, including normalization of the
  development-only `memory_ref` relation to `tool_report` without failing on
  a duplicate current row.

Evidence for this slice is external to the model self-report: WSL ext4 runs
passed `yunxi-core` memory store (12), browse (2), compact (21), and
`yunxi-engine` context (36) tests. The context count includes the real archive
path test that checks the typed row and verifies that deleting the fact hides
only the linked tool report while the ordinary user row remains searchable.

## G3-05-02 implemented slice: summary carriers

The second slice adds typed provenance to compact summary rows without changing
the summary/checkpoint wire shape. A named migration creates the additive
`conversation.db` table with `carrier_id TEXT`; this is intentionally separate
from the existing evicted-context database table. Compaction now collects only
typed `remember_fact` report references from folded turns, inherits references
from the previous summary, de-duplicates them, and writes the rows in the same
transaction as the new summary row. `reset`, persona-context reset, and both
reversible-summary undo paths remove the summary carrier rows in their own state
transaction.

Checkpoint rendering consults the memory tombstone table by `(kind, id)`. If any
linked fact or episode is deleted, the complete summary carrier is replaced by
the fixed marker `[conversation summary redacted: linked memory was deleted]`
and its extras are not rendered. This is a carrier-level barrier: it does not
attempt unsafe substring surgery and it does not infer a link from prose. The
same decision is used by the compaction anchor, so a deleted summary cannot be
fed back into the next summarization request.

Evidence for G3-05-02 (WSL Ubuntu ext4, one cargo job at a time):

- `yunxi-core` `state::tests::compact`: 22 passed, including typed summary
  provenance round-trip and undo cleanup.
- `yunxi-core` `memory::tests::store`: 12 passed, including the tombstone query
  API used by the summary barrier.
- `yunxi-engine` `agent::tests::context`: 37 passed, including deletion of a
  linked fact and fixed-marker checkpoint redaction.

The transcript carrier remains deliberately out of this slice. Transcript
paths are still legacy/unknown until a stable logical id and a read barrier are
implemented together; no transcript text is guessed or scrubbed here.

## Current evidence and boundaries

- `yunxi-base/src/memory_types.rs::EvictedTurn` has `source_id`, role, time,
  text, and ownership, but no memory reference.
- `yunxi-engine/src/agent/context.rs::evicted_turn_entries` creates separate
  archived rows for user, assistant, tool, and follow-up content. The
  `remember_fact` tool report already contains a numeric id in its compact JSON,
  so tool-report rows can acquire typed provenance without parsing free text.
- `yunxi-core/src/state/conversation_db/history.rs::replace_visible_with_summary`
  stores `compact_hidden_json`, the model-written summary, and `compact_extras`
  in one summary turn. The summary prose is not a reliable memory boundary.
- `yunxi-engine/src/agent/compact_extras.rs` writes transcript files and stores
  their paths. A transcript can contain copied conversation text and may be
  read later through a tool.
- Legacy carriers without provenance must remain readable. They are marked
  unknown rather than guessed by content or retroactively deleted.

## Proposed schema

Add a state-database table, migrated independently of the persona memory DB:

```sql
CREATE TABLE IF NOT EXISTS memory_provenance (
    carrier_kind TEXT NOT NULL CHECK (
        carrier_kind IN ('evicted_turn', 'summary_turn', 'transcript')
    ),
    carrier_id TEXT NOT NULL,
    memory_kind TEXT NOT NULL CHECK (memory_kind IN ('fact', 'episode')),
    memory_id INTEGER NOT NULL,
    relation TEXT NOT NULL CHECK (relation IN ('tool_report', 'summary_input', 'transcript_input')),
    session_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    PRIMARY KEY (carrier_kind, carrier_id, memory_kind, memory_id, relation)
);
CREATE INDEX IF NOT EXISTS idx_memory_provenance_memory
    ON memory_provenance(memory_kind, memory_id, carrier_kind, carrier_id);
```

`carrier_id` is a state-side id: the numeric `evicted_turns.id`, the summary
`turn_id`, or a stable transcript id derived from the summary turn and export
sequence. It is never a filesystem path. The table stores ids and relation
metadata only, not memory text.

## Write path

1. Extend the pure `EvictedTurn` value with a small list of typed
   `MemoryRef`s. Do not put the memory body in this list.
2. In `context.rs`, attach a `fact` reference to a tool-report archive row
   only when the persisted `remember_fact` report parses as the expected
   wrapper and contains a positive id. User/assistant rows remain unlinked.
3. In `memory/evicted.rs`, insert the `evicted_turns` row and its provenance
   rows in the same state-database transaction. A malformed or legacy report
   gets no guessed reference.
4. During compaction, collect typed references from folded tool reports and
   pass them to `replace_visible_with_summary`. Store them for the summary
   `turn_id`; keep the existing summary bytes and cache contract unchanged.
5. Give transcript extras a stable logical id and store the same references
   before exposing the path. The file format may gain a machine-readable
   provenance header, but the header must not include private memory text.

## Delete and read behavior

Memory deletion remains authoritative in the persona memory DB and writes the
existing tombstone in its transaction. State-side deletion is deliberately
not a cross-database transaction: reads consult the memory tombstone before
returning a carrier, and a best-effort reconciler removes stale provenance
rows/embeddings after a restart.

- Evicted keyword and semantic queries join `memory_provenance` by carrier id
  and exclude a carrier when any referenced memory id is tombstoned. This is an
  exact id check, never text matching.
- Summary checkpoint rendering omits a summary or transcript reference whose
  provenance is tombstoned, replacing it with a deterministic redaction marker
  so unrelated live context is not reinterpreted as a deleted memory.
- Transcript read helpers perform the same check before serving a path. A
  missing/legacy provenance row is treated as `unknown`, not as proof that the
  content is safe or deleted; the existing privacy policy decides whether it
  may be exposed.
- `reset_all` and session reset keep their current broad state cleanup. Import
  and rollback must copy the provenance table together with state DBs and run
  the same tombstone reconciliation before install.

## Migration and failure policy

- Migration is additive and idempotent. Existing `evicted_turns`, summaries,
  and transcript files remain valid without references.
- A crash between the memory transaction and the state reconciler is safe:
  tombstone-aware reads hide the carrier first; the next maintenance pass can
  delete stale state rows.
- A missing or malformed provenance table fails closed for newly written
  references but does not corrupt old state. No free-text scrub is attempted.
- The table is per state installation and follows the existing transfer/
  rollback boundaries; it is not shared with the memory vector tables.

## Acceptance matrix for G3-05

1. A `remember_fact` tool report archived as an evicted tool row gets one
   typed reference; unrelated user/assistant rows get none.
2. Deleting that fact makes keyword and semantic evicted recall exclude the
   tool row after restart; its vector is not returned.
3. A compact summary carrying the same reference is omitted/redacted after
   deletion, while a summary with no dead reference remains byte-identical.
4. Transcript read is denied or redacted after deletion; legacy transcript
   files without a reference are not changed by heuristic text matching.
5. Old state databases migrate without data loss; import, rollback, and
   reset preserve the provenance invariants.
6. Concurrent delete/recall and crash-after-delete tests show no deleted id in
   keyword, semantic, association, compact, restore, or backup-import paths.

## Explicit non-goals

G3-05 must not rewrite all historical text, infer references from content,
merge the memory and knowledge-base databases, or change the existing persona,
vector, DecisionPort, Laya, or provider boundaries. Full implementation should
start only after the schema and carrier-level redaction tests above are agreed
and can be run in the disposable WSL ext4 harness.
