# FerroSmith board file and MCU profile schema

Draft schema for FerroSmith's two hardware inputs, and what `ferrohal-stm32`
needs to consume them. It supersedes `ferrosmith-sbd-dialect.md`.

Status tags, as in FerroHAL's architecture brief:

- **[Decided]**: agreed by the owner.
- **[Proposal]**: current direction, not yet confirmed.
- **[Open]**: undecided.

FerroHAL facts were read from `ferrohal-stm32` at `704e578` (2026-11-05).
Examples: `foxeer-f405-v2.yaml` (board), `stm32f405rg-fc.profile.yaml` (profile).

---

## 1. Layers

| Layer | Holds | Owner |
|---|---|---|
| Board file | Physical facts of one PCB revision | Electronics engineer |
| MCU profile | Peripheral instances, DMA streams, timer channels, clock request | Embedded engineer |
| App composition | Roles (RC, MSP, telemetry), system timers, priorities | Firmware engineer |
| Boot-time config | What is plugged in, what each input/output is used for | System engineer |
| Runtime config | Calibration, tuning | Operator |

- **[Decided]** The board file holds only immutable board facts the firmware
  cannot get any other way. Anything software can calibrate or detect stays out.
- **[Decided]** No resolver. Peripheral instance, DMA stream and timer channel
  are stated explicitly in the profile.
- **[Decided]** Names describe the signal at the pad (UART, SPI, timer output),
  not the block that generates it (peripheral, PIO, FPGA).

## 2. Board file

### 2.1 Format

- **[Decided]** A dialect of Ariel OS's sbd. Plain sbd fields are used as-is;
  everything else sits under `ferro:` inside each target.
- **[Decided]** Generic additions are proposed upstream and staged in the
  dialect while pending.
- **[Decided]** Parsing is strip-then-parse: remove `ferro`, deserialize the rest
  with upstream `sbd-gen-schema`, parse `ferro` with FerroSmith's own types
  (`deny_unknown_fields`). Stripping must leave a valid upstream file (verified
  against `sbd-gen-schema` 0.6.0, format 0.4.1).
- **[Decided]** One complete target per board revision; no inheritance.
  (sbd-gen merges by identical key only, replaces lists, and ignores `include`.)

### 2.2 Identifiers and references

- **[Decided]** IDs are lowercase snake_case, unique per target.
- **[Decided]** Dialect collections are mappings keyed by ID.
- **[Decided]** In plain-sbd lists, `aliases[0]` is the ID.
- **[Decided]** A reference is `<id>` or `<id>.<signal>`. Signal names are the
  schema's field names minus `_pin`: `rx`, `tx`, `cts`, `rts`, `sck`, `miso`,
  `mosi`, `scl`, `sda`, `cs`, `int`.
- **[Decided]** UPPERCASE is a net name (`VBAT`, `GND`, `5V`, `ESC_CURR`).
  `NC` is reserved for not connected and may not name a net.

### 2.3 Plain sbd fields

| Field | Content |
|---|---|
| `chip` | metapac chip name, e.g. `stm32f405rg` |
| `description`, `flags` | Free text; flags such as `has_usb_device_port` |
| `leds` | `pin`, optional `color`, optional `active` (`high`/`low`), `aliases` |
| `buttons` | `pin`, optional `active`, `aliases` |
| `uarts` | `rx_pin`, `tx_pin`, optional `cts_pin`/`rts_pin`, `aliases` |
| `i2c` | `scl_pin`, `sda_pin`, `aliases` |

- **[Decided]** `active` is optional when unknown.

### 2.4 Dialect fields (`ferro:`)

| Field | Content | Status |
|---|---|---|
| `schema` | Dialect version | [Proposal] |
| `clocks.hse_hz` | External oscillator frequency | [Proposal] |
| `clocks.hse_mode` | `crystal` or `external` (bypass) | [Proposal], new |
| `usb.vbus_sense_pin` | Pin VBUS is routed to for cable detection; absent if not routed | [Proposal] |
| `spi.<bus>` | `sck_pin`, `miso_pin`, `mosi_pin`, `devices` | [Proposal] |
| `spi.<bus>.devices.<id>` | `compatible` or `one_of`, `cs_pin`, optional `int_pin`, `rotation`, `card_detect` | [Proposal] |
| `i2c_devices.<id>` | `bus` (sbd alias), `compatible` or `one_of`, optional `address` | [Proposal] |
| `sdmmc.<id>` | `clk_pin`, `cmd_pin`, `data_pins` (count = bus width), `card_detect` | [Proposal] |
| `adc.<id>` | `pin`, `net`, optional `scale` | [Decided] |
| `timer_outputs.<id>` | `pin`, optional `fixed` (e.g. `esc` on an AIO) | [Decided] |
| `gpio.<id>` | `pin`, optional `active` | [Proposal] |
| `connectors.<id>` | `part`, `pins` | [Decided] |

### 2.5 Field rules

- **Devices.** **[Decided]** A device has either `compatible` (one part) or
  `one_of` (alternative parts on one slot, detected at boot), never both.
  `one_of` is keyed by compatible string. Shared wiring sits on the slot,
  `rotation` per alternative. Compatible strings are devicetree style
  (`vendor,part`) and must not go in YAML flow lists, where the comma splits them.
- **I2C address.** **[Decided]** Optional; it is detectable by probing.
- **Rotation.** **[Decided]** Euler degrees only, `{ roll, pitch, yaw }`,
  mapping sensor frame to board frame; board frame FRD; applied ZYX. Importers
  convert other forms. **[Proposal]** Rotations are compared as matrices.
- **ADC.** **[Decided]** `value = pin_mV × scale`, `scale` defaulting to 1.0.
  `net` is required and names the net measured, upstream of any conditioning
  (`VBAT`, not the divider midpoint). No offset and no reference voltage:
  calibration is software, the reference belongs to the MCU side. What an input
  is used for is bound in boot-time config.
- **Timer outputs.** **[Decided]** Pads driven by hardware-timed waveform
  generation. Function is assigned in boot-time config unless `fixed`.
- **Storage.** **[Decided]** SPI flash is a bus device (`jedec,spi-nor`); capacity
  and part ID are detectable. An SD card on SPI is a bus device
  (`mmc-spi-slot`); on SDIO/SDMMC it has its own section. Storage purpose is
  boot-time config.
- **Connectors and pad groups.** **[Decided]** A connector is any physical group
  of contacts. Numbered parts list `pins` from pin 1; `part: pads` lists
  `{ label, ref }` pairs, because silkscreen labels repeat. Labels live only on
  groups. Every item exposed off-board appears in at least one group; internal
  items are exempt; an item may appear in several groups. No raw pin names in
  connectors. **[Proposal]** Imported boards may lack groups; the rule is then a
  warning.
- **Jumpers.** **[Proposal]** Signal jumpers declared with `positions` (a ref per
  position); the fitted position is chosen in boot-time config.
  **[Open]** Whether pad groups are firmware scope only (strip power nets,
  power jumpers out) or also integration information (power jumpers modelled
  the same way).

### 2.6 Importers

- **[Decided]** When a source is silent, the importer applies that firmware's
  default (Betaflight `vbat_scale` 110 → `scale: 11.0`), never the dialect's.

## 3. MCU profile

- **[Proposal]** One file per MCU configuration, reusable by any board whose
  pins match.
- **[Proposal]** Keys are reference-manual peripheral names (`USART2`, `TIM1`,
  `CH3N`). DMA streams are named `DMA2_STREAM5`.
- **[Proposal]** Explicit where the silicon offers a choice: peripheral
  instance, DMA stream, timer and channel, ADC instance. Omitted where it
  offers none: alternate function, DMA request (fixed by peripheral and stream
  on `dma_v2`, by DMAMUX request ID on H7), EXTI line, ADC channel, USB pins.
- **[Proposal]** Partial by design: a board item with no profile entry is
  unavailable (warning). A peripheral may route only some signals (RX only).
- **[Proposal]** Binding: each board item maps to exactly one profile entry by
  type and pins, or validation fails.
- **[Decided]** Clocks are a request (`sysclk_hz`, `usb_hz`); FerroHAL computes
  and validates the tree from it and the board's HSE.
- **[Open]** Supply voltage range (FerroHAL's `Supply`): assume 2.7–3.6 V, or
  state it in the board file.

| Field | Content |
|---|---|
| `chip` | metapac chip name |
| `clocks` | `sysclk_hz`, `usb_hz` |
| `usb` | USB instance (`OTG_FS`) |
| `usart.<INST>.rx`/`.tx` | `pin`, `dma` |
| `spi.<INST>` | `sck`, `miso`, `mosi`, optional `dma: { rx, tx }` (none = polled) |
| `adc.<INST>` | `pins`, `dma` |
| `timers.<TIM>.<CHn or CHnN>` | `pin`, `dma` |

- **Out of the profile:** roles, system timers (scheduler, timebase,
  watchdog), RTIC dispatchers, priorities. FerroSmith checks resource conflicts
  across profile and app together.

## 4. Generated code

- **[Proposal]** Logic lives in FerroHAL helpers; the generator only translates
  data into their types and consts. Helpers stay generic: FerroHAL knows
  nothing about board files or FerroSmith.
- **[Proposal]** Four pieces:

| Piece | From | Contents | Depends on |
|---|---|---|---|
| Board facts | board file | identity, rotation, ADC net/scale, alternatives, groups | typed-units crate only |
| MCU profile crate | profile | one wiring alias per routed entry, manifest test against `RESOURCES` | `ferrohal-stm32` |
| Binding | board + profile | `Board`/`Rest` struct named by board IDs, clock config, binding checks | both |
| Firmware | hand-written | `ferroforge::app!` | binding |

- **[Proposal]** `Board` holds assembled wirings, not drivers; pub fields and no
  `Drop`, so RTIC `init` can destructure it; unbound peripherals and the GPIO
  port controls are handed back in `Rest`.
- **[Proposal]** Route errors are compile errors in generated code. FerroSmith
  may pre-check against metapac for friendlier messages.
- **[Proposal]** Generated output uses create/update/check modes with a tag
  file, and CI runs `check`.

## 5. FerroHAL support

Measured at `704e578`.

### 5.1 Element by element

| Schema element | In FerroHAL today | Needed |
|---|---|---|
| `chip` | Chip features are metapac names; four chips (F401RE, F405RG, H743VI, H753ZI) | Glue rows per new chip, by hand (§10 of the brief parks the generator) |
| `clocks.hse_hz`, `hse_mode` | `rcc::Config` with `Hse { freq, mode }`, const `validate()` | Nothing |
| Profile `clocks` request | Explicit config only; brief §6: solver optional, later, compile time | **Const clock solver** (or explicit tree in the profile until it exists) |
| `usb` | `UsbFs` on OTG_FS, behind `usb` | Nothing for the profile |
| `usb.vbus_sense_pin` | No VBUS sensing in `usb.rs` | VBUS sense, hardware or GPIO, if a board needs it |
| `uarts` | `uart::Wiring`, `RxWiring`, both with `RESOURCES` | TX-only wiring if a profile routes TX alone; RTS/CTS (DEBT: not supported) |
| `i2c`, `i2c_devices` | Tokens only (`I2C1`–`I2C3` on F405); no driver, no open-drain GPIO mode | **I2C driver with wiring and `RESOURCES`; open-drain pin mode** |
| `spi.<bus>` | `spi::Wiring` (DMA), `BlockingWiring` (polled), both with `RESOURCES` | Nothing for routing |
| `cs_pin` | GPIO push-pull output | Nothing |
| `int_pin` | `exti::Line<N>` | Nothing |
| `sdmmc` | `sdmmc_v2` on H7, `Wiring` with D0–D3 only | 1-bit wiring for `data_pins` of one; `RESOURCES` |
| `card_detect` | GPIO input | Nothing (the app reads it) |
| `adc` | `AdcPin` glue with `CHANNEL`; ADC1; single or scan over DMA | **ADC wiring type with `RESOURCES`** |
| `timer_outputs` | `Pwm`, `ChannelPin`/`ComplementaryPin`, `CompareLane` via `CompareDma`, `Awg` incl. two-way lanes and active-low outputs | **Lane wiring type with `RESOURCES`** |
| `gpio`, `leds`, `buttons` | Push-pull output, input | Nothing (polarity is applied above the HAL) |
| Devices, rotation, ADC net/scale, groups, jumpers | n/a | Nothing (board facts crate) |

### 5.2 Cross-cutting additions

1. **`Resource` and `Signal` coverage.** `Signal` has `Tx`, `Rx`, `Sck`, `Miso`,
   `Mosi` only. Timer channels (with complementary), ADC inputs, SDMMC and I2C
   signals need variants, so every wiring describes itself for the generated
   manifest test.
2. **AF-free pin aliases.** Glue rows already carry the triple
   (`pins!(TxPin<USART1>: ('A', 9, 7))`), and `AltPin` reads port, pin and AF
   back from a type. The rows could also emit an associated pin type per
   (instance, signal, pin), e.g. `uart::Tx<USART1, 'A', 9>`, so generated code
   needs no AF lookup.
3. **Type-driven constructors (`assemble`).** Convert pin tokens by type and
   return `Refused` on error, matching the hand-back DEBT already plans for
   every constructor.
4. **Const `PinConfig { pull, speed }`** accepted by constructors.
   **[Open]** Whether its values are board facts, profile choices or
   per-signal HAL defaults.
5. **Package-aware pins.** DEBT: `split()` hands out all 16 pins of every port,
   bonded or not. A per-package pin list in the glue makes an unbonded pin in a
   board file a compile error.
6. **Const clock solver** (5.1): deterministic selection rule,
   constraint-aware (USB 48 MHz), exposing the chosen tree.

### 5.3 Consistency with the architecture brief

- The additions are HAL API in the trait shapes the brief already uses; none
  of them is the code generator, which §10 parks and §13 forbids adding now.
  FerroSmith's generator lives outside FerroHAL.
- The no-resolver rule (§5) holds: helpers check and convert stated routes,
  and the clock solver computes registers from a stated request, as §6 allows.
- None of the additions blocks FerroWasp's swap (§15): everything the swap
  uses already exists, hand-wired. The additions are what the generated path
  needs. I2C, 1-bit SDMMC, VBUS sense and TX-only UART only matter for boards
  beyond FerroWasp's current two.
- Doc drift: the README still lists SDMMC under "Not yet", though `sdmmc_v2`
  exists and DEBT describes it.
