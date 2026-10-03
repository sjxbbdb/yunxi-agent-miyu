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

## G3-05-03b implemented slice: transcript carrier identity and writes

This slice adds the write-side transcript carrier contract without introducing
the read/grep/run-command barriers. Compact extras keep the existing
`transcripts` path list and add an optional, position-aligned `transcript_ids`
list, so old JSON remains readable. Each new id is a deterministic BLAKE3
digest of a version tag, the session id, and the ordered folded turn ids. The
filesystem path remains a display/lookup key and is never used as
`memory_provenance.carrier_id`.

The additive named migration `2026-10-02-g3-05-transcript-carriers` stores the
path-to-id registry and the summary turn that introduced it. Compaction writes
the registry row and `transcript_input` links in the same transaction as the
summary row. Undo and session/persona reset remove both registry and typed
links; transcript files themselves are retained. Legacy transcript paths
without ids resolve as unknown until a later write assigns one.

Evidence for this write-side slice (WSL Ubuntu-24.04, ext4 disposable copy,
one cargo job at a time):

- `yunxi-core` `state::tests::compact`: 24 passed, including migration-backed
  path lookup and undo/reset cleanup.
- `yunxi-core` `memory::tests::store`: 12 passed, including the existing
  tombstone and memory/profile isolation regressions.
- `yunxi-core` `memory::tests::browse`: 3 passed, including the direct
  tombstone-aware evicted browse barrier.
- `yunxi-engine` `agent::tests::compact_extras`: 17 passed, including stable
  logical ids, legacy extras parsing, and transcript chain compatibility.
- `yunxi-engine` `agent::tests::context`: 37 passed, covering compaction and
  replay consumers of the updated extras/state API.

The disposable copy was removed after these runs. The read/grep/run-command
barrier is intentionally not part of this evidence and remains the next
implementation slice.

## G3-05-04 implemented slice: transcript read barrier

The Agent now binds a local, session-scoped transcript access guard after its
`StateStore` and `MemoryStore` are constructed. The guard is attached to the
existing `ToolRegistry` and is copied with registry clones, so it does not
change provider-visible tool definitions or introduce a second dispatch path.
`read`, `grep`, and `glob` paths under the current session's
`state/compact/<session>` root are checked against the typed transcript
carrier registry. A registered carrier with no tombstoned memory references is
readable; a carrier whose exact `fact`/`episode` reference is tombstoned is
denied. A path in that compact root with no registry row is treated as a
legacy/unknown carrier and denied rather than guessed safe. Provenance lookup
errors also fail closed.

`run_command` receives the same check when a lightweight path token resolves
inside the current compact-session root. This covers the emitted absolute path
and relative/`~/` spellings that resolve to that root. Shell syntax, variables,
command substitutions, and embedded script strings are not parsed; transcript
contents are never searched or scrubbed. Structured `yunxi tool-call` reads
therefore re-enter the same registry guard, while arbitrary shell grammar
mediation remains outside this slice.

Evidence for this read-side slice (WSL Ubuntu-24.04, ext4 disposable copy,
one cargo job at a time):

- `yunxi-engine` `tools::transcript_guard`: 6 passed, covering path-scope
  separation, lexical traversal normalization, unknown legacy denial, live
  registered readability, tombstoned-reference denial, and the command-token
  parser regression.
- The Windows-host `cargo test` attempt was not used as evidence: this
  repository's source intentionally compiles Unix-only `yunxi-base` modules
  and fails before reaching the changed code. WSL is the authoritative Rust
  harness for this Linux branch.

The disposable copy must be removed after the broader G3-05 regression run;
the barrier is not yet a claim that every historical session or every shell
grammar has been exhaustively mediated.

## G3-05-05 implemented slice: session deletion and idle-sweep GC

`ConversationDb::delete_session` and the idle one-shot session sweep now delete
`memory_provenance` and `transcript_carriers` by the same `session_id` inside
the existing SQLite transaction, before removing the session row. This closes
the orphan-index gap without touching sibling sessions, global profile data, or
filesystem transcript files. A failed later delete still rolls the metadata
cleanup back with the session transaction.

Evidence for this slice (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job
at a time):

- `yunxi-core` `state::tests::compact`: 25 passed, including direct session
  deletion removing both provenance tables.
- `yunxi-core` `state::tests::sessions`: 16 passed, including idle ask-session
  sweep removing both provenance tables while leaving the user session intact.
- `cargo fmt --all -- --check` and `git diff --check`: passed on the host
  checkout before the ext4 run.
- Temporary source and target directories were removed after the run; no cargo
  or rustc process remained.

This does not yet prove import/rollback transfer of the provenance tables or
concurrent delete/recall behavior; those remain the next G3-05 boundaries.

## G3-05-06 implemented slice: transfer round-trip coverage

The existing SQLite transfer round-trip fixture now contains one
`memory_provenance` row and one `transcript_carriers` row for the exported
conversation session. The import assertion verifies both rows survive the
SQLite snapshot/archive/import path alongside the existing session data. This
is coverage of the registered `state/conversation.db` unit; it does not change
the transfer registry or add a second database.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-engine --lib transfer::tests::an_export_round_trips_into_an_empty_home --locked -- --test-threads=1` — 1 passed.
- Temporary source and target directories were removed after the run; no cargo
  or rustc process remained.

Rollback failure-injection and concurrent delete/recall remain unverified.

## G3-05-07 implemented slice: provenance rollback regression

The transfer failure-injection suite now seeds distinct source and target
sentinels in both provenance tables, forces an import into a target whose
layout marker is a symlink, and verifies that rollback restores the target
rows without installing the source rows. The test exercises the existing
SQLite snapshot plus staged-install/undo path; it does not alter transfer
registry policy or transcript-file transport.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-engine --lib transfer::tests::marker_failure_restores_provenance_tables --locked -- --exact` — 1 passed.
- Temporary source and target directories were removed after the run; no cargo
  or rustc process remained.

The `state/compact` transcript-file policy and concurrent delete/recall remain
explicitly outside this slice.

## G3-05-09 implemented slice: committed-delete recall and browse convergence

The store regression now keeps a reader active across the delete boundary. It
first proves that a provenance-linked assistant carrier is recallable, then
releases the reader only after the fact tombstone transaction commits. The
reader performs both keyword recall and direct evicted browse afterward. The
linked carrier must disappear from both paths while an unlinked user carrier
with the same text remains visible. This keeps the assertion on the typed
`(fact, id)` edge rather than on text matching.

Evidence is recorded in
[`2026-10-02-g3-05-concurrent-delete-recall-evidence.md`](2026-10-02-g3-05-concurrent-delete-recall-evidence.md):

- `cargo test -p yunxi-core --lib memory::tests::store::concurrent_recall_and_browse_honor_a_committed_memory_tombstone --locked -- --exact --test-threads=1` — 1 passed, 706 filtered.
- The disposable WSL Ubuntu-24.04 ext4 source and target were removed after
  the run; no cargo or rustc process remained.

This is still a committed-delete/convergence contract. It does not introduce
or claim a cross-database transaction, an overlap-after-commit guarantee for a
read that started before deletion, or crash consistency across the memory and
state SQLite files.

## G3-05-10 implemented slice: historical transcript scope barrier

The transcript guard now treats the whole `state/compact` tree as a protected
namespace instead of checking only the active session directory. The compact
root itself, sibling-session paths, and paths that resolve through `..` are
fail-closed. The active session still goes through the typed provenance and
tombstone check; a path belonging to another session is rejected even when its
provenance is live. `run_command` uses the same scope check for shell chains,
so an absolute historical path cannot be smuggled through `cat ...; ...`.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-engine --lib transcript_guard --locked -- --test-threads=1` — 11 passed, 694 filtered.
- The new regressions cover tombstoned transcript reads through a shell chain
  and a live-provenance historical-session read; literal glob paths and
  `cd`-then-relative reads are also rejected before shell dispatch.
- The disposable source and target were removed after the run; no cargo or
  rustc process remained.

This slice does not claim to parse arbitrary shell grammar or defend against
symlink/hardlink races; dynamic variables, command substitution, redirection,
and external filesystem links remain explicit next-boundary work.

## G3-05-11 implemented slice: execution-entry binding parity

The transcript barrier is now attached at the narrow execution boundaries
that construct a fresh registry after static composition. Daemon IPC
`ToolCall`, direct CLI `tool`/`tool-call`, and direct MCP `call_tool` all pin
`StateStore` and `MemoryStore` to the requested session before dispatch. The
public binding facade does not alter tool definitions or provider-facing
catalog bytes. Catalog-only paths (`ToolCatalog` and MCP `tools/list`) remain
unbound intentionally because they expose definitions and do not execute a
filesystem-reading tool.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-hosts --lib web::tests::ipc_bridge --locked -- --test-threads=1` — 22 passed.
- `cargo check -p yunxi --locked` — passed.
- `cargo test -p yunxi-engine --lib subagent::tests::foreground_dev_registry_binds_transcript_guard_to_ambient_session --locked -- --exact --test-threads=1` — 1 passed, 702 filtered.
- `cargo fmt --all -- --check`, `git diff --check`, and the privacy scan passed.

The daemon-less development subagent creates a separate dev registry inside
its foreground fallback loop and is now bound to the ambient parent session.
Missing or unknown ambient sessions fail closed; no process-current session is
silently substituted. The session-hosted subagent path continues to receive
its session-owned registry from the host.

## G3-05-12 boundary audit: shell indirection and filesystem links

The remaining `run_command` barrier is intentionally still lexical. Literal
absolute/relative/`~/` paths, literal `..`, and literal wildcard paths are
normalized and checked against the compact namespace before dispatch. The
guard does not claim to interpret shell variables, command substitution,
redirection, `cd` plus a later relative path, embedded scripts, or filesystem
symlink/hardlink identity races. Covering those cases requires an argv/shell
mediation layer or a carrier inode/realpath registry; this contract does not
silently introduce either one.

## G3-05-13 implemented slice: committed-delete restart barrier

The memory-store regression now closes and reopens the data/state SQLite
handles after a fact delete has committed. The reopened store must hide the
typed linked evicted carrier from keyword recall, direct browse, and the
semantic corpus while retaining an unrelated carrier with identical text.
The durable tombstone is asserted after reopen as well. This is a restart
barrier for a committed delete; it is not a process-crash injection and does
not claim cross-database atomicity or an overlap-after-commit guarantee.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-core --lib memory::tests::store::committed_delete_survives_memory_store_restart_for_evicted_carriers --locked -- --exact --test-threads=1` — 1 passed, 707 filtered.
- The disposable source and target were removed after the run; a follow-up
  process check found no cargo or rustc process.

## G3-05-14 implemented slice: reset id-reuse barrier

`reset_all` still clears the data tables before it clears the separate
evicted-context database. The data transaction therefore keeps the
`facts`/`episodes` AUTOINCREMENT high-water marks instead of resetting them.
This prevents a replacement row from reusing a tombstoned id if the process
dies before the state cleanup. The other resettable bookkeeping sequences are
unchanged. The regression keeps the deleted fact's tombstone and requires the
replacement fact to receive a larger id.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-core --lib memory::tests::store:: --locked -- --test-threads=1` — 15 passed, 694 filtered.
- `cargo fmt --all -- --check`, `git diff --check`, and the privacy scan passed.
- The disposable source and target were removed after the run.

## G3-05-15 implemented slice: import-side evicted carrier barrier

Force-import already carried the live data-memory tombstones into staged
`memory.db` files, but an archive could still bring back an older
`state/personas/<scope>/memory/evicted_context.db`. This slice extends the
same protection to the state-side archive before the staged tree is installed.

The import fixup pairs `data/personas/<scope>/memory/memory.db` with the
matching `state/personas/<scope>/memory/evicted_context.db` (and keeps the
legacy `personas/<scope>/memory/memory.db` layout as a fallback). It reads only
typed tombstone ids from the live memory database, selects linked
`evicted_turn` carrier ids through `memory_provenance`, then deletes that
carrier's provenance, embedding, and turn in one state-database transaction.
Missing/legacy tables are left untouched, and scopes are never mixed.

Evidence (WSL Ubuntu-24.04 ext4 disposable checkouts, one cargo job at a time):

- `cargo test -p yunxi-engine --lib transfer::fixups::tests::evicted_tombstones_remove_the_whole_linked_carrier --locked -- --exact --test-threads=1` — 1 passed.
- `cargo test -p yunxi-engine --lib transfer::tests::force_import_filters_evicted_carriers_by_persona_scope --locked -- --exact --test-threads=1` — 1 passed.

The second regression creates a source archive with linked and unlinked
evicted carriers, imports it over a target that already owns a fact tombstone,
and verifies that only the typed linked carrier is removed. This proves the
real export→force-import wiring, not just the SQL helper in isolation.

## G3-05-08 implemented slice: concurrent browse convergence contract

The memory browse regression now runs a delete and an evicted-context browse
from cloned `MemoryStore` values behind a two-party barrier. It asserts that
the delete commits a tombstone, the concurrent call completes without a
database error, and every read after both operations hides the linked carrier
while the tombstone lookup remains true.

This is intentionally a convergence contract, not a claim of a cross-database
transaction: a read that linearizes before the data-database delete commits
may still observe the carrier. Proving a stronger overlap-after-commit rule
would require a coordinated read hook or a cross-database synchronization
policy, neither of which is introduced here.

Evidence (WSL Ubuntu-24.04, ext4 disposable copy, one cargo job):

- `cargo test -p yunxi-core --lib memory::tests::browse::concurrent_delete_and_browse_converge_on_tombstone_barrier --locked -- --exact` — 1 passed.
- Temporary source and target directories were removed after the run; no cargo
  or rustc process remained.

## G3-05-03a implemented slice: direct evicted browse barrier

The existing keyword and semantic evicted-context paths already consulted the
typed tombstone set, but the dashboard-style no-keyword browse and direct
`browse_evicted_item` lookup queried `evicted_turns` without that barrier. This
slice closes only that read gap: both the count/page query and the direct item
lookup exclude carriers by exact `evicted_turns.id` when their typed
`fact`/`episode` reference has a durable tombstone. The state/data databases
remain separate and the snapshot is still best-effort across that boundary;
there is no text matching or cross-database transaction.

Evidence for G3-05-03a (WSL Ubuntu ext4, one cargo job at a time):

- `yunxi-core` `memory::tests::browse`: 3 passed, including the linked versus
  unlinked carrier regression and keyword-path parity.
- `yunxi-core` `memory::tests::store`: 12 passed, including the existing
  keyword/semantic tombstone and migration coverage.

The direct browse path still retains its pre-existing visibility/pagination
semantics; changing those contracts is outside this slice. Summary and
transcript barriers remain separate G3-05 slices.

## G3-05-16 implemented slice: async semantic eviction barrier

The semantic evicted-context path performs embedding work across an `await`.
Before this slice, a carrier could pass the initial tombstone snapshot, wait
for document or query embeddings, and then be written or returned after the
linked memory had been deleted. The new barrier re-reads the typed tombstone
set and confirms that each candidate still exists in `evicted_turns` after
embedding completes. Candidates that became stale are skipped before an
embedding write and before final hit materialization; unrelated live carriers
continue through the same path.

This is an await-boundary protection slice, not a strict cross-database
linearization claim. It deliberately avoids holding a process-wide lock or
claiming atomicity between the data and state SQLite files. The remaining
post-check overlap window is tracked in the next G3-05 施工边界.

Evidence (WSL Ubuntu-24.04, ext4 disposable checkout, one cargo job):

- `cargo test -p yunxi-core --lib memory::tests::store::async_semantic_evicted_recall_rechecks_tombstones_before_write_and_return --locked -- --exact --test-threads=1` — 1 passed.
- `cargo test -p yunxi-core --lib memory::tests::store:: --locked -- --test-threads=1` — 16 passed.
- `cargo fmt --all -- --check`, `git diff --check`, and the privacy scan passed.
- The disposable checkout and target were removed; no cargo or rustc process
  remained after verification.

## G3-05-17 implemented slice: tombstone epoch optimistic read barrier

The memory database now carries an additive, monotonic
`memory_meta.tombstone_epoch` counter. The schema migration is idempotent and
initializes legacy databases to zero. A delete, full reset, session reset, or
expired short-diary cleanup bumps the counter only when that transaction
actually inserts one or more typed tombstones; repeated deletes and empty
cleanups do not create false epochs. The increment is in the same
data-database transaction as the tombstone rows.

State-side evicted browse, direct item lookup, keyword recall, and semantic
recall now use a bounded optimistic read barrier: read `(epoch, tombstones)` in
one SQLite read transaction, materialize the state-side result, then re-read
the epoch before returning it. An epoch change retries once with a fresh
snapshot; if the second attempt still overlaps a delete, the read fails closed
to an empty result. Semantic recall additionally re-materializes the live
corpus on retry, so an unlinked carrier that shares text with a deleted carrier
remains eligible while the typed linked carrier stays hidden. This closes the
snapshot-to-state and embedding-await windows without a process-wide lock or a
cross-database transaction.

Evidence for this slice (WSL Ubuntu-24.04 ext4 disposable checkout, one cargo
job at a time):

- lifecycle epoch/idempotence regressions: 8 passed;
- evicted browse barrier tests: 4 passed;
- memory store tests, including the async semantic retry: 16 passed;
- full `yunxi-core` library run: 699 passed, 8 ignored, 5 pre-existing
  `llm::openai_compatible` endpoint/error-message failures unrelated to these
  memory-only files;
- `cargo fmt --all -- --check`, `git diff --check`, and the privacy scan passed.

This remains an optimistic convergence barrier, not strict linearization: a
read can still overlap the tiny interval after its final epoch check and before
the caller consumes the value. Transfer-side staged tombstone fixups and
crash-injection evidence remain the next G3-05 boundaries.

## G3-05-18 implemented slice: dynamic transcript shell-path barrier

The `run_command` transcript guard now closes the first shell-indirection gap
without becoming a general shell parser. It splits only unquoted compound
segments, recognizes a small set of file-access command positions and
unquoted redirections, and rejects unresolved `$NAME`/`${NAME}`, `$()` and
backtick expansion in those contexts. Literal paths still use the existing
typed provenance check. A substitution in a non-file command such as
`echo "$(printf hi)"` remains allowed, so the new rule does not turn every
shell feature into a privacy denial.

The regression suite proves rejection before the handler for variable-backed
reads, command-substitution paths, `cd` followed by a dynamic relative read,
and dynamic redirection, while proving the non-file echo case still reaches
the handler. The suite also covers path-qualified file commands, shallow
wrapper/shell invocations, append-style assignments, and positional parameters.
This is intentionally a conservative lexical boundary: arbitrary wrapper
chains, embedded scripts, symlink/hardlink identity races, and full shell
data-flow remain explicit residuals and are not claimed solved.

Evidence (WSL Ubuntu-24.04 ext4 disposable checkout, one cargo job):

- `cargo test -p yunxi-engine --lib tools::transcript_guard --locked --
  --test-threads=1` — 15 passed;
- `cargo fmt --all -- --check` and `git diff --check` passed.

The next boundary is to decide whether deeper wrapper/embedded-script mediation
is warranted by observed use, rather than silently growing a second shell parser.

## G3-05-19 implemented slice: nested shell substitution barrier

The transcript `run_command` lexical barrier now inspects bounded `$()` and
backtick bodies in addition to the top-level compound segment. A nested file
access command with unresolved expansion, such as `echo "$(cat \"$p/fold\")"`,
is rejected before the handler; nested non-file substitutions such as
`echo "$(printf hi)"` remain executable. The scan is deliberately capped at
eight substitution levels and remains a lexical helper, not a shell parser.

The regression suite covers nested `cat` in both substitution syntaxes and
the corresponding nested `printf` allow-list. Quoted embedded scripts passed to
`sh -c`/`eval`, arbitrary wrapper data-flow, process/arithmetic substitution,
and filesystem identity races remain explicit residuals.

Evidence (WSL Ubuntu-24.04 ext4 disposable checkout, one cargo job):

- `cargo test -p yunxi-engine --lib tools::transcript_guard --locked --
  --test-threads=1` — 16 passed;
- `cargo fmt --all -- --check` and `git diff --check` passed.

The next boundary is transcript carrier file identity/TOCTOU hardening at the
actual file-open and shell execution seams, not further growth of this lexical
scanner.

## G3-05-20 implemented slice: transcript identity and shell access boundary

The transcript guard now adds a narrow filesystem-identity barrier for
structured transcript reads. A registered live carrier must resolve to a
regular file without a symlink in its compact/session path; on Unix, a leaf
with more than one hard link is rejected as well. This is intentionally scoped
from the active compact root through the session directory, so platform-level
symlinks such as macOS `/var` are not treated as transcript violations.

`run_command` is now fail-closed for a path that resolves to a registered
protected transcript, even when the transcript is live. The generic `sh -lc`
handler cannot carry an opened-file capability across arbitrary shell syntax,
so allowing that path would leave a deterministic replacement window between
the guard and shell open. Ordinary files and non-transcript commands retain
their previous behavior.

The 22-test regression suite covers live structured reads, shell denial for
live transcripts, leaf symlink replacement, compact-root symlink replacement,
an external symlink alias, hard-link replacement, and an ordinary file command
allow-list. It does not claim to close every rename race: the
current registry API still passes only a denial string, and `read`/`grep`/`glob`
handlers still open after the guard. Parent replacement outside the compact
root, an external hard-link alias (which canonicalizes outside the compact
root), quoted embedded scripts, arbitrary wrapper data-flow, and a future
open-file capability remain explicit follow-up boundaries. The alias probe is
also an extra canonicalization on paths outside the compact root, so it is not
presented as a general filesystem policy.

Evidence (WSL Ubuntu-24.04 ext4 disposable checkout, one cargo job):

- `cargo test -p yunxi-engine transcript_guard::tests -- --nocapture` — 22
  passed, 0 failed;
- `cargo fmt --all -- --check` and `git diff --check` passed;
- the Windows host build was not used as Linux evidence because this workspace
  intentionally contains Unix-only modules; the baseline host failure is
  unrelated to this slice.

The next boundary is to choose the remaining search and shell seams: whether
`grep`/`glob` should receive an opened capability, how parent-directory
replacement should be handled (for example with a future `openat`/dirfd
design), and whether quoted `sh -c`/`eval` wrappers need a narrower policy.
No general shell parser or cross-database memory change is implied by this
slice.

## G3-05-21 implemented slice: opened transcript capability for `read`

The registry now carries an opaque, local-only `ToolCallContext` from the
transcript guard to a context-aware handler. For a live, provenance-backed
transcript, the guard repeats the existing scope, tombstone, symlink, regular
file, and hard-link checks, then opens a read-only descriptor. On Unix the
open uses `O_NOFOLLOW` for the leaf. The structured `read` handler consumes a
clone of that descriptor instead of reopening the path, so a leaf rename and
replacement after the guard cannot switch the bytes returned to the model.
The capability never enters the provider-facing tool contract or transcript.

Ordinary files and legacy paths keep the previous path-based behavior. The
slice is intentionally narrow: `grep`/`glob` still use their existing
path/subprocess seams; parent-directory replacement, full `openat`/dirfd
semantics, external hard-link aliases, quoted embedded scripts, arbitrary
wrapper data-flow, and a general shell parser remain explicit residuals.

The regression suite includes both a handler-level path-replacement test and a
default `read` test proving that the opened descriptor returns the original
bytes after the pathname is replaced. The targeted WSL Ubuntu-24.04 ext4
checkout passed `cargo test -p yunxi-engine transcript_guard::tests --locked
-- --nocapture`: 23 passed, 0 failed. The disposable ext4 checkout and target
were removed after the run.

The next boundary is the search-handler decision (`grep`/`glob`), the parent
replacement/openat decision, and the remaining quoted-shell/wrapper and
cross-database overlap audits; G3-05 is not complete.

## G3-05-22 implemented slice: deny live transcript search without a descriptor

The `grep` and `glob` handlers remain subprocess/path based and do not yet
accept `ToolCallContext`. Rather than leave a live transcript's narrow
rename/replacement window open, the transcript guard now denies those two
structured searches when their exact path has live provenance. Unknown,
tombstoned, out-of-scope, and identity-invalid transcript paths retain their
existing fail-closed errors; ordinary non-transcript searches remain allowed.
The error explicitly directs the caller to `read`, which is the only
structured transcript path currently backed by an opened descriptor.

The regression suite covers both search tool names against a live registered
transcript. This is a conservative interim boundary, not a claim that search
is permanently unsupported: a future descriptor-aware search implementation
may replace the denial after its own identity and subprocess-inheritance
tests. Parent-directory replacement, `openat`/dirfd semantics, quoted shell
wrappers, and cross-database overlap remain outside this slice.

## G3-05-23 implemented slice: staged evicted fixup idempotence

The transfer-side regression now reruns `apply_evicted_tombstones` against the
same staged evicted-context database after the first successful cleanup. The
second pass removes zero rows, proving that deleting the provenance row,
embedding, and carrier is an idempotent operation rather than a one-shot
assumption. This is evidence for repeated import/fixup retries, not a claim of
cross-database atomicity or crash recovery. SQLite transaction rollback and the
existing staged-install rollback tests remain the authoritative failure path;
crash injection between the live tombstone read and staged commit is still a
follow-up boundary.

## G3-05-24 implemented slice: descriptor-backed single-file transcript grep

The live transcript `grep` path now receives the same opened read capability
as `read`. When the requested path exactly matches a live provenance-backed
transcript, the handler feeds a cloned descriptor to `rg` through stdin and
rewrites the stdin label back to the requested path in the result. A leaf
rename or replacement after the guard therefore cannot redirect the search to
new bytes. Ordinary files retain the existing path-based `rg` behavior.

`include` is deliberately rejected for this protected path until its glob
semantics can be reproduced without relying on a pathname. `glob` remains
denied for live transcripts because it needs a directory capability rather
than a single-file descriptor. This keeps the interim search surface explicit
instead of silently weakening filename-filter behavior.

Evidence (WSL Ubuntu-24.04 ext4 disposable checkout, one cargo job):

- `grep_uses_opened_transcript_capability_after_path_replacement` — 1 passed;
- `grep_rejects_include_for_opened_transcript_capability` — covered by the same
  default-tools test target;
- `live_registered_transcript_search_allows_grep_but_denies_glob` — 1 passed;
- `transcript_guard::tests` — 24 passed, 0 failed;
- `cargo fmt --all -- --check`, `git diff --check`, and the privacy scan passed.

The disposable ext4 source and target were removed after verification. Parent
directory replacement, directory `glob`, quoted embedded scripts, arbitrary
wrapper data-flow, and cross-database overlap remain explicit boundaries.

## G3-05-25 overlap contract: standard read linearization

The remaining concurrent-delete question is fixed to the standard, bounded
linearization contract for this stage. A read takes a tombstone snapshot,
materialises its result, and treats the final `tombstone_epoch` equality check
as its read linearization point. If the epoch changed before that point, the
read retries within its fixed budget; if the budget is exhausted it returns an
empty, fail-closed result. A read that has already passed the final check may
still be consumed by its caller while a later delete commits. That caller-side
window is outside this contract and is not described as strict post-commit
exclusion.

This deliberately does not introduce a cross-database global lock or claim
atomicity between the persona memory database and the state/evicted database.
Async semantic recall must re-check live ids around provider awaits and before
returning; state-side carrier cleanup must remain id-based and idempotent. Any
future strict post-commit guarantee would require a separate coordinated
read/write protocol and a new performance/crash-recovery review rather than a
silent strengthening of this slice.

The acceptance evidence for this contract must include deterministic overlap
seams for keyword browse, semantic/hybrid fallback, and state-side evicted
deletion, plus the existing restart and async-provider tests. The tests prove
convergence and the chosen linearization point; they must not assert a stronger
post-commit guarantee than the contract states.

## G3-05-26 implemented slice: quoted shell-wrapper boundary

The transcript `run_command` lexical barrier now treats a shell wrapper's
quoted command as a separate command string. `sh`/`bash`/`dash`/`ksh`/`zsh`
recognise `-c`, combined short options such as `-lc`, and `--command`; the
wrapper payload is recursively scanned for dynamic paths and file-access
commands. `eval` and the other shallow command wrappers receive the same
conservative payload treatment. Dynamic or opaque payloads fail closed, while
static payloads such as `sh -c 'printf hi'` remain allowed. The implementation
is still a bounded lexical screen, not a shell interpreter.

WSL Ubuntu-24.04 ext4 `transcript_guard::tests` passed 25/25, including quoted
`sh -c`, `bash -lc`, `--command=`, `eval`, positional/variable payloads, nested
command substitution, and safe static wrapper payloads. The remaining explicit
non-goals are arbitrary function/alias/dynamic-command data flow, `source`/`.`
and embedded Python/Node/Perl scripts, filesystem identity/openat semantics,
and cross-database overlap linearization.

## G3-05-27 implemented slice: deterministic overlap test seam

Memory overlap tests now use a per-`MemoryStore`, `cfg(test)`-only callback
instead of sleeps, process-wide locks, or scheduler-dependent barriers. The
seam can pause immediately before the final epoch check and immediately before
an evicted state delete commits; production builds do not carry the callback or
its synchronization machinery. The new tests deterministically cover the
hybrid keyword fallback retry after a memory tombstone commit and the state-side
delete overlap followed by a clean subsequent browse. The latter intentionally
does not assert strict post-commit exclusion for the already-raced result, in
accordance with G3-05-25; it asserts idempotent deletion and convergence on the
next read.

WSL Ubuntu-24.04 ext4 `memory::tests::browse` passed 6/6 with one test thread.
The existing async-provider semantic regression remains the evidence for the
embedding-await boundary. No runtime global lock, database schema change,
fixed sleep, or cross-database atomicity claim was added.

## Current evidence and boundaries

## G3-05-28 implemented slice: semantic final-epoch overlap

The deterministic per-store seam now covers the semantic path's final
`tombstone_epoch` check as well as keyword fallback and state-side browse. The
regression pre-seeds the state-side vectors so the provider request is not the
timing mechanism: it pauses after semantic hits have been materialised and
before the final epoch comparison, commits a linked fact tombstone, then
releases the reader. The stale first pass is rejected and its bounded retry
returns only the still-live unlinked carrier.

The existing provider-await regression remains separate evidence for the
earlier async boundary. Together the two tests cover both provider completion
and final semantic read linearization without adding a production lock,
schema, sleep, or cross-database atomicity claim. The test intentionally uses a
per-store `cfg(test)` hook and does not assert strict post-commit exclusion for
a result already past the final check.

WSL Ubuntu-24.04 ext4 focused semantic tests passed 2/2, and the complete
`memory::tests::store` set passed 17/17. The disposable source and target
directories were removed after the run.

## G3-05-29 implemented slice: evicted detail overlap

The same read-linearization contract now has a dedicated detail endpoint
regression. A linked evicted carrier is read through `browse_evicted_item`; the
per-store test seam pauses immediately before its final `tombstone_epoch`
comparison, the owning fact is deleted and committed, and the reader resumes.
The raced snapshot is discarded by the bounded retry, and both that result and
a subsequent detail read return no item. The test uses a plain thread because
the detail API is synchronous; it adds no runtime lock, sleep, schema change,
or stronger post-commit guarantee.

WSL Ubuntu-24.04 ext4 `memory::tests::browse` passed 7/7, including the new
detail overlap regression.

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
