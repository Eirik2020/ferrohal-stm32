# FerroHAL's Goals

Every requirement derives from one of these, or from another requirement. They are
FerroHAL's own: it knows nothing of what it is used for.

1. **G1 - Scheduler agnostic.** FerroHAL works from RTIC, async, bare metal or an
   equivalent. It owns no interrupt handler, vector, priority or time source, and
   the caller wires it to whatever scheduler it uses.
2. **G2 - No panic.** A hardware fault or a bad request comes back as an error. No
   public function can reach a panic, and no wait is unbounded in a stop path.
3. **G3 - A single unified HAL.** One crate, with one API across every chip it
   supports. A chip's differences stay in its glue and its IP versions.
4. **G4 - Requirement tracing and verification.** Each file states the requirements
   it fulfils and is hashed, and evidence records the hash. One first pass verifies
   everything; after it, only what changed is retested.
