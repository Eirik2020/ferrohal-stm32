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
    erratum!("2.1.1", "Interrupted loads to SP can cause erroneous behavior", [0x1000, 0x1001], Workaround, Avoided("FerroHAL loads SP nowhere, and compilers never emit LDR SP"), []),
    erratum!("2.1.2", "VDIV or VSQRT instructions might not complete correctly when very short ISRs are used", [0x1000, 0x1001], Workaround, Firmware("with the FPU on, clear FPCCR.LSPEN or give every ISR more than two instructions; FerroHAL defines no ISR"), []),
    erratum!("2.1.3", "Store immediate overlapping exception return operation might vector to incorrect interrupt", [0x1000, 0x1001], Workaround, Firmware("a DSB before an ISR returns, or ACTLR.DISDEFWBUF without the MPU; FerroHAL defines no ISR"), []),
    erratum!("2.2.1", "Debugging Stop mode and SysTick timer", [0x1000, 0x1001], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.2", "Debugging Stop mode with WFE entry", [0x1000, 0x1001], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.3", "Debugging Sleep/Stop mode with WFE/WFI entry", [0x1000, 0x1001], Workaround, Avoided("freeze turns the prefetch queue on, which the erratum needs off; FerroHAL enters no Sleep or Stop"), []),
    erratum!("2.2.4", "Wake-up sequence from Standby mode when using more than one wake-up source", [0x1000, 0x1001], Workaround, NotReached("no Standby entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.5", "Full JTAG configuration without NJTRST pin cannot be used", [0x1000, 0x1001], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.2.6", "MPU attribute to RTC and IWDG registers incorrectly managed", [0x1000, 0x1001], Workaround, Firmware("FerroHAL sets no MPU region; with one marking IWDG non-bufferable at APB = AHB, read back after a write"), []),
    erratum!("2.2.7", "Delay after an RCC peripheral clock enabling", [0x1000, 0x1001], Workaround, Workaround("Rcc::enable reads the enable register back before the peripheral is touched"), []),
    erratum!("2.2.8", "Internal noise impacting the ADC accuracy", [0x1000, 0x1001], Workaround, Firmware("average or filter ADC samples (AN4073); freeze turns prefetch on, against the sheet's first measure"), []),
    erratum!("2.2.9", "Possible delay in backup domain protection disabling/enabling after programming the DBP bit", [0x1000, 0x1001], Workaround, NotReached("no backup-domain driver: DBP is never set"), []),
    erratum!("2.2.10", "PC13 signal transitions disturb LSE", [0x1000, 0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.2.11", "In some specific cases, DMA2 data corruption occurs when managing AHB and APB2", [0x1000, 0x1001], Workaround, Firmware("keep APB2 traffic off DMA2 while a sampler reads GPIO"), []),
    erratum!("2.2.12", "Corrupted content of the backup domain due to a missed power-on reset after this domain", [0x1000, 0x1001], Workaround, NotReached("no backup-domain driver; the bootloader request lives in RAM"), []),
    erratum!("2.2.13", "PB5 I/O VIN limitation", [0x1000, 0x1001], Workaround, Board("keep PB5 at or below VDD"), []),
    erratum!("2.2.14", "PA0 I/O VIN limitation in Standby mode", [0x1000, 0x1001], Workaround, Board("in Standby, keep PA0 at or below VDD"), []),
    erratum!("2.2.15", "PH1 cannot be used as a GPIO in HSE bypass mode", [0x1000, 0x1001], None, Board("in HSE bypass, PH1 is not a GPIO"), []),
    erratum!("2.3.1", "Wrong data written during SDIO hardware flow control", [0x1000, 0x1001], None, NotReached("no SDIO driver"), []),
    erratum!("2.3.2", "Wrong CCRCFAIL status after a response without CRC is received", [0x1000, 0x1001], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.3.3", "Data corruption in SDIO clock dephasing (NEGEDGE) mode", [0x1000, 0x1001], None, NotReached("no SDIO driver"), []),
    erratum!("2.3.4", "CE-ATA multiple write command and card busy signal management", [0x1000, 0x1001], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.3.5", "No underrun detection with wrong data transmission", [0x1000, 0x1001], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.4.1", "ADC sequencer modification during conversion", [0x1000, 0x1001], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.5.1", "DMA request not automatically cleared by clearing DMAEN", [0x1000, 0x1001], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.5.2", "DMA underrun flag not set when an internal trigger is detected on the clock cycle of the DMA", [0x1000, 0x1001], None, NotReached("no DAC driver"), []),
    erratum!("2.6.1", "PWM re-enabled in automatic output enable mode despite of system break", [0x1000, 0x1001], Partial, Avoided("no break input: AOE and BKE stay clear"), []),
    erratum!("2.6.3", "Consecutive compare event missed in specific conditions", [0x1000, 0x1001], None, Open("a compare lane may lose a DMA request when its duty steps from the period's top to 0"), []),
    erratum!("2.6.4", "Output compare clear not working with external counter reset", [0x1000, 0x1001], Partial, Avoided("no ocref_clr and no reset slave mode"), []),
    erratum!("2.7.1", "RVU flag not reset in Stop", [0x1000, 0x1001], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.7.2", "PVU flag not reset in Stop", [0x1000, 0x1001], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.7.3", "RVU flag not cleared at low APB clock frequency", [0x1000, 0x1001], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.7.4", "PVU flag not cleared at low APB clock frequency", [0x1000, 0x1001], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.8.1", "Spurious tamper detection when disabling the tamper channel", [0x1000, 0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.2", "RTC calendar registers are not locked properly", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.3", "RTC interrupt can be masked by another RTC interrupt", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.4", "Calendar initialization may fail in case of consecutive INIT mode entry", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.5", "Alarm flag may be repeatedly set when the core is stopped in debug", [0x1000, 0x1001], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.8.6", "Detection of a tamper event occurring before enabling the tamper detection is not supported in", [0x1000, 0x1001], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.1", "Spurious bus error detection in controller mode", [0x1000, 0x1001], Workaround, Open("the F4 I2C lacks two errata workarounds"), []),
    erratum!("2.9.2", "SMBus standard not fully supported", [0x1000, 0x1001], Workaround, Avoided("I2C mode only: no SMBus"), []),
    erratum!("2.9.3", "Start cannot be generated after a misplaced Stop", [0x1000, 0x1001], Workaround, Open("the F4 I2C lacks two errata workarounds"), []),
    erratum!("2.9.4", "Mismatch on the “Setup time for a repeated Start condition” timing parameter", [0x1000, 0x1001], Workaround, Firmware("a repeated start in Standard mode wants 88 kHz at most, or Fast mode"), []),
    erratum!("2.9.5", "Data valid time (tVD;DAT) violated without the OVR flag being set", [0x1000, 0x1001], Workaround, Avoided("controller only: no target mode"), []),
    erratum!("2.9.6", "Both SDA and SCL maximum rise times (tr) violated when the VDD_I2C bus voltage is higher", [0x1000, 0x1001], Workaround, Board("pull the bus up to at most (VDD + 0.3) / 0.7 V: 5.14 V at 3.3 V"), []),
    erratum!("2.10.1", "Idle frame is not detected if the receiver clock speed is deviated", [0x1000, 0x1001], None, Firmware("IDLE can be missed when the sender runs faster: a read ending at idle needs a timeout fallback"), []),
    erratum!("2.10.2", "In full-duplex mode, the Parity Error (PE) flag can be cleared by writing to the data register", [0x1000, 0x1001], Workaround, Firmware("parity is reliable only while the receiving USART is not transmitting"), []),
    erratum!("2.10.3", "Parity Error (PE) flag is not set when receiving in Mute mode using address mark detection", [0x1000, 0x1001], None, Avoided("no mute mode"), []),
    erratum!("2.10.4", "Break frame is transmitted regardless of CTS input line status", [0x1000, 0x1001], None, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.10.5", "RTS signal abnormally driven low after a protocol violation", [0x1000, 0x1001], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.10.6", "Start bit detected too soon when sampling for NACK signal from the smartcard", [0x1000, 0x1001], None, Avoided("no smartcard mode"), []),
    erratum!("2.10.7", "Break request can prevent the transmission complete flag (TC) from being set", [0x1000, 0x1001], Workaround, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.10.8", "Guard time not respected when data are sent on TXE events", [0x1000, 0x1001], Workaround, Avoided("no smartcard mode"), []),
    erratum!("2.10.9", "RTS is active while RE or UE = 0", [0x1000, 0x1001], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.11.1", "BSY bit may stay high when SPI is disabled", [0x1000, 0x1001], Workaround, Avoided("SPE drops only in configure, with the bus idle"), []),
    erratum!("2.11.2", "Anticipated communication upon SPI transit from slave receiver to master", [0x1000, 0x1001], Workaround, Avoided("master only: never a slave first"), []),
    erratum!("2.11.3", "Wrong CRC calculation when the polynomial is even", [0x1000, 0x1001], Workaround, Avoided("CRC off"), []),
    erratum!("2.11.4", "Corrupted last bit of data and/or CRC received in Master mode with delayed SCK feedback", [0x1000, 0x1001], Workaround, Firmware("SCK at high or very high speed with APB at 84 MHz; low allows 25 MHz, medium 75"), []),
    erratum!("2.11.5", "BSY flag may stay high at the end of a data transfer in Slave mode", [0x1000, 0x1001], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.11.6", "In I2S Slave mode, the WS level must be set by the external master when enabling the I2S", [0x1000, 0x1001], Workaround, NotReached("no I2S driver"), []),
    erratum!("2.12.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with", [0x1000, 0x1001], Workaround, Firmware("every USB call from one context, as usb.rs asks, so no register access cuts into a FIFO write"), []),
    erratum!("2.12.2", "Host packet transmission may hang when connecting through a hub to a low-speed device", [0x1000, 0x1001], None, Avoided("device mode only"), []),
    erratum!("2.12.3", "Data in RxFIFO is overwritten when all channels are disabled simultaneously", [0x1000, 0x1001], Workaround, Avoided("device mode only"), []),
    erratum!("2.12.4", "OTG host blocks the receive channel when receiving IN packets and no TxFIFO is configured", [0x1000, 0x1001], Workaround, Avoided("device mode only"), []),
    erratum!("2.12.5", "Host channel-halted interrupt not generated when the channel is disabled", [0x1000, 0x1001], Workaround, Avoided("device mode only"), []),
    erratum!("2.12.6", "Wrong software-read OTG_FS_DCFG register values", [0x1000, 0x1001], Workaround, Avoided("synopsys-usb-otg never reads DAD or PFIVL back right after writing them"), []),
];
