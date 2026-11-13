# FerroHAL's Goals

Every requirement derives from one of these, or from another requirement. They are
FerroHAL's own: it knows nothing of what it is used for.

1. **G1 - Scheduler and time agnostic.** FerroHAL works from RTIC, async, bare metal
   or an equivalent. It owns no interrupt handler, vector, priority or time source,
   and the caller wires it to whatever scheduler it uses. Its core uses no time
   crate; optional adapter features may connect it to one (fugit, rtic-time,
   embassy-time and the like), and the core never calls an adapter.
2. **G2 - No panic by default.** A hardware fault or a bad request comes back as an
   error. With default features no public function can reach a panic, and no wait
   is unbounded in a stop path. A feature may bring in code that can panic only when
   it is off by default and listed as an exception in the
   [features policy](../docs/policies/features.md): the default build is panic-free,
   and the ecosystem stays reachable for a user who opts in.
3. **G3 - A single unified HAL.** One crate, with one API across every chip it
   supports. A chip's differences stay in its glue and its IP versions.
4. **G4 - Requirement tracing and verification.** Each file states the requirements
   it fulfils and is hashed, and evidence records the hash. One first pass verifies
   everything; after it, only what changed is retested.
5. **G5 - Safe by default.** The public API exposes no `unsafe fn` and no reachable
   raw register access: a HAL is the safe abstraction. An exception needs a written
   rationale, the owner's dated approval, and a default-off feature gate
   ([safety policy](../docs/policies/safety.md)).
