//! ES0305 Rev 15: the errata of die 0x441 (STM32F412xE/xG, RM0402), one
//! entry per row of the sheet's summary of device limitations, transcribed on
//! 2026-11-17 (ADR-010). Hand facts: edit an entry's handling as it is
//! reviewed.
//!
//! The verdicts for the rows other F4 sheets share are carried over from
//! them: the same IP versions, the same driver code. The rows it adds are for
//! peripherals FerroHAL has no driver for (FMC, QUADSPI, SDIO, FMPI2C).
//! Revisions 1 and C read the same code, 0x3000 (ES0305 Table 2).
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0305",
    revision: 15,
    revisions: &[(0x1001, "Z"), (0x2000, "B"), (0x3000, "1, C")],
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

/// Every erratum of the sheet, in its order: one row each, as the sheet has them.
#[rustfmt::skip]
pub const ERRATA: &[Erratum] = &[
    erratum!("2.1.1", "Interrupted loads to SP can cause erroneous behavior", [0x1001, 0x2000, 0x3000], Workaround, Avoided("FerroHAL loads SP nowhere, and compilers never emit LDR SP"), []),
    erratum!("2.1.2", "VDIV or VSQRT instructions might not complete correctly when very short ISRs are used", [0x1001, 0x2000, 0x3000], Workaround, Firmware("with the FPU on, clear FPCCR.LSPEN or give every ISR more than two instructions; FerroHAL defines no ISR"), []),
    erratum!("2.1.3", "Store immediate overlapping exception return operation might vector to incorrect interrupt", [0x1001, 0x2000, 0x3000], Workaround, Firmware("a DSB before an ISR returns, or ACTLR.DISDEFWBUF without the MPU; FerroHAL defines no ISR"), []),
    erratum!("2.2.1", "Debugging Stop mode and SysTick timer", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.2", "Debugging Stop mode with WFE entry", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.3", "Debugging Sleep/Stop mode with WFE/WFI entry", [0x1001, 0x2000, 0x3000], Workaround, Firmware("revision A runs without prefetch (2.2.1), so with DBG_SLEEP or DBG_STOP set put three NOPs after WFI or WFE; FerroHAL enters no Sleep or Stop"), []),
    erratum!("2.2.4", "Wake-up sequence from Standby mode when using more than one wake-up source", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no Standby entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.5", "Full JTAG configuration without NJTRST pin cannot be used", [0x1001, 0x2000, 0x3000], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.2.6", "MPU attribute to RTC and IWDG registers incorrectly managed", [0x1001, 0x2000, 0x3000], Workaround, Firmware("FerroHAL sets no MPU region; with one marking IWDG non-bufferable at APB = AHB, read back after a write"), []),
    erratum!("2.2.7", "Delay after an RCC peripheral clock enabling", [0x1001, 0x2000, 0x3000], Workaround, Workaround("Rcc::enable reads the enable register back before the peripheral is touched"), []),
    erratum!("2.2.8", "Internal noise impacting the ADC accuracy", [0x1001, 0x2000, 0x3000], Workaround, Firmware("average or filter ADC samples (AN4073); freeze turns prefetch on, against the sheet's first measure"), []),
    erratum!("2.2.9", "Possible delay in backup domain protection disabling/enabling after programming the DBP bit", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no backup-domain driver: DBP is never set"), []),
    erratum!("2.2.10", "PC13 signal transitions disturb LSE", [0x1001, 0x2000, 0x3000], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.2.11", "In some specific cases, DMA2 data corruption occurs when managing AHB and APB2 peripherals in a concurrent way", [0x1001, 0x2000, 0x3000], Workaround, Firmware("keep APB2 traffic off DMA2 while a sampler reads GPIO"), []),
    erratum!("2.2.12", "Corrupted content of the backup domain due to a missed power-on reset after this domain supply voltage drop", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no backup-domain driver; the bootloader request lives in RAM"), []),
    erratum!("2.2.13", "Flash sector erase issue for sectors 5 to 11", [0x1001], Workaround, NotReached("FerroHAL never erases flash"), []),
    erratum!("2.3.1", "Dummy read cycles inserted when reading synchronous memories", [0x1001, 0x2000, 0x3000], None, NotReached("no FMC driver"), []),
    erratum!("2.3.2", "Wrong data read from a busy NAND memory", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.3", "Spurious clock stoppage with continuous clock feature enabled", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.4", "Data read might be corrupted when the write FIFO is disabled", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.4.1", "First nibble of data not written after dummy phase", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.2", "Wrong data from memory-mapped read after an indirect mode operation", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.3", "Memory-mapped read operations may fail when timeout counter is enabled", [0x1001, 0x2000, 0x3000], Partial, NotReached("no QUADSPI driver"), []),
    erratum!("2.4.4", "Memory-mapped access in indirect mode clearing QUADSPI_AR register", [0x1001, 0x2000, 0x3000], Partial, NotReached("no QUADSPI driver"), []),
    erratum!("2.5.1", "Wrong CCRCFAIL status after a response without CRC is received", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.5.2", "No underrun detection with wrong data transmission", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.6.1", "ADC sequencer modification during conversion", [0x1001, 0x2000, 0x3000], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.7.1", "DMA request not automatically cleared by clearing DMAEN", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.7.2", "DMA underrun flag not set when an internal trigger is detected on the clock cycle of the DMA request acknowledge", [0x1001, 0x2000, 0x3000], None, NotReached("no DAC driver"), []),
    erratum!("2.8.1", "PWM re-enabled in automatic output enable mode despite of system break", [0x1001, 0x2000, 0x3000], Partial, Avoided("no break input: AOE and BKE stay clear"), []),
    erratum!("2.8.3", "Consecutive compare event missed in specific conditions", [0x1001, 0x2000, 0x3000], None, Workaround("a lane refuses a duty within `Pwm::top_margin` of the period's end followed by 0 (PWM-009; the H753 measured it, 2026-11-17)"), []),
    erratum!("2.8.4", "Output compare clear not working with external counter reset", [0x1001, 0x2000, 0x3000], Partial, Avoided("no ocref_clr and no reset slave mode"), []),
    erratum!("2.9.1", "RVU flag not reset in Stop", [0x1001, 0x2000, 0x3000], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.9.2", "PVU flag not reset in Stop", [0x1001, 0x2000, 0x3000], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.9.3", "RVU flag not cleared at low APB clock frequency", [0x1001, 0x2000, 0x3000], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.9.4", "PVU flag not cleared at low APB clock frequency", [0x1001, 0x2000, 0x3000], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.10.1", "RTC calendar registers are not locked properly", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.10.2", "RTC interrupt can be masked by another RTC interrupt", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.10.3", "Calendar initialization may fail in case of consecutive INIT mode entry", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.10.4", "Alarm flag may be repeatedly set when the core is stopped in debug", [0x1001, 0x2000, 0x3000], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.12.1", "10-bit controller mode: new transfer cannot be launched if first part of the address is not acknowledged by the target", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.2", "Wrong data sampling when data setup time (tSU;DAT) is shorter than one FMPI2C kernel clock period", [0x1001, 0x2000, 0x3000], Partial, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.3", "Spurious bus error detection in controller mode", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.4", "Last-received byte loss in reload mode", [0x1001, 0x2000, 0x3000], Partial, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.5", "Spurious controller transfer upon own target address match", [0x1001, 0x2000, 0x3000], Partial, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.7", "OVR flag not set in underrun condition", [0x1001, 0x2000, 0x3000], None, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.8", "Transmission stalled after first byte transfer", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.12.9", "SDA held low upon SMBus timeout expiry in target mode", [0x1001, 0x2000, 0x3000], Workaround, NotReached("no FMPI2C driver: FMPI2C1 is not offered"), []),
    erratum!("2.11.1", "Spurious bus error detection in controller mode", [0x1001, 0x2000, 0x3000], Workaround, Workaround("BERR is cleared and counted, and the transfer carries on"), []),
    erratum!("2.11.2", "SMBus standard not fully supported", [0x1001, 0x2000, 0x3000], Workaround, Avoided("I2C mode only: no SMBus"), []),
    erratum!("2.11.3", "Start cannot be generated after a misplaced Stop", [0x1001, 0x2000, 0x3000], Workaround, Workaround("recover resets through SWRST and writes the timing back"), []),
    erratum!("2.11.4", "Mismatch on the “Setup time for a repeated Start condition” timing parameter", [0x1001, 0x2000, 0x3000], Workaround, Firmware("a repeated start in Standard mode wants 88 kHz at most, or Fast mode"), []),
    erratum!("2.11.5", "Data valid time (tVD;DAT) violated without the OVR flag being set", [0x1001, 0x2000, 0x3000], Workaround, Avoided("controller only: no target mode"), []),
    erratum!("2.11.6", "Both SDA and SCL maximum rise times (tr) violated when the VDD_I2C bus voltage is higher than ((VDD + 0.3) / 0.7) V", [0x1001, 0x2000, 0x3000], Workaround, Board("pull the bus up to at most (VDD + 0.3) / 0.7 V: 5.14 V at 3.3 V"), []),
    erratum!("2.13.1", "Idle frame is not detected if the receiver clock speed is deviated", [0x1001, 0x2000, 0x3000], None, Firmware("IDLE can be missed when the sender runs faster: a read ending at idle needs a timeout fallback"), []),
    erratum!("2.13.2", "In full-duplex mode, the Parity Error (PE) flag can be cleared by writing to the data register", [0x1001, 0x2000, 0x3000], Workaround, Firmware("parity is reliable only while the receiving USART is not transmitting"), []),
    erratum!("2.13.3", "Parity Error (PE) flag is not set when receiving in Mute mode using address mark detection", [0x1001, 0x2000, 0x3000], None, Avoided("no mute mode"), []),
    erratum!("2.13.4", "Break frame is transmitted regardless of CTS input line status", [0x1001, 0x2000, 0x3000], None, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.13.5", "RTS signal abnormally driven low after a protocol violation", [0x1001, 0x2000, 0x3000], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.13.6", "Start bit detected too soon when sampling for NACK signal from the smartcard", [0x1001, 0x2000, 0x3000], None, Avoided("no smartcard mode"), []),
    erratum!("2.13.7", "Break request can prevent the transmission complete flag (TC) from being set", [0x1001, 0x2000, 0x3000], Workaround, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.13.8", "Guard time not respected when data are sent on TXE events", [0x1001, 0x2000, 0x3000], Workaround, Avoided("no smartcard mode"), []),
    erratum!("2.13.9", "RTS is active while RE or UE = 0", [0x1001, 0x2000, 0x3000], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.14.1", "BSY bit may stay high when SPI is disabled", [0x1001, 0x2000, 0x3000], Workaround, Avoided("SPE drops only in configure, with the bus idle"), []),
    erratum!("2.14.2", "Anticipated communication upon SPI transit from slave receiver to master", [0x1001, 0x2000, 0x3000], Workaround, Avoided("master only: never a slave first"), []),
    erratum!("2.14.3", "I2S slave in PCM short pulse mode sensitive to timing between WS and CK", [0x1001, 0x2000, 0x3000], Partial, NotReached("no I2S driver"), []),
    erratum!("2.14.4", "Wrong CRC calculation when the polynomial is even", [0x1001, 0x2000, 0x3000], Workaround, Avoided("CRC off"), []),
    erratum!("2.14.5", "Corrupted last bit of data and/or CRC received in Master mode with delayed SCK feedback", [0x1001, 0x2000, 0x3000], Workaround, Firmware("SCK at high or very high speed with APB at 84 MHz or less; low allows 25 MHz, medium 75: APB2 at 100 MHz is above the limit, so a master receiving on SPI1, SPI4 or SPI5 wants PCLK2 at or below 84 MHz"), []),
    erratum!("2.14.6", "BSY flag may stay high at the end of a data transfer in Slave mode", [0x1001, 0x2000, 0x3000], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.15.1", "bxCAN time-triggered communication mode not supported", [0x1001, 0x2000, 0x3000], None, NotReached("no bxCAN driver"), []),
    erratum!("2.16.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with accesses to certain OTG_FS registers", [0x1001, 0x2000, 0x3000], Workaround, Firmware("every USB call from one context, as usb.rs asks, so no register access cuts into a FIFO write"), []),
    erratum!("2.16.2", "Host packet transmission may hang when connecting through a hub to a low-speed device", [0x1001, 0x2000, 0x3000], None, Avoided("device mode only"), []),
];
