# ADR-021: Naming scheme (2026-11-24)

ADR-021 - Status: promoted

Promoted to: [naming.md](../design/naming.md) (design).

Supersedes [ADR-017](archive/2026/2026-11-23-naming-scheme.md).

## Context

ADR-017's ID rule was read as allowing a driver kind run into the family (`UARTDMA`),
and its word list followed no stated source. This record states the whole scheme
again with both changed.

## Decision

1. **Catalogs**: one per IP or subsystem, under `src/requirements/`, holding its
   family and sub-categories.
2. **IDs**: `<FAMILY>-NNN`, one family per prefix; the family is the driver or
   subsystem. A sub-category takes a hyphen: driver kind (`UART-DMA-001`), IP version
   (`USART-V4-001`), chip family (`RESET-H7-001`), CPU core (`WAIT-M4-001`). Never
   run together.
3. **Features**: `<ip>_v<metapac version>` where metapac versions the IP,
   `<ip>_<family>` where FerroHAL needs a family shape.
4. **Modules**: as ADR-012 lays them out.
5. **Vocabulary**: a hardware thing takes the name the first source gives it, in the
   order metapac, ST's reference manual, embassy, FerroHAL. Where a source names it
   differently between chips (F4, F7, H7, G4) or has no name, the next decides: one
   name on every chip. Code yields; "the code says so" is no reason. Kept against
   the order: "IP" and "IP version" (metapac: "peripheral version"), since renaming is
   a bigger change than it is worth. FerroHAL's own terms are the owner's to decide: a
   *lane* is a DMA-fed timer output; a *handle* is an ownerless view of a running
   driver that another context copies freely, since all it does is one register read,
   or one register write complete in itself.
6. **Methods**: constructors are `new`; `Watchdog::start` is the one exception.
   `set_*` changes in place, `into_*` consumes. Units ride in the name (`*_us`, `*_hz`).
7. **One quantity, one word**: *frequency* is a clock's, *rate* is events a second,
   *baudrate* stays the UART's, *speed* is GPIO slew only.
8. **Refusals**: `<What>Refused`, named after what refuses, not the method.
9. **No module prefix** on an item (`spi::Parts`), except a module's main driver types,
   their refusals, and routing traits a glue file imports beside another module's
   trait of the same name.
10. **Benches**: `run-<bench>.sh`. A bench binary is named by its test, the same name
    on every bench that runs it. Run letters are each bench's aliases.
11. **Documents**: capitals for registries (`DEBT`, `README`), kebab-case otherwise.
12. **Checker**: `tools/check_names.py` holds the catalogs, IDs and feature names.

## Rejected

- ST first among the sources: its manuals differ from chip to chip.
- `<Type><Method>Refused`: the name would change whenever a method is renamed.
- Dropping the word "handle" rather than defining it.
- `Watchdog::new`: the name would hide that a started independent watchdog cannot be
  stopped.

## Consequences

- ADR-017 is archived as superseded.
- Run-together prefixes are renamed.
