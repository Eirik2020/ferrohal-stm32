# ADR-005: FerroWasp integration and scope (2026-10-31)

ADR-005 - Status: accepted

Taken after the UART prototype ran on the Nucleo-F401RE and Nucleo-H753ZI
benches; recorded from the architecture map's section 15 when the map dissolved
(2026-11-10).

## Integration

- **[Decided 2026-11-13, owner]** FerroWasp is FerroHAL's first-class supported user: FerroWasp's needs drive what FerroHAL builds and in what order. The toaster rule still holds: a FerroWasp need lands as a generic capability, named, specified and tested in FerroHAL's own terms, never as FerroWasp-specific logic, and FerroHAL never depends on FerroWasp. Binding text: [`../design/scope.md`](../design/scope.md).
- **[Decided]** The owner dropped the gate on FerroWasp's refactoring (2026-11-03): FerroHAL is implemented in FerroWasp first, before the rest of that refactoring. It still has to support everything FerroWasp's HALs do today.
- **[Decided]** The swap is all at once. A partial transition (FerroHAL beside stm32f4xx-hal or stm32h7xx-hal in one firmware) is not doable. A transitional `unsafe` path for adopting clocks and peripherals another HAL set up was accepted in principle, but is not needed without a partial transition, and is not built.
- **[Decided]** The H7 is a real target: FerroWasp's TBS Lucid H7 (STM32H743) moves off stm32h7xx-hal as well as the Foxeer F405 V2 off stm32f4xx-hal.
- **[Decided]** Code that exists only for the transition (a compatibility adapter for FerroWasp's `UartRxDmaTransfer` and `UartTxDmaTransfer`, shims) is marked `TRANSITION:` in its documentation, kept in its own module, feature or crate, and listed in DEBT.md, so that it is removed after FerroWasp moves to FerroHAL's native model. FerroHAL itself still never depends on FerroWasp.
- **[Proposal]** The full swap runs FerroWasp's transports through the compatibility adapter first, then moves them to the native model as a separate step.
- **[Decided]** FerroHAL's requirements may come before the swap (2026-11-03). FerroHAL's dependency in FerroWasp is an exact git revision until it is proven, and then FerroHAL is published as a crate (2026-11-02).

## What "supports everything" means

- **[Decided 2026-11-13]** (a proposal until FerroWasp became first-class) The peripherals FerroWasp's F4 and H7 backends drive, all of which FerroHAL needs before the swap:

| Area | FerroWasp uses it for |
|---|---|
| UART + DMA | serial ports (SBUS, CRSF, MSP, MAVLink, ESC telemetry, CLI) |
| SPI + DMA | the IMU |
| I2C | the barometer (DPS310) and the GNSS's DDC mode, both new with the driver crates; added to the table 2026-11-06 |
| USB OTG FS | CDC serial |
| SD card, flash | H7 SD storage (SDMMC); configuration storage |
| Timers + DMA to GPIO | DShot, the scheduler tick, the timebase |
| EXTI | IMU data ready |
| ADC | battery voltage and current |
| IWDG | the watchdog |
| System | reset, ROM bootloader entry, UID and flash-size signature |
| Clocks, GPIO | everything; HSE on both boards |

The UART design for FerroWasp proposals moved to
[../design/uart-ferrowasp.md](../design/uart-ferrowasp.md) when this record was
made from the architecture map (2026-11-10); they are realized in the shipped
UART engine.

## Testing

- **[Decided]** Work that can be tested on the Nucleo-F401RE and Nucleo-H753ZI comes first.
- **[Decided]** There is no F405 development board: F405 support is tested on the Foxeer F405 V2.
