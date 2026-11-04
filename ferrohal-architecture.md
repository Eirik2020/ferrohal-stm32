# FerroHAL: architecture brief

Architecture only. Implementation details are decided when building and are not covered here.
Status tags:

- **[Decided]**: settled by the owner. Follow it.
- **[Proposal]**: current direction, not yet confirmed. Raise it before building on it.
- **[Open]**: undecided. Ask before choosing.

Facts marked *(measured)* were read from source at the versions given. Everything else is design.

---

## 1. What FerroHAL is

- **[Decided]** FerroHAL is a general-purpose STM32 HAL usable from RTIC, async, bare metal or an equivalent. It is RTIC-first, but RTIC is never required.
- **[Decided]** Crate name `ferrohal-stm32`, in its own repository, independent of FerroForge and FerroWasp.
- **[Decided]** It is built only on `stm32-metapac`. Scope is limited to what metapac covers; anything metapac does not support is out of scope.
- **[Decided]** `embassy-stm32` is a reference to port from, never a dependency. Keep MIT/Apache attribution for ported code.
- **[Decided]** The "toaster rule": FerroHAL does not know whether it runs in a toaster or an aircraft. It contains no safety-tier, certification-class or FerroWasp-specific logic.
- **[Decided]** FerroHAL's main goals are: scheduler agnostic; no panic; a single unified HAL; requirement tracing and verification. These are the roots its requirements trace to. It does not trace to FerroWasp's W1-W7.

## 2. Crate and module structure

- **[Decided]** Hierarchy, bottom up: **IP modules → glue → chips → families → vendors**. Each level is a feature-flag bundle of the level below. Only one vendor exists for now.
- **[Decided]** One module per metapac IP version (`usart_v2`, `dma_v2`, `gpio_v2`, …), each behind its own feature. Chip features are thin bundles of IP-version features, and family features bundle chips.
- **[Decided]** IP modules are chip-neutral: they speak registers only. Chip facts (DMA request maps, pin alternate functions, clock trees, per-chip constants) live in the glue layer, generated from metapac metadata.
- **[Decided]** FerroHAL uses metapac's own IP-version grouping as the definition of "shared IP". If metapac groups chips wrongly, accept it for now and fix it upstream rather than keeping a local exception table.
- **[Decided]** Start as one crate. An IP module moves into its own crate (`ferrohal-<ip>`) once another HAL reuses it.
- **[Decided]** Chip feature names are metapac's exact chip names, passed straight through. FerroHAL owns metapac's `rt` feature behind one FerroHAL feature.
- **[Decided]** Pin stm32-metapac exactly. A metapac upgrade is a deliberate, reviewed change.

## 3. Execution model: scheduler-agnostic

**[Decided]** FerroHAL drives peripherals; the scheduler owns execution.

- It never defines interrupt handlers, never sets NVIC priorities and never enables vectors in the NVIC. It may set a peripheral's own interrupt-enable bits.
- Drivers expose plain service methods (read and clear flags, `on_interrupt`-style handlers) that the scheduler's handlers call.
- There is no async in the core and no time source owned by FerroHAL. Bounded waits are counted, or the caller supplies the bound.
- RTIC-first means owned driver values that are moved into `#[local]` or `#[shared]` once. Runtime singletons that need their own locks are not used.

## 4. Ownership and shared registers

- **[Decided]** Ownership follows the svd2rust model: one `Peripherals::take()`, owned zero-sized tokens, and constrain or split to divide them further. metapac has no ownership model, so FerroHAL generates an svd2rust-style `Peripherals` per chip from metapac metadata. Tokens keep an `unsafe steal()`.
- **[Decided]** Design rule: runtime operations only write registers the driver owns.
- **[Decided]** Writes to shared registers (RCC enable and reset bits, GPIO mode, EXTI, SYSCFG, DMAMUX) go through the single owner of that shared block, for example `&mut Rcc` as in stm32f4xx-hal. This keeps them race-free on every core without bit-banding or critical sections.
  - An RTIC or bare-metal firmware does these writes in init.
  - A layer that constructs drivers at runtime (for example an async layer) wraps the owner in its own lock.
  - Bit-banding exists only on M4 parts (F4, G4) and is at most an optimisation. M7 and M33 parts (F7, H7, H5) have none.
- **[Decided]** There are no critical sections inside drivers.
- **[Decided]** Raw register escape hatch: an `unsafe fn` returns the raw metapac register accessor for a peripheral. Past that point the caller owns every invariant and there are no guarantees. metapac must not be reachable through the normal safe public API.
- **[Decided]** Instance typing: an `Instance` trait checks routing at construction, after which the instance type is erased. This is the pattern used by embassy-stm32 (a static `Info` reference) and imxrt-hal (`AnyInstance`). One driver type then serves every instance (USART1 to USART6), so one FerroForge task can name a single concrete type.

## 5. Public API surface

- **[Decided]** Implement `embedded-hal` 1.0 (and `embedded-io` where it fits) for everything those traits can express.
- **[Decided]** DMA and other capabilities that embedded-hal cannot express use per-IP, family-wide APIs, configured per board. Each IP version's API exposes its engine as it really is. `dma_v2` has FIFO and direct-mode errors, NDTR and double buffering; `bdma_v1` has circular mode and no double buffering *(measured)*. Making traits chip-neutral is a job for the adapter layer.
- **[Decided]** The transport mechanism (how a buffer swap happens on each IP) is internal to that IP's driver. Any mechanism is acceptable as long as it is robust:
  - every DMA and peripheral error flag can be read and cleared by the caller;
  - there is a bounded stop that reports failure, with no unbounded spin;
  - there is no panic on a hardware error.
- **[Decided]** Fault policy: the ultimate goal is a no-panic HAL in which every fault flag is handled. The prototype balances this against development speed.
- **[Decided]** Type protection matches stm32f4xx-hal: pin mode typestates, per-chip pin alternate-function traits, and DMA stream, channel and peripheral pairings as trait impls. Wrong routing is a compile error. For the prototype these impls are hand-written, in the same trait shapes a generator would emit later.
- **[Decided]** No resolver ever chooses DMA streams, channels, pins or timers. The developer states them explicitly and the types check them.
- **[Decided]** Device module for RTIC: FerroHAL exports an svd2rust-shaped device module (the `Interrupt` enum, `NVIC_PRIO_BITS`, `Peripherals::steal()` and the vector table). RTIC's `device = …` and `peripherals = true` then work unchanged.

## 6. Clocks

- **[Decided]** The core is an explicit typed configuration per clock version, ported from embassy, and it is the only path that touches RCC. The user supplies the dividers; FerroHAL checks the limits and returns one frozen frequency record, which drivers must take (like stm32f4xx-hal's `Clocks`).
- **[Decided]** An optional solver may be added later. It would only output an explicit configuration and never configure hardware itself, and should ideally be computed at compile time so its result can be reviewed.
- **[Decided]** The prototype builds the explicit path only.
- Known trap *(measured)*: embassy-stm32's `rcc/f247.rs` writes `VOS Scale1` on every F4 with a PLL, which is reserved on the F401. Review each ported clock version against its reference manual.

## 7. Crate layering above FerroHAL

**[Decided]**

```text
async layer (left to others; assumed to follow embassy-stm32's implementation)
ferroforge-stm32   RTIC/FerroForge drivers and task groups, built the way FerroWasp does it today
ferrohal-stm32     the neutral HAL
stm32-metapac
```

- FerroHAL knows nothing about FerroForge. FerroForge knows FerroHAL only as backend data: the device path and which HAL crate to select.
- **[Decided]** Hooks the core keeps so an embassy-style async layer can be built on top without a fork:
  1. **Interrupt servicing from a static context:** per-instance static access (safe or `unsafe`) to read and clear flags and to mask interrupt-enable bits. The erased static `Info` from section 4 provides this.
  2. **A raw `unsafe` DMA transfer API** (pointer plus length) alongside the safe API, which takes owned or `'static` buffers. Leak soundness is then the async layer's responsibility.
  3. **Stop and status primitives** reachable directly: request stop, is-running, remaining count and circular mode with half-transfer flags.
  4. **Shared-register writes through a lockable single owner** (section 4).
  5. **Timer handover:** a timer token can be consumed by an external owner, such as a time driver or a monotonic.
  6. **Stealable zero-sized tokens.**
- Wakers, lost wakeups, cancellation semantics and the choice of executor are the async layer's concerns, not the core's.

## 8. FerroForge seam: foot-guns and fixes

The rtic-monotonics versions referred to below are 2.1.0, 2.2.1 and 3.0.0 *(measured)*.

| # | Foot-gun | Fix | Status |
|---|---|---|---|
| 1 | The monotonic's timer is owned twice. rtic-monotonics takes its timer completely (registers, the RCC bit, its own handler, its NVIC priority), while FerroHAL's `Peripherals` would also hand out a token for it | The monotonic setup path consumes FerroHAL's timer token by value. rtic-monotonics 3's `start()` takes no token, so the wrapper does this; start it in init | Still applies after the version bump |
| 2 | No per-IP instance traits | `Instance` checked at construction, then erased (section 4) | Resolved |
| 3 | A chip named twice (FerroHAL names vs rtic-monotonics chip features) | Pass metapac's chip names through. rtic-monotonics 3 selects the chip on the user's own stm32-metapac dependency | Dissolved by the version bump |
| 4 | `rt` enabled by nobody, or by two crates | FerroHAL owns `rt` behind one feature, which the backend turns on | Decided |
| 5 | Two metapac versions in one build | rtic-monotonics 3 accepts stm32-metapac `>=16`, so it unifies with FerroHAL's copy | Dissolved by the version bump |
| 6 | Task crates keyed on chips | HAL-specific task crates are keyed on IP-version features and checked against one representative chip per IP version | Decided |

- **[Decided]** Branch FerroForge and bump it to rtic 2.4.0 and rtic-monotonics 3.0.0. rtic-monotonics 3 is a breaking release: FerroForge's `[timer-monotonic] chip-feature` mechanism must change, interrupt-based monotonics take their priority in `start()`, and fugit moves to 0.6, which affects every task crate that reads time.
- **[Proposal]** FerroForge chooses between the old HAL and FerroHAL through backend data, for example a second backend entry per chip.
- **[Decided]** RTIC itself is not modified.

## 9. Requirements and verification

Decided by the owner on 2026-11-03. The tool that holds evidence and gates is separate from the crate; FerroHAL provides self-contained files with stable hashes and clean feature flags.

### The general rule

- **[Decided]** A car consists of an engine, wheels, a chassis and suspension. The car has its own requirements and sets requirements on its components. A first pass verifies that the car works as required. Later a set of wheels can be replaced by another that fulfils the wheel requirements, without reverifying the car.
- **[Decided]** A layer's interface to the layer below is the requirements it sets there. A change to a part that still fulfils them re-owes only that part. A change to the requirements re-owes the layer that set them.
- **[Decided]** The layers, each with its own requirements and verification: IP block, engine, chip glue, HAL API.
- **[Decided]** One first pass tests everything. After it, only what changed is retested: an IP change retests the IP, a glue change retests the glue.

### Files and hashes

- **[Decided]** Things are grouped into files, for example `usart_v2`. The file states its requirements and is hashed. Verification evidence records the hash, and evidence that does not match the hash is flagged as unverified.
- **[Decided]** Another local file is not part of a file's hash; the requirements cover that link. A consequence: a file serving two IP versions, such as `dma_stream.rs`, has one hash, so changing it re-owes both unless it is split.
- **[Proposal]** The tool keeps two labels for the flag: `stale` when a pass exists and the hash has changed, `unverified` when none ever passed. Both count as not verified and hold a gate closed.
- **[Proposal]** A file's evidence also covers the `macro_rules!` definitions it expands, since a macro changes what a file compiles to without changing its hash. `chips/shared/macros.rs` expands in all four chip files. Requirement docs and markers for internal macros go inside the macro body, so each expansion carries them.

### Dependencies

- **[Decided]** Dependencies are pinned. Bumping one breaks the verification of what uses it, and requires a diff analysis or reverification.
- **[Decided]** Contributors may do a diff analysis. A maintainer must review it. The owner is the only maintainer for now.
- **[Decided]** The compiler does not count as a pinned dependency. Differences between compiler versions are negligible outside safety assurance, and neither FerroHAL nor FerroWasp counts as safety assurance.

### Requirement IDs

- **[Decided]** IDs are named, such as `UART-012`, not paths, and carry no project prefix: the crate name says whose they are. The layer above sets a requirement once, and every part that fulfils it declares the ID, so `usart_v2` and `usart_v4` share one requirement and a replacement part keeps it. A requirement only one part has gets that part's own family, such as `USARTV2-003`. This replaces the earlier decision for path-based IDs after Ferrocene, which tied a requirement to the part that fulfils it and would have needed a rename map.
- **[Decided]** IDs stay unique across projects by naming the catalog, not by a hash. A catalog is named by its crate, and a link names both, such as `ferrohal-stm32:UART-012`, so two catalogs may share a prefix and a reader can still read the link.

### What FerroHAL offers its users

- **[Decided]** FerroHAL presents its requirements and supplies its verification evidence. A project using it maps its own requirements onto them, and a mapped requirement inherits the evidence. For example, a FerroWasp requirement that UART shall be capable of bidirectional communication using DMA maps to `ferrohal-stm32:UART-001`, that the API shall support bidirectional communication using DMA.
- **[Decided]** The mapping lives in the project using FerroHAL. FerroHAL knows nothing of it, as the toaster rule requires.
- **[Proposal]** A mapping records the FerroHAL version and the hash of the requirement's text, so a changed requirement flags the mapping.
- **[Proposal]** A mapped requirement inherits only the evidence for what its image compiles in: the IP versions and the chip's glue.

### Format and tooling

- **[Decided]** A YAML schema is enforced, so broken requirement text fails.
- **[Decided]** Rustdoc presentation: the full requirement text goes on a docs-only page, a `#[cfg(doc)] pub mod requirements` containing one page per IP version, linked from the module. Module docs show only the ID list, so editor hovers stay small. `#[doc(alias = "<ID>")]` on implementing items is optional.
- **[Proposal]** Requirements live in a `requirements.yaml` beside the layer that sets them (`src/uart/requirements.yaml`), so they move with it. A file's doc comment names the IDs it fulfils.
- **[Proposal]** Validation has three layers: the schema (structure), a text lint (vague words, exactly one "shall" per requirement) and a resolver (IDs match real items; every public item in a verified file is covered). These run in a separate publish tool, never in a build script.
- **[Proposal]** The publish tool generates `requirements/index.json` (file hashes and a feature → file → requirement map) and a `requirements.md` per module. CI checks they are in sync with the sources.
- **[Decided]** The tool is `reqtool`, its own crate shared by FerroWasp, FerroForge and FerroHAL (2026-11-03). It evidences a file and its pinned dependencies, without local imports or the compiler.
- **[Decided]** `reqtool` speaks only generic terms, drawn from ordinary requirements practice, and knows nothing of any project. FerroWasp turns on more of it than FerroHAL does (2026-11-03).
- **[Decided]** The hazard decides who runs a test: an agent may run and record what has none, and a person runs what is dangerous or could damage expensive equipment, with its preconditions stated (2026-11-03).

### Not met yet *(measured 2026-11-03)*

- A bench run crosses every layer, since a UART loopback goes through IP, engine, glue and API at once. Each UART check now names the requirements it verifies, but a run still refreshes every requirement naming its procedure: refreshing one layer alone means splitting procedures, not yet done.

## 10. Code generator (parked)

**[Decided]** The generator is not part of the current work. Avoid closing it off:

- keep board facts as data, not code;
- keep routing traits typed per IP and per instance;
- remember that the metapac pin also pins the metadata a generator would read.

## 11. Prototype scope

The integration plan below is superseded in part by §15 (decisions of 2026-10-31): FerroHAL replaces FerroWasp's HALs all at once, later, and the drop-in UART backend is no longer the first step.

- **[Decided]** The first target is the F4 UART RX/TX DMA path (`usart_v2` + `dma_v2`).
- **[Decided]** The prototype is a drop-in backend. FerroWasp's existing traits (`UartRxDmaTransfer`, `UartTxDmaTransfer` in `ferrowasp-stm32-critical`) stay the contract. A thin FerroWasp backend crate implements them over FerroHAL's per-IP API; FerroHAL itself never depends on FerroWasp. Those traits carry F4 concepts (direct-mode error, FIFO error, NDTR), which is known debt.
- **[Decided]** Bench comparison on a Nucleo-F401RE and a Nucleo-G4 without FerroWasp, then an F405 swap into FerroWasp (Foxeer F405 V2) to measure integration cost. Run the old and new backends on the same bumped FerroForge branch, so FerroHAL is the only difference.
- **[Decided]** The prototype is not intended for safety certification.
- **[Open]** The kill condition.
- **[Open]** How to measure reuse (see the constraint below).

Constraint *(measured, stm32-metapac 21)*: F4 and G4 share almost no IP versions, so a G4 UART run measures what a second family costs from scratch, not reuse. Reuse can be measured on GPIO and EXTI between F4 and G4 now, or on UART between G4, H5 and H7 later.

| Peripheral | F401 / F405 | G431 / G474 | F722 | H743 | H563 |
|---|---|---|---|---|---|
| USART | usart_v2 | usart_v4 | usart_v3 | usart_v4 | usart_v4 |
| DMA | dma_v2 | bdma_v1 + dmamux_v1 | dma_v2 | dma_v1 + bdma_v1 + dmamux_v1 | gpdma_v1 |
| Timer | timer_v1 | timer_v2 | timer_v1 | timer_v3 | timer_v2 |
| SPI | spi_v2_i2s | spi_v3_i2s | spi_v3_i2s | spi_v4_i2s | spi_v5_i2s |
| GPIO | gpio_v2 | gpio_v2 | gpio_v2 | gpio_v2 | gpio_v2 |
| EXTI | exti_v1 | exti_v1 | exti_v1 | exti_h7 | exti_h5 |
| RCC / PWR / FLASH | rcc_f4 / pwr_f4 / flash_f4 | rcc_g4 / pwr_g4 / flash_g4c* | – | – | – |

## 12. Deferred

Superseded in part by §15: USB and SD/flash are now required before integration, and FerroWasp migrates all at once, not one transport at a time.

- USB and SDMMC are out of prototype scope. **[Proposal]** Direction for USB: glue for `usb-device`, using `synopsys-usb-otg` (OTG on F4, F7, H7) and `stm32-usbd` (USB FS on G4, H5). The glue enables the clock through FerroHAL's RCC owner; FIFO depth and endpoint count come from a small hand-written per-chip table, since metapac's metadata lacks them.
- Migrating FerroWasp's flight evidence: old and new backends side by side, one transport at a time, DShot last.

## 13. Hard rules for the agent

FerroHAL must not:

- depend on FerroForge, FerroWasp or embassy-stm32;
- define interrupt handlers, set NVIC priorities or enable vectors in the NVIC;
- use critical sections in drivers, or write a shared register outside its single owner;
- contain async code or own a time source;
- panic on a hardware error, or spin without a bound in a stop path;
- choose DMA, pins or timers automatically;
- expose metapac through the safe public API;
- know about safety tiers;
- add the code generator now;
- modify RTIC.

## 14. Reference implementations

Reviewed at the commits listed. Licences: stm32f4xx-hal and stm32h7xx-hal are 0BSD; embassy-stm32, stm32g4xx-hal and imxrt are MIT or Apache-2.0.

| Reference | Use it for | Do not copy |
|---|---|---|
| embassy-stm32 (`fc3a052`) | Structure by IP version, generating `Peripherals` and routing traits from metadata, erased `Info`, clock config per version | Owning interrupts and NVIC priorities, panic on DMA error, global critical sections, unbounded `Drop`. It now pins git stm32-data metadata, so ported code may rely on metadata missing from crates.io metapac 21 |
| stm32f4xx-hal (`0eb673d`) | Type protection (`DMASet`, pin typestates), every DMA flag exposed, `next_transfer`, `&mut RCC` enable, an RTIC 2 monotonic that takes the timer by value | Unbounded spins in `stream_disable` and `wait()` |
| stm32h7xx-hal (`bcb9248`) | Capability-split DMA traits (`Stream`, `DoubleBufferedStream`, `MasterStream`) as the model for adapter traits | Critical sections around RCC |
| imxrt-hal / imxrt-ral / imxrt-dma (`7f900ae` / `02160f8`) | Closest architecture: no NVIC access in library code, an unsafe low-level DMA channel API with explicit flags, `on_interrupt()` called by the user's ISR, a register layer with no resource management, unsafe-hygiene lints | DMA built as futures |

## 15. Integration and scope decisions (2026-10-31)

Taken after the UART prototype ran on the Nucleo-F401RE and Nucleo-H753ZI benches.

### Integration

- **[Decided]** The owner dropped the gate on FerroWasp's refactoring (2026-11-03): FerroHAL is implemented in FerroWasp first, before the rest of that refactoring. It still has to support everything FerroWasp's HALs do today.
- **[Decided]** The swap is all at once. A partial transition (FerroHAL beside stm32f4xx-hal or stm32h7xx-hal in one firmware) is not doable. A transitional `unsafe` path for adopting clocks and peripherals another HAL set up was accepted in principle, but is not needed without a partial transition, and is not built.
- **[Decided]** The H7 is a real target: FerroWasp's TBS Lucid H7 (STM32H743) moves off stm32h7xx-hal as well as the Foxeer F405 V2 off stm32f4xx-hal.
- **[Decided]** Code that exists only for the transition (a compatibility adapter for FerroWasp's `UartRxDmaTransfer` and `UartTxDmaTransfer`, shims) is marked `TRANSITION:` in its documentation, kept in its own module, feature or crate, and listed in DEBT.md, so that it is removed after FerroWasp moves to FerroHAL's native model. FerroHAL itself still never depends on FerroWasp.
- **[Proposal]** The full swap runs FerroWasp's transports through the compatibility adapter first, then moves them to the native model as a separate step.
- **[Decided]** FerroHAL's requirements may come before the swap (2026-11-03). FerroHAL's dependency in FerroWasp is an exact git revision until it is proven, and then FerroHAL is published as a crate (2026-11-02).

### What "supports everything" means

- **[Proposal]** The peripherals FerroWasp's F4 and H7 backends drive, all of which FerroHAL needs before the swap:

| Area | FerroWasp uses it for |
|---|---|
| UART + DMA | serial ports (SBUS, CRSF, MSP, MAVLink, ESC telemetry, CLI) |
| SPI + DMA | the IMU |
| USB OTG FS | CDC serial |
| SD card, flash | H7 SD storage (SDMMC); configuration storage |
| Timers + DMA to GPIO | DShot, the scheduler tick, the timebase |
| EXTI | IMU data ready |
| ADC | battery voltage and current |
| IWDG | the watchdog |
| System | reset, ROM bootloader entry, UID and flash-size signature |
| Clocks, GPIO | everything; HSE on both boards |

### UART design for FerroWasp

- **[Proposal]** Receive never stops the DMA: the circular ring stays, and the interrupt records chunk boundaries (idle line, or the receiver timeout on `usart_v4`) and line errors at their ring positions. A reader takes chunks out of the ring by copy.
- **[Proposal]** Data loss is part of the stream: every overrun, DMA error, line error or restart ends a chunk with a gap and advances a generation counter, so a reader cannot miss a discontinuity. A `Health` snapshot counts each cause.
- **[Proposal]** Transmit is frame-atomic and queued: a frame goes in whole or not at all, and the transmit DMA interrupt chains the next one.
- **[Proposal]** Receive-only UARTs return the same receive type as full ones; `usart_v4` exposes RX and TX inversion.
- **[Proposal]** A UART configuration can be checked at compile time like the clock tree; a driver reports the interrupt vectors it needs and can verify their NVIC state read-only; `Wiring` exports its resources as data for board manifests; long waits can use a caller-supplied clock instead of counted passes.

### Testing

- **[Decided]** Work that can be tested on the Nucleo-F401RE and Nucleo-H753ZI comes first.
- **[Decided]** There is no F405 development board: F405 support is tested on the Foxeer F405 V2.

