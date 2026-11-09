# Debt sweep: detail, gates, cross-repo

Part of [the plan](2026-11-08-debt-sweep.md).

## Detail

- W2: a feed before the window opens resets; `window_us <= timeout_us` validated by the divider arithmetic; v1 (F4) has no window register -> API absent there.
- W3: IWDG has no readable counter (`iwdg_v1.rs` kr/pr/rlr/sr; `iwdg_v2.rs` + winr); timers count at the bus clock and the F4 has no LSI route -> the HAL cannot measure LSI. Bench measures it across a reset (50 ms -> 53.3 ms F401, 50.5 ms H753): per-board constant, no runtime measurement.
- W4b: safe `Pwm::free()` unsound today (`Pwm<T>` holds `PhantomData<T>`; `PwmChannel`/`CompareLane` hold no borrow); `Tick`/`Timebase` non-generic, token erased. Restructure benches now; redesign later.
- W6: leaks = `DmaInstance::regs`, `DmaInstance::mux`, `PortInstance::regs`, SDMMC `Instance::regs`, `Channel::regs`, uart `stream_dma::regs`. No users in `hil/` or ferro-wasp; `mux` (H7 `Dmamux`) is the easily missed one.
- W12: safe-api rules (1) no `pub unsafe fn` outside chip glue beyond the allowlist; (2) item carries `cfg(rtic)`; (3) `default = []`, `rtic = ["rt"]`, nothing implies it; (4) metapac imports only `src/chips/` + 12 drivers.
- Next free IDs: IWDG-004/005/006, PWM-007, USART-005, API-001.
- Masked vectors must not fail `UartIrq::verify()`.
- `checks/no-panic` gains every new public fn: `start_windowed`, `start_with_lsi`, `start_one_pulse`, `mask`, `unmask`.

## Gates

- `checks/run.sh` (clippy x2/chip incl. usb, no-panic, glue, docs; new steps `safe-api`, `no-metapac`, `doc sizes`).
- reqtool; one `shall` per statement.
- RTIC T1-T3: `../notes/2026-11-08-rtic-and-hal-comparison.md`.
- Hardware: runs J/K (freeze), the window run, LSI tight window, one-pulse samplers, masking, full `run-f401.sh`/`run-h753.sh`, `run-dfu.sh` (VTOR), `run-h753.sh m` twice.
- Re-cable: CN13 back from the Pico for run M/DFU, then restore Pico power.
- Cross-repo: `cargo check` the 3 ferro-wasp firmwares; ferro-wasp adds `rtic` where `peripherals = true` (Cargo-only; pinned rev b6d8e84, features rt/defmt/usb).
- CI: iterate green; gitleaks CDN 504 -> `gh run rerun <id> --failed`; never change the workflow.

## Deferred

W14 naming -> `free()` redesign PR. `release()`/`free()` -> same. Input capture, dead time +
break input, FIFOEN, LPUART/BDMA/MDMA, SDMMC IDMA, VOS0/CRS/VCO (DBGMCU IDCODE first).
