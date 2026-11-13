# Housekeeping

Documents rot by addition. An agent adds a note because it helped it once, and nobody
checks whether it duplicates another, belongs elsewhere, or contradicts a policy. This is
the periodic pass that forces the review. It is **due on the date in [`state.md`](state.md)**
- the documentation check fails after that date - and also when more than ten documents
have been added since the last review. A PR cannot ignore either.

## The pass

After any edit, regenerate the index: `python3 -I tools/docs_index.py` (`--check` fails
when stale). Then run `python3 -I tools/housekeeping.py` and work through its report:

1. **Orphans** - a document nothing links to is either unreachable or in the wrong home.
   Link it from the hub that owns it, merge it, or delete it (git keeps history).
2. **Duplicate hints** - overlapping titles are a hint; read both, merge them, or make one
   point at the other.
3. **Decisions** - promote what binds (policy, `docs/design/<topic>.md`, or requirement),
   supersede what changed, archive what is settled. Keep the active set under 20.
4. **Plans and notes** - a plan whose work merged becomes a `RESOLVED.md` line and is
   deleted once its facts live in their owning documents (owner, 2026-11-13); a note
   whose facts moved into a module doc is deleted; anything still useful is re-linked.
5. **Budgets and orphans** - fix every checker failure by **moving prose** (split the
   document, move a chapter to a topic document), never by compressing it; link
   the split-off part from where it came, so it is never an orphan.
6. **Workspace pass** - `agent-memory/MEMORY.md`, stale state notes, and
   `python3 -I agent-memory/tools/check_agent_docs.py`.

Record the pass in [`state.md`](state.md): the dates, the document count, and one line
saying what changed. That commit is the evidence; a pass that changes nothing says so.

## What this cannot enforce

Whether the pass was *thoughtful*. It forces traversal - the report lists every document,
orphan, duplicate hint and decision - but judgement stays with the agent and the owner.
