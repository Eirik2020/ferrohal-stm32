//! ES0290 Rev 7: the errata of die 0x449 (STM32F74xxx and STM32F75xxx, RM0385), one entry
//! per row of the sheet's summary of device limitations, transcribed on
//! 2026-11-17 (ADR-010). Hand facts: edit an entry's handling as it is reviewed.
//!
//! One revision code, 0x1001, covers every marking (ES0290 Table 2). The
//! verdicts follow the F4 and H7 tables' for the same IP versions and the same
//! driver code; none is proven on a board of this die.
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0290",
    revision: 7,
    revisions: &[(0x1001, "Z, 1")],
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
pub const ES0290_2_9_4: Erratum = erratum!("2.9.4", "Last-received byte loss in reload mode", [0x1001], Workaround, Workaround("reads over 255 bytes reload one byte at a time (i2c_v2 read_chunk)"), []);

/// Every erratum of the sheet, in its order: one row each, as the sheet has them.
#[rustfmt::skip]
pub const ERRATA: &[Erratum] = &[
    erratum!("2.1.1", "Cortex®-M7 data corruption when using data cache configured in write-through", [0x1001], Workaround, Firmware("write-back D-cache only; check_dma_buffer refuses cacheable buffers in SRAM1 and SRAM2"), []),
    erratum!("2.2.1", "Missed ADC triggers from TIM1/TIM8, TIM2/TIM5/TIM4/TIM6/TRGO or TGRO2 event", [0x1001], Workaround, NotReached("conversions start by software (SWSTART): no timer trigger"), []),
    erratum!("2.2.2", "Internal noise impacting the ADC accuracy", [0x1001], Workaround, Firmware("set PWR_CR1.ADCDC1 or one SYSCFG_PMC.ADCxDC2 bit (AN4073), or average or filter ADC samples"), []),
    erratum!("2.2.3", "Wakeup from Standby mode when the back-up SRAM regulator is enabled", [0x1001], Workaround, NotReached("no Standby entry and no backup-SRAM driver"), []),
    erratum!("2.2.4", "Full JTAG configuration without NJTRST pin cannot be used", [0x1001], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.3.1", "Dummy read cycles inserted when reading synchronous memories", [0x1001], None, NotReached("no FMC driver"), []),
    erratum!("2.3.2", "Wrong data read from a busy NAND memory", [0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.3", "Spurious clock stoppage with continuous clock feature enabled", [0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.4", "Data read might be corrupted when the write FIFO is disabled", [0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.4.1", "Extra data written in the FIFO at the end of a read transfer", [0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.2", "First nibble of data not written after a dummy phase", [0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.3", "Wrong data from memory-mapped read after an indirect mode operation", [0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.4", "Memory-mapped read operations may fail when timeout counter is enabled", [0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.5.1", "ADC sequencer modification during conversion", [0x1001], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.6.1", "DMA underrun flag management", [0x1001], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.6.2", "DMA request not automatically cleared by clearing DMAEN", [0x1001], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.7.1", "MCU may remain stuck in LPTIM interrupt when entering Stop mode", [0x1001], Workaround, NotReached("no LPTIM driver, and no Stop entry"), []),
    erratum!("2.8.1", "RTC calendar registers are not locked properly", [0x1001], Partial, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.2", "Spurious tamper detection when disabling the tamper channel", [0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.1", "Wrong data sampling when data setup time (tSU;DAT) is shorter than one I2C kernel clock period", [0x1001], Workaround, Workaround("timing refuses a kernel clock slower than the mode's data setup: 4, 10, 20 MHz"), []),
    erratum!("2.9.2", "Spurious bus error detection in master mode", [0x1001], Workaround, Workaround("BERR is cleared and counted, and the transfer carries on"), []),
    erratum!("2.9.3", "10-bit master mode: new transfer cannot be launched if first part of the address is not acknowledged by the slave", [0x1001], Workaround, Avoided("7-bit addresses only: ADD10 stays clear"), []),
    ES0290_2_9_4,
    erratum!("2.10.1", "Start bit detected too soon when sampling for NACK signal from the smartcard", [0x1001], None, Avoided("no smartcard mode"), []),
    erratum!("2.10.2", "Break request can prevent the transmission complete flag (TC) from being set", [0x1001], Workaround, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.10.3", "nRTS is active while RE or UE = 0", [0x1001], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.11.1", "I2S slave in PCM short pulse mode sensitive to timing between WS and CK", [0x1001], Workaround, NotReached("no I2S driver"), []),
    erratum!("2.11.2", "BSY bit may stay high at the end of a data transfer in Slave mode", [0x1001], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.12.1", "Wrong CCRCFAIL status after a response without CRC is received", [0x1001], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.12.2", "MMC stream write of less than 8 bytes does not work correctly", [0x1001], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.13.1", "BxCAN time triggered mode communication not supported", [0x1001], None, NotReached("no CAN driver"), []),
    erratum!("2.14.1", "Incorrect layer 3 (L3) checksum is inserted in transmitted IPv6 packets without TCP, UDP or ICMP payloads", [0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.14.2", "The Ethernet MAC processes invalid extension headers in the received IPv6 frames", [0x1001], None, NotReached("no Ethernet driver"), []),
    erratum!("2.14.3", "MAC stuck in the Idle state on receiving the TxFIFO flush command exactly 1 clock cycle after a transmission completes", [0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.14.4", "Transmit frame data corruption", [0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.14.5", "Successive write operations to the same register might not be fully taken into account", [0x1001], Workaround, NotReached("no Ethernet driver"), []),
];
