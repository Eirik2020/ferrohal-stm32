# Requirement format and reqtool

Source: ADR-006, [`../decisions/2026-11-10-doc-map-dissolution.md`](../decisions/2026-11-10-doc-map-dissolution.md).


The architecture map's section 9's tooling half, dissolved 2026-11-10. The model
is [requirements-system.md](requirements-system.md).

## Format and tooling

- **[Decided]** A YAML schema is enforced, so broken requirement text fails.
- **[Decided]** Rustdoc presentation: the full requirement text goes on a docs-only page, a `#[cfg(doc)] pub mod requirements` containing one page per IP version, linked from the module. Module docs show only the ID list, so editor hovers stay small. `#[doc(alias = "<ID>")]` on implementing items is optional.
- **[Proposal]** Requirements live in a `requirements.yaml` beside the layer that sets them (`src/uart/requirements.yaml`), so they move with it. A file's doc comment names the IDs it fulfils.
- **[Proposal]** Validation has three layers: the schema (structure), a text lint (vague words, exactly one "shall" per requirement) and a resolver (IDs match real items; every public item in a verified file is covered). These run in a separate publish tool, never in a build script.
- **[Proposal]** The publish tool generates `requirements/index.json` (file hashes and a feature → file → requirement map) and a `requirements.md` per module. CI checks they are in sync with the sources.
- **[Decided]** The tool is `reqtool`, its own crate shared by FerroWasp, FerroForge and FerroHAL (2026-11-03). It evidences a file and its pinned dependencies, without local imports or the compiler.
- **[Decided]** `reqtool` speaks only generic terms, drawn from ordinary requirements practice, and knows nothing of any project. FerroWasp turns on more of it than FerroHAL does (2026-11-03).
- **[Decided]** The hazard decides who runs a test: an agent may run and record what has none, and a person runs what is dangerous or could damage expensive equipment, with its preconditions stated (2026-11-03).

## Not met yet *(measured 2026-11-03)*

- A bench run crosses every layer, since a UART loopback goes through IP, engine, glue and API at once. Each UART check now names the requirements it verifies, but a run still refreshes every requirement naming its procedure: refreshing one layer alone means splitting procedures, not yet done.
