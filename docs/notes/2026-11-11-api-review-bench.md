# Bench runs and baseline, B1-B3 (API review)

Baseline 2026-11-11; run letters per bench. Hub: [2026-11-11-api-review.md](2026-11-11-api-review.md). `main` `d2bdccd`.

## Baseline (measured 2026-11-11)

- `hil/run-f401.sh`: 22 runs, 318 checks, 0 failed.
- `hil/run-h753.sh`: 22 runs, 532 checks, 0 failed (M off its list).
- `checks/run.sh`: 23/23, four chips; again after the index split.
- Not run: Foxeer, Lucid (no probe).

## Run letters (B1)

From each `hil/*/src/bin/run_*.rs` module header.

| test | F401 | H753 |
|---|---|---|
| every timer vs DWT | H | I |
| DShot lanes + sampler | I | J |
| watchdog across reset | J | K |
| polled SPI | K | L |
| ROM bootloader request | L | N |
| ADC | M | O |
| TIM1 triggers TIM3/TIM8 | N | P |
| waveform generator | O, P | Q |
| I2C master | Q | R |
| I2C vs Pico simulator | S | S |
| each timer's second use | U | T |
| constructor refusals | V | U |
| injected faults | W | W |
| USB CDC | - | M |
| cache + MPU matrix | - | X |

## Findings

| id | sev | finding | evidence |
|---|---|---|---|
| B1 | L | A letter means a different test on each bench; only S and W match | table. Docs cope by naming the bench beside the letter (`DEBT.md` "TIM3->TIM2 on the F4s (run R)", `AGENTS.md` "run M + DFU need H753 CN13"); a bare "run K" is ambiguous. ADR-003 rule 6 fixes only the letter's case |
| B2 | L | One bench generated, one hand-written | H753 runs: `tools/gen_h753_runs.py`; F401 runs: by hand |
| B3 | L | Bench encodes D7 | `hil/nucleo-f401re-loopback/src/bin/run_v.rs:499` expects `UartConfigError::Unsupported` from a timer constructor |

## Fix (opinion)

- B1: name files by test (`run_watchdog.rs`, `run_refusals.rs`); letters, if kept, as per-bench aliases in `requirements/benches.md`. Failing that: one letter registry for every bench, gaps where a bench lacks the test.
- B2: one way for both; decide with B1.
- B3: changes with D7.
