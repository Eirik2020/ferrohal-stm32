//! ES0299 Rev 6: the errata of die 0x433 (STM32F401xD/xE, RM0368), one entry
//! per row of the sheet's summary of device limitations, transcribed on
//! 2026-11-15 (ADR-010). Hand facts: edit an entry's handling as it is reviewed.
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0299",
    revision: 6,
    revisions: &[(0x1000, "A"), (0x1001, "1, Z")],
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
    erratum!("2.1.1", "Interrupted loads to SP can cause erroneous behavior", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.1.2", "VDIV or VSQRT instructions might not complete correctly when very short ISRs are used", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.1.3", "Store immediate overlapping exception return operation might vector to incorrect interrupt", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.1", "Debugging Stop mode and SysTick timer", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.2", "Debugging Stop mode with WFE entry", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.3", "Debugging Sleep/Stop mode with WFE/WFI entry", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.4", "Wake-up sequence from Standby mode when using more than one wake-up source", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.5", "Full JTAG configuration without NJTRST pin cannot be used", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.6", "MPU attribute to RTC and IWDG registers incorrectly managed", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.7", "Delay after an RCC peripheral clock enabling", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.8", "Internal noise impacting the ADC accuracy", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.9", "Possible delay in backup domain protection disabling/enabling after programming the DBP bit", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.10", "PC13 signal transitions disturb LSE", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.2.11", "In some specific cases, DMA2 data corruption occurs when managing AHB and APB2", [0x1000, 0x1001], Workaround, Firmware("keep APB2 traffic off DMA2 while a sampler reads GPIO"), []),
    erratum!("2.2.12", "Corrupted content of the backup domain due to a missed power-on reset after this domain", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.13", "PB5 I/O VIN limitation", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.14", "PA0 I/O VIN limitation in Standby mode", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.2.15", "PH1 cannot be used as a GPIO in HSE bypass mode", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.3.1", "Wrong data written during SDIO hardware flow control", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.3.2", "Wrong CCRCFAIL status after a response without CRC is received", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.3.3", "Data corruption in SDIO clock dephasing (NEGEDGE) mode", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.3.4", "CE-ATA multiple write command and card busy signal management", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.3.5", "No underrun detection with wrong data transmission", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.4.1", "ADC sequencer modification during conversion", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.5.1", "DMA request not automatically cleared by clearing DMAEN", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.5.2", "DMA underrun flag not set when an internal trigger is detected on the clock cycle of the DMA", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.6.1", "PWM re-enabled in automatic output enable mode despite of system break", [0x1000, 0x1001], Partial, Unreviewed, []),
    erratum!("2.6.3", "Consecutive compare event missed in specific conditions", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.6.4", "Output compare clear not working with external counter reset", [0x1000, 0x1001], Partial, Unreviewed, []),
    erratum!("2.7.1", "RVU flag not reset in Stop", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.7.2", "PVU flag not reset in Stop", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.7.3", "RVU flag not cleared at low APB clock frequency", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.7.4", "PVU flag not cleared at low APB clock frequency", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.8.1", "Spurious tamper detection when disabling the tamper channel", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.8.2", "RTC calendar registers are not locked properly", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.8.3", "RTC interrupt can be masked by another RTC interrupt", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.8.4", "Calendar initialization may fail in case of consecutive INIT mode entry", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.8.5", "Alarm flag may be repeatedly set when the core is stopped in debug", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.8.6", "Detection of a tamper event occurring before enabling the tamper detection is not supported in", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.1", "Spurious bus error detection in controller mode", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.2", "SMBus standard not fully supported", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.3", "Start cannot be generated after a misplaced Stop", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.4", "Mismatch on the “Setup time for a repeated Start condition” timing parameter", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.5", "Data valid time (tVD;DAT) violated without the OVR flag being set", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.9.6", "Both SDA and SCL maximum rise times (tr) violated when the VDD_I2C bus voltage is higher", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.10.1", "Idle frame is not detected if the receiver clock speed is deviated", [0x1000, 0x1001], None, Firmware("IDLE can be missed when the sender runs faster: a read ending at idle needs a timeout fallback"), []),
    erratum!("2.10.2", "In full-duplex mode, the Parity Error (PE) flag can be cleared by writing to the data register", [0x1000, 0x1001], Workaround, Firmware("parity is reliable only while the receiving USART is not transmitting"), []),
    erratum!("2.10.3", "Parity Error (PE) flag is not set when receiving in Mute mode using address mark detection", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.10.4", "Break frame is transmitted regardless of CTS input line status", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.10.5", "RTS signal abnormally driven low after a protocol violation", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.10.6", "Start bit detected too soon when sampling for NACK signal from the smartcard", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.10.7", "Break request can prevent the transmission complete flag (TC) from being set", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.10.8", "Guard time not respected when data are sent on TXE events", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.10.9", "RTS is active while RE or UE = 0", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.11.1", "BSY bit may stay high when SPI is disabled", [0x1000, 0x1001], Workaround, Avoided("SPE drops only in configure, with the bus idle"), []),
    erratum!("2.11.2", "Anticipated communication upon SPI transit from slave receiver to master", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.11.3", "Wrong CRC calculation when the polynomial is even", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.11.4", "Corrupted last bit of data and/or CRC received in Master mode with delayed SCK feedback", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.11.5", "BSY flag may stay high at the end of a data transfer in Slave mode", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.11.6", "In I2S Slave mode, the WS level must be set by the external master when enabling the I2S", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.12.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.12.2", "Host packet transmission may hang when connecting through a hub to a low-speed device", [0x1000, 0x1001], None, Unreviewed, []),
    erratum!("2.12.3", "Data in RxFIFO is overwritten when all channels are disabled simultaneously", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.12.4", "OTG host blocks the receive channel when receiving IN packets and no TxFIFO is configured", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.12.5", "Host channel-halted interrupt not generated when the channel is disabled", [0x1000, 0x1001], Workaround, Unreviewed, []),
    erratum!("2.12.6", "Wrong software-read OTG_FS_DCFG register values", [0x1000, 0x1001], Workaround, Unreviewed, []),
];
