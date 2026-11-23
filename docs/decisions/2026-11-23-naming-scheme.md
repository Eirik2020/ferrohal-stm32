# ADR-017: Naming scheme (2026-11-23)

ADR-017 - Status: promoted

Promoted to: [naming.md](../design/naming.md) (design).

Supersedes [ADR-003](archive/2026/2026-11-08-naming-scheme.md).

## Context

ADR-003 set the naming scheme as a list of renames, all since made. ADR-012 replaced
its module rule. A review of the public API then found what it had not settled: four
patterns for a refusal's alias, three words for one quantity, module prefixes on some
items and not others, "handle" used loosely, and run letters that meant a different
test on each bench. This record states the whole scheme as it holds.

## Decision

1. **Catalogs**: one per IP or subsystem, under `src/requirements/`, holding its part
   families.
2. **IDs**: `<FAMILY>-NNN`, one family per prefix. A per-part family takes a hyphen and
   the part (`USART-V4-001`).
3. **Features**: `<ip>_v<metapac version>` where metapac versions the IP,
   `<ip>_<family>` where FerroHAL needs a family shape. metapac's DMA numbers are
   metapac's, not an order.
4. **Modules**: as ADR-012 lays them out.
5. **Vocabulary**: *channel* (timer compare), *lane* (DMA-fed output), *stream* (DMA),
   *line* (UART). A *handle* is an ownerless view of a running driver that another
   context holds and copies freely, because everything it does is a single read or a
   write no one else races.
6. **Methods**: constructors are `new`; `Watchdog::start` is the one exception, because
   an independent watchdog cannot be stopped once started. `set_*` changes in place,
   `into_*` consumes. Units ride in the name (`*_us`, `*_hz`).
7. **One quantity, one word**: *frequency* is a clock's, *rate* is events a second,
   *baudrate* stays the UART's, *speed* is GPIO slew only.
8. **Refusals**: the alias is `<What>Refused`, named after what refuses, not after the
   method.
9. **No module prefix** on an item (`spi::Parts`), except a module's main driver types,
   the refusals named after them, and routing traits a glue file imports beside another
   module's trait of the same name.
10. **Benches**: `run-<bench>.sh`. A bench binary is named by its test, the same name on
    every bench that runs it. Run letters are each bench's aliases, upper case in prose
    and lower case on the command line.
11. **Documents**: capitals for registries (`DEBT`, `README`, `WIRING`), kebab-case
    for everything else.
12. **Checker**: `tools/check_names.py` holds the catalogs, IDs and feature names.

## Rejected

- `<Type><Method>Refused`: the name would change whenever a method is renamed.
- Dropping the word "handle" rather than defining it.
- `Watchdog::new`: the name would hide that a started watchdog cannot be stopped.
- One letter registry for every bench, with gaps where a bench lacks the test.

## Consequences

- `docs/design/naming.md` states each rule with its examples.
- ADR-003 is archived as superseded. No code changes.
