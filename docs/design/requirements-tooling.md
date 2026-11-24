# Requirement format and reqtool

Source: ADR-006 (archived: [`../decisions/archive/2026/2026-11-10-doc-map-dissolution.md`](../decisions/archive/2026/2026-11-10-doc-map-dissolution.md)).


The model
is [requirements-system.md](requirements-system.md).

## Format and tooling

- A YAML schema is enforced, so broken requirement text fails.
- Rustdoc presentation: the full requirement text goes on a docs-only page, a `#[cfg(doc)] pub mod requirements` containing one page per IP version, linked from the module. Module docs show only the ID list, so editor hovers stay small. `#[doc(alias = "<ID>")]` on implementing items is optional.
- Requirements live in one catalog per IP or subsystem, `src/requirements/<ip>.yaml` (`uart.yaml` holds UART, UARTDMA, USART-V2 and USART-V4); `tools/check_names.py` holds each family to exactly one catalog. A file's doc comment names the IDs it fulfils.
- Validation is reqtool's (`reqtool validate`): the schema, with unknown fields refused, and exactly one "shall" per requirement. `tools/check_names.py` resolves every ID to exactly one catalog entry.
- A requirement takes its worst entry. Where a requirement of a neutral driver has one procedure run on several boards, its entries for that procedure name one `part`, after the procedure, so a run on any one of those boards carries the requirement (the car rule; reqtool's README states the rule). A requirement of one IP version, of chip glue, of a family's clock module or of a chip's ROM names no part, and neither does an entry whose procedure exists on one board: each stands alone.
- The tool is `reqtool`, its own crate shared by FerroWasp, FerroForge and FerroHAL. It evidences a file and its pinned dependencies, without local imports or the compiler.
- Every library file owes tracing: `every_file_traced`, with `hil/` left out by `every_file_traced_except` (`reqtool.toml`).
- `reqtool` speaks only generic terms, drawn from ordinary requirements practice, and knows nothing of any project. FerroWasp turns on more of it than FerroHAL does.
- The hazard decides who runs a test: an agent may run and record what has none, and a person runs what is dangerous or could damage expensive equipment, with its preconditions stated.

## Not met yet *(measured 2026-11-03)*

- A bench run crosses every layer, since a UART loopback goes through IP, engine, glue and API at once. Each UART check now names the requirements it verifies, but a run still refreshes every requirement naming its procedure: refreshing one layer alone means splitting procedures, not yet done.
