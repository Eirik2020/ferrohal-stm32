//! ES0334 Rev 9: the errata of die 0x451 (STM32F76xxx and STM32F77xxx, RM0410), one entry
//! per row of the sheet's summary of device limitations, transcribed on
//! 2026-11-17 (ADR-010). Hand facts: edit an entry's handling as it is reviewed.
//!
//! Two revision codes (ES0334 Table 2): 0x1000 is revision A, 0x1001 covers
//! Z and 1, whose columns agree on every row. The verdicts follow the F4, F7
//! and H7 tables' for the same IP versions and the same driver code; none is
//! proven on a board of this die.
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0334",
    revision: 9,
    revisions: &[(0x1000, "A"), (0x1001, "Z, 1")],
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
pub const ES0334_2_13_5: Erratum = erratum!("2.13.5", "Last-received byte loss in reload mode", [0x1000, 0x1001], Partial, Workaround("reads over 255 bytes reload one byte at a time (i2c_v2 read_chunk)"), []);

/// The DTCM erratum, which `check_dma_buffer` checks: on revision A a DMA
/// reads zeros from the DTCM while the core sleeps.
#[rustfmt::skip]
pub const ES0334_2_2_4: Erratum = erratum!("2.2.4", "DTCM-RAM not accessible in read when the MCU is in Sleep mode (WFI/WFE)", [0x1000], Workaround, Workaround("on revision A, check_dma_buffer refuses DTCM buffers: SRAM1 and SRAM2 only"), []);

/// Every erratum of the sheet, in its order: one row each, as the sheet has them.
#[rustfmt::skip]
pub const ERRATA: &[Erratum] = &[
    erratum!("2.1.1", "Cortex-M7 data corruption when using Data cache configured in write-through", [0x1000, 0x1001], None, Firmware("write-back D-cache only; check_dma_buffer refuses cacheable buffers in SRAM1 and SRAM2"), []),
    erratum!("2.2.1", "Internal noise impacting the ADC accuracy", [0x1000, 0x1001], Workaround, Firmware("set PWR_CR1.ADCDC1 or one SYSCFG_PMC.ADCxDC2 bit (AN4073), or average or filter ADC samples"), []),
    erratum!("2.2.2", "Wakeup from Standby mode when the back-up SRAM regulator is enabled", [0x1000, 0x1001], Workaround, NotReached("no Standby entry and no backup-SRAM driver"), []),
    erratum!("2.2.3", "LSE high driving and low driving capability is not usable for TFBGA216 package under certain conditions", [0x1000], Workaround, NotReached("no LSE or RTC driver"), []),
    ES0334_2_2_4,
    erratum!("2.2.5", "Full JTAG configuration without NJTRST pin cannot be used", [0x1000, 0x1001], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.2.6", "PC13 signal transitions disturb LSE", [0x1000, 0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.3.1", "Dummy read cycles inserted when reading synchronous memories", [0x1000, 0x1001], None, NotReached("no FMC driver"), []),
    erratum!("2.3.3", "Wrong data read from a busy NAND memory", [0x1000, 0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.4", "Spurious clock stoppage with continuous clock feature enabled", [0x1000, 0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.5", "Data read might be corrupted when the write FIFO is disabled", [0x1000, 0x1001], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.4.1", "First nibble of data not written after dummy phase", [0x1000, 0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.2", "Wrong data from memory-mapped read after an indirect mode operation", [0x1000, 0x1001], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.3", "Memory-mapped read operations may fail when timeout counter is enabled", [0x1000, 0x1001], Partial, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.4", "Memory-mapped access in indirect mode clearing QUADSPI_AR register", [0x1000, 0x1001], Partial, NotReached("no QUADSPI driver"), []),
    erratum!("2.5.1", "ADC sequencer modification during conversion", [0x1000, 0x1001], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.6.1", "DMA request not automatically cleared by clearing DMAEN", [0x1000, 0x1001], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.6.2", "DMA underrun flag not set when an internal trigger is detected on the clock cycle of the DMA request acknowledge", [0x1000, 0x1001], None, NotReached("no DAC driver"), []),
    erratum!("2.6.3", "DMA underrun flag management", [0x1000, 0x1001], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.7.1", "Tearing effect parasitic detection", [0x1000, 0x1001], Partial, NotReached("no DSI driver"), []),
    erratum!("2.7.2", "Incorrect calculation of the time to activate the clock between HS transmissions", [0x1000, 0x1001], Partial, NotReached("no DSI driver"), []),
    erratum!("2.7.3", "The immediate update procedure may fail", [0x1000, 0x1001], Workaround, NotReached("no DSI driver"), []),
    erratum!("2.7.4", "When used over the DSI link, the tearing effect interrupt flag is set when an acknowledge trigger is received from the display", [0x1000, 0x1001], Workaround, NotReached("no DSI driver"), []),
    erratum!("2.8.1", "False EOI marker is inserted after clearing the HDR bit", [0x1000, 0x1001], Workaround, NotReached("no JPEG driver"), []),
    erratum!("2.8.2", "No DMA transfer complete generated at the end of the encoding process after clearing the HDR bit", [0x1000, 0x1001], Workaround, NotReached("no JPEG driver"), []),
    erratum!("2.8.3", "JPEG FIFO might be corrupted", [0x1000, 0x1001], Workaround, NotReached("no JPEG driver"), []),
    erratum!("2.10.1", "One-pulse mode trigger not detected in master-slave reset + trigger configuration", [0x1000, 0x1001], Partial, Avoided("MSM is never set"), []),
    erratum!("2.10.2", "Consecutive compare event missed in specific conditions", [0x1000, 0x1001], None, Workaround("a lane refuses a duty within `Pwm::top_margin` of the period's end followed by 0 (PWM-009)"), []),
    erratum!("2.10.3", "Output compare clear not working with external counter reset", [0x1000, 0x1001], Partial, Avoided("no ocref_clr and no reset slave mode"), []),
    erratum!("2.11.1", "Device may remain stuck in LPTIM interrupt when entering Stop mode", [0x1000, 0x1001], Workaround, NotReached("no LPTIM driver, and no Stop entry"), []),
    erratum!("2.11.2", "Device may remain stuck in LPTIM interrupt when clearing event flag", [0x1000, 0x1001], Partial, NotReached("no LPTIM driver"), []),
    erratum!("2.11.3", "LPTIM events and PWM output are delayed by 1 kernel clock cycle", [0x1000, 0x1001], Partial, NotReached("no LPTIM driver"), []),
    erratum!("2.12.1", "RTC calendar registers are not locked properly", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.12.2", "RTC interrupt can be masked by another RTC interrupt", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.12.3", "Calendar initialization may fail in case of consecutive INIT mode entry", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.12.4", "Alarm flag may be repeatedly set when the core is stopped in debug", [0x1000, 0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.13.1", "10-bit master mode: new transfer cannot be launched if first part of the address is not acknowledged by the slave", [0x1000, 0x1001], Workaround, Avoided("7-bit addresses only: ADD10 stays clear"), []),
    erratum!("2.13.3", "Wrong data sampling when data setup time (tSU;DAT) is shorter than one I2C kernel clock period", [0x1000, 0x1001], Partial, Workaround("timing refuses a kernel clock slower than the mode's data setup: 4, 10, 20 MHz"), []),
    erratum!("2.13.4", "Spurious bus error detection in master mode", [0x1000, 0x1001], Workaround, Workaround("BERR is cleared and counted, and the transfer carries on"), []),
    ES0334_2_13_5,
    erratum!("2.13.6", "Spurious master transfer upon own slave address match", [0x1000, 0x1001], Partial, Avoided("controller only: no own address is enabled"), []),
    erratum!("2.13.7", "OVR flag not set in underrun condition", [0x1000, 0x1001], None, Avoided("controller only: no target mode"), []),
    erratum!("2.13.8", "Transmission stalled after first byte transfer", [0x1000, 0x1001], Workaround, Avoided("the kernel clock is the bus clock (freeze writes DCKCFGR2's reset value), so the APB-to-kernel ratio is 1"), []),
    erratum!("2.14.1", "RTS is active while RE = 0 or UE = 0", [0x1000, 0x1001], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.14.2", "Receiver timeout counter wrong start in two-stop-bit configuration", [0x1000, 0x1001], Workaround, Avoided("no receiver timeout: RTOEN stays clear"), []),
    erratum!("2.14.3", "Data corruption due to noisy receive line", [0x1000, 0x1001], None, Board("a clean receive line; filter a noisy one"), []),
    erratum!("2.15.1", "BSY bit may stay high when SPI is disabled", [0x1000, 0x1001], Workaround, Avoided("SPE drops only in configure and free, after done() saw the transmit FIFO empty and BSY clear"), []),
    erratum!("2.15.2", "BSY bit may stay high at the end of data transfer in slave mode", [0x1000, 0x1001], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.15.3", "Wrong CRC in full-duplex mode handled by DMA with imbalanced setting of data counters", [0x1000, 0x1001], Workaround, Avoided("CRCEN stays clear"), []),
    erratum!("2.16.2", "Last SAI_SCK clock pulse truncated upon disabling SAI master", [0x1000, 0x1001], None, NotReached("no SAI driver"), []),
    erratum!("2.16.3", "Last SAI_MCLK clock pulse truncated upon disabling SAI master", [0x1000, 0x1001], Workaround, NotReached("no SAI driver"), []),
    erratum!("2.16.4", "SAI_MCLK clock absent in a specific configuration", [0x1000, 0x1001], Workaround, NotReached("no SAI driver"), []),
    erratum!("2.17.1", "Wrong CCRCFAIL status after a response without CRC is received", [0x1000, 0x1001], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.17.2", "MMC stream write of less than seven bytes does not work correctly", [0x1000, 0x1001], Workaround, NotReached("no SDMMC driver on the F7"), []),
    erratum!("2.18.1", "bxCAN time-triggered communication mode not supported", [0x1000, 0x1001], None, NotReached("no CAN driver"), []),
    erratum!("2.19.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with accesses to certain OTG_FS registers", [0x1000, 0x1001], Workaround, Workaround("rule of use: the deferred vector's service and every bus call never preempt one another, from one context or under one lock at the vector's priority, so no OTG_FS access cuts into a FIFO write (usb/mod.rs, USB-002)"), []),
    erratum!("2.19.2", "Host packet transmission may hang when connecting through a hub to a low-speed device", [0x1000, 0x1001], None, Avoided("device mode only"), []),
    erratum!("2.20.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with accesses to certain OTG_HS registers", [0x1000, 0x1001], Workaround, NotReached("OTG_HS is not offered: only the full-speed port"), []),
    erratum!("2.20.2", "Host packet transmission may hang when connecting the full speed interface through a hub to a low-speed device", [0x1000, 0x1001], None, NotReached("OTG_HS is not offered: only the full-speed port"), []),
    erratum!("2.21.1", "Incorrect L3 checksum is inserted in transmitted IPv6 packets without TCP, UDP or ICMP payloads", [0x1000, 0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.21.2", "The ethernet MAC processes invalid extension headers in the received IPv6 frames", [0x1000, 0x1001], None, NotReached("no Ethernet driver"), []),
    erratum!("2.21.3", "MAC stuck in the idle state on receiving the TxFIFO flush command exactly one clock cycle after a transmission completes", [0x1000, 0x1001], Partial, NotReached("no Ethernet driver"), []),
    erratum!("2.21.4", "Transmit frame data corruption", [0x1000, 0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.21.5", "Incorrect status and corrupted frames when RxFIFO overflow occurs on the penultimate word of Rx frames", [0x1000, 0x1001], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.21.6", "Ethernet erroneous data received in RMII configuration", [0x1000], Workaround, NotReached("no Ethernet driver"), []),
];
