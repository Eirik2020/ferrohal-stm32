# Naming audit (evidence)

Audited 2026-11-08/09. Target scheme: `../decisions/2026-11-08-naming-scheme.md`. Renames: W14.

## Catalogs (11 files, two homes)

| file | holds |
|---|---|
| `src/requirements/platform.yaml` | RCC-001..003 (:3-40), GPIO-001 (:62), DMA-001 (:80), WAIT-001 (:98), RES-001 (:117) |
| `src/requirements/reset.yaml` | RESET-001..004 + SIG-001 (:84) |
| `src/uart/requirements.yaml` | UART-001..015, USART-001..004 (:485-542), UARTDMA-001..010, GLUE-001..006 (:582-660) |
| `src/requirements/timer.yaml` | TIMER-*, PWM-*, AWG-* (:203+) |
| co-located | `src/{adc,i2c,spi,uart}/requirements.yaml` |
| central | `src/requirements/{exti,iwdg,sdmmc,usb}.yaml` |

## IDs

- UART has three prefixes (UART/USART/UARTDMA); SPI/I2C/ADC use SPIV2-001/I2CV1-001/ADC-*.
- RES-001 vs RESET-001..004: one letter apart, unrelated.
- SIG-001 = the UID requirement.
- Granularity: DMA-001/GPIO-001/RES-001 one per driver; UART 15+10+4.
- GLUE-* has no catalog of its own.

## Features

- `<ip>_v<metapac version>`: usart_v2/v4, spi_v2/v4, i2c_v1/v2, timer_v1/v3, iwdg_v1/v2, adc_v2/v4, sdmmc_v2, gpio_v2.
- `<ip>_<family>`: exti_v1/exti_h7, rcc_f4/rcc_h7rm0433 (missing underscore).
- Inverted: F4 = `dma_v2`, H7 = `dma_v1` (`Cargo.toml:84` vs `:114`) - metapac versions, not chronology.
- `rt` (vector table/VTOR, `:38`) vs `rtic` (gates `steal`, implies `rt`).

## Modules

- Neutral + per-IP dirs: `uart`/`usart_v*`, `spi`, `i2c`, `adc`.
- One file twice: `src/iwdg.rs` (`lib.rs:71-74` `#[path]`).
- One file split by function: `timer.rs` + `timer_pwm.rs` + `timer_awg.rs` (`timer_v1`/`timer_v3`).

## Benches, scripts, docs

- `run-f401.sh`/`run-h753.sh` (chip) vs `run-dfu.sh` (task) vs `run-i2c-sim.sh` (fixture).
- Crates `nucleo-*-loopback` (board+chip+role) vs `foxeer-f405-v2`/`lucid-h7` (board) vs `pico-i2c-sim` (role).
- Letters uppercase in prose, lowercase in files.
- Root docs caps vs kebab.
- `DEBT.md` §7 says `ferrowasp-ferrohal-critical`; the crate is `ferrowasp-stm32-critical`.

## Consistent

Chip features = metapac chip names; most IP features = metapac versions; `<FAMILY>-NNN`; one
`shall`; `Refused`; module `Requirements:` trailers.

## Collision

The masking requirement planned as `USART-001` already exists (`src/uart/requirements.yaml:485`);
the sweep uses **USART-005**.
