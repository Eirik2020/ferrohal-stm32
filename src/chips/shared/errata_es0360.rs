//! ES0360 Rev 5: the errata of die 0x452 (STM32F72xxx and STM32F73xxx, RM0431), one entry
//! per row of the sheet's summary of device limitations, transcribed on
//! 2026-11-17 (ADR-010). Hand facts: edit an entry's handling as it is reviewed.
//!
//! One revision code, 0x1000, covers every marking (ES0360 Table 2). The
//! verdicts follow the F4 and H7 tables' for the same IP versions and the same
//! driver code; none is proven on a board of this die.
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0360",
    revision: 5,
    revisions: &[(0x1000, "A, 1")],
};

macro_rules! erratum {
    ($section:literal, $title:literal, [$($rev:literal),*], $status:ident, $handling:expr, [$($req:literal),*]) => {
        Erratum {
            sheet: &ERRATA_SHEET,
            section: $section,
            title: $title,
            affected: &[$($rev),*],
            status: SheetStatus::$status,
            handling: $handling,
            requirements: &[$($req),*],
        }
    };
}

/// The I2C reload erratum, which `i2c_v2` checks.
#[rustfmt::skip]
pub const ES0360_2_9_4: Erratum = erratum!("2.9.4", "Last-received byte loss in reload mode", [0x1000], Workaround, Workaround("reads over 255 bytes reload one byte at a time (i2c_v2 read_chunk)"), []);

/// Every erratum of the sheet, in its order: one row each, as the sheet has them.
#[rustfmt::skip]
pub const ERRATA: &[Erratum] = &[
    erratum!("2.1.1", "Cortex®-M7 data corruption when using data cache configured in write-through", [0x1000], Workaround, Firmware("write-back D-cache only; check_dma_buffer refuses cacheable buffers in SRAM1 and SRAM2"), []),
    erratum!("2.2.1", "Internal noise impacting the ADC accuracy", [0x1000], Workaround, Firmware("set PWR_CR1.ADCDC1 or one SYSCFG_PMC.ADCxDC2 bit (AN4073), or average or filter ADC samples"), []),
    erratum!("2.2.2", "Wakeup from Standby mode when the back-up SRAM regulator is enabled", [0x1000], Workaround, NotReached("no Standby entry and no backup-SRAM driver"), []),
    erratum!("2.2.3", "Full JTAG configuration without NJTRST pin cannot be used", [0x1000], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.2.4", "PCROP-protected areas in Flash memory may be unprotected", [0x1000], Workaround, NotReached("no flash-programming or option-byte driver"), []),
    erratum!("2.3.1", "Dummy read cycles inserted when reading synchronous memories", [0x1000], None, NotReached("no FMC driver"), []),
    erratum!("2.3.2", "Wrong data read from a busy NAND memory", [0x1000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.3", "Spurious clock stoppage with continuous clock feature enabled", [0x1000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.4", "Data read might be corrupted when the write FIFO is disabled", [0x1000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.4.1", "First nibble of data not written after a dummy phase", [0x1000], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.2", "Wrong data from memory-mapped read after an indirect mode operation", [0x1000], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.3", "Memory-mapped read operations may fail when timeout counter is enabled", [0x1000], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.5.1", "ADC sequencer modification during conversion", [0x1000], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.6.1", "DMA underrun flag management", [0x1000], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.6.2", "DMA request not automatically cleared by clearing DMAEN", [0x1000], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.7.1", "MCU may remain stuck in LPTIM interrupt when entering Stop mode", [0x1000], Workaround, NotReached("no LPTIM driver, and no Stop entry"), []),
    erratum!("2.8.1", "RTC calendar registers are not locked properly", [0x1000], Partial, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.1", "Wrong data sampling when data setup time (tSU;DAT) is shorter than one I2C kernel clock period", [0x1000], Workaround, Workaround("timing refuses a kernel clock slower than the mode's data setup: 4, 10, 20 MHz"), []),
    erratum!("2.9.2", "Spurious bus error detection in master mode", [0x1000], Workaround, Workaround("BERR is cleared and counted, and the transfer carries on"), []),
    erratum!("2.9.3", "10-bit master mode: new transfer cannot be launched if first part of the address is not acknowledged by the slave", [0x1000], Workaround, Avoided("7-bit addresses only: ADD10 stays clear"), []),
    ES0360_2_9_4,
    erratum!("2.10.1", "nRTS is active while RE or UE = 0", [0x1000], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.11.1", "BSY bit may stay high at the end of a data transfer in Slave mode", [0x1000], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.12.1", "Wrong CCRCFAIL status after a response without CRC is received", [0x1000], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.12.2", "MMC stream write of less than 8 bytes does not work correctly", [0x1000], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.13.1", "BxCAN time triggered mode communication not supported", [0x1000], None, NotReached("no CAN driver"), []),
];
