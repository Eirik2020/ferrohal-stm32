//! ES0206 Rev 24: the errata of die 0x419 (STM32F427/437, STM32F429/439,
//! RM0090), one entry per row of the sheet's summary of device limitations,
//! transcribed on 2026-11-17 (ADR-010). Hand facts: edit an entry's handling
//! as it is reviewed.
//!
//! The verdicts for the rows ES0182 (die 0x413, the F405) shares are carried
//! over from it: the same IP versions, the same driver code. Revisions 4, 5
//! and B read the same code, 0x2003 (ES0206 Table 2).
//!
//! Requirements: GLUE-007, GLUE-009

use crate::errata::{Erratum, Handling::*, Sheet, SheetStatus};

pub const ERRATA_SHEET: Sheet = Sheet {
    name: "ES0206",
    revision: 24,
    revisions: &[
        (0x1000, "A"),
        (0x1003, "Y"),
        (0x1007, "1"),
        (0x2001, "3"),
        (0x2003, "4, 5, B"),
    ],
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

/// ES0206 2.2.12: revisions A and Y have no over-drive, so `freeze` refuses
/// HCLK above 168 MHz there.
#[rustfmt::skip]
pub const ES0206_2_2_12: Erratum = erratum!("2.2.12", "Over-drive and Under-drive modes unavailability", [0x1000, 0x1003], None, Workaround("on revisions A and Y, freeze refuses a tree that needs the over-drive (above 168 MHz) with ClockError::Hclk"), []);

/// Every erratum of the sheet, in its order: one row each, as the sheet has them.
#[rustfmt::skip]
pub const ERRATA: &[Erratum] = &[
    erratum!("2.1.1", "Interrupted loads to SP can cause erroneous behavior", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("FerroHAL loads SP nowhere, and compilers never emit LDR SP"), []),
    erratum!("2.1.2", "VDIV or VSQRT instructions might not complete correctly when very short ISRs are used", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("with the FPU on, clear FPCCR.LSPEN or give every ISR more than two instructions; FerroHAL defines no ISR"), []),
    erratum!("2.1.3", "Store immediate overlapping exception return operation might vector to incorrect interrupt", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("a DSB before an ISR returns, or ACTLR.DISDEFWBUF without the MPU; FerroHAL defines no ISR"), []),
    erratum!("2.2.1", "Debugging Stop mode and SysTick timer", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.2", "Debugging Stop mode with WFE entry", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Stop entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.3", "Debugging Sleep/Stop mode with WFE/WFI entry", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("freeze turns the prefetch queue on, which the erratum needs off; FerroHAL enters no Sleep or Stop"), []),
    erratum!("2.2.4", "Wake-up sequence from Standby mode when using more than one wake-up source", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Standby entry: FerroHAL has no low-power API"), []),
    erratum!("2.2.5", "Full JTAG configuration without NJTRST pin cannot be used", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("debug over SWD: 4-wire JTAG loses the probe when PB4 is a GPIO"), []),
    erratum!("2.2.6", "MPU attribute to RTC and IWDG registers incorrectly managed", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("FerroHAL sets no MPU region; with one marking IWDG non-bufferable at APB = AHB, read back after a write"), []),
    erratum!("2.2.7", "Delay after an RCC peripheral clock enabling", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Workaround("Rcc::enable reads the enable register back before the peripheral is touched"), []),
    erratum!("2.2.8", "Internal noise impacting the ADC accuracy", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("average or filter ADC samples (AN4073); freeze turns prefetch on, against the sheet's first measure"), []),
    erratum!("2.2.9", "Possible delay in backup domain protection disabling/enabling after programming the DBP bit", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no backup-domain driver: DBP is never set"), []),
    erratum!("2.2.10", "PC13 signal transitions disturb LSE", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.2.11", "Corrupted content of the backup domain due to a missed power-on reset after this domain supply voltage drop", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no backup-domain driver; the bootloader request lives in RAM"), []),
    ES0206_2_2_12,
    erratum!("2.2.13", "Operating voltage extension down to 1.7 V in the whole temperature range", [0x1000, 0x1003, 0x1007], None, Board("on the revisions the sheet marks (A, Y, 1; its text names A and Y), a 1.7 V supply holds only from 0 to 70 °C"), []),
    erratum!("2.2.14", "Data read from flash memory corrupted when PA12 used as GPIO or alternate function", [0x1000, 0x1003, 0x1007], Workaround, Firmware("on revisions A, Y and 1 of a 2 MiB part, or a 1 MiB part in dual-bank mode, leave PA12 unconnected and held low: no GPIO and no OTG_FS on it"), []),
    erratum!("2.2.15", "Data cache might be corrupted during flash memory read-while-write operation", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("FerroHAL never programs flash"), []),
    erratum!("2.3.1", "Dummy read cycles inserted when reading synchronous memories", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no FMC driver"), []),
    erratum!("2.3.2", "FMC synchronous mode and NWAIT signal disabled", [0x1000, 0x1003, 0x1007], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.3", "Read access to a non-initialized SDRAM bank", [0x1000, 0x1003, 0x1007], Partial, NotReached("no FMC driver"), []),
    erratum!("2.3.4", "Interruption of CPU burst read access to the end of an SDRAM row", [0x1000, 0x1003, 0x1007], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.5", "Corruption of data read by CPU from the FMC used as stack or heap", [0x1000], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.6", "FMC NOR/PSRAM controller: asynchronous read access on bank 2 to 4 returns wrong data when bank 1 is in synchronous mode (BURSTEN bit is set)", [0x1000, 0x1003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.7", "FMC dynamic and static bank switching", [0x1000, 0x1003, 0x1007], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.8", "FMC NOR/PSRAM controller write protocol violation", [0x1000, 0x1003, 0x1007], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.9", "Data corruption during burst read from FMC synchronous memory", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.11", "Missed burst write transaction on multiplexed PSRAM", [0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.12", "FMC NOR/PSRAM controller bank switch with different BUSTURN durations", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.13", "Wrong data read from a busy NAND memory", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.14", "Spurious clock stoppage with continuous clock feature enabled", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.3.15", "SDRAM bank address corruption upon an interruption of CPU burst read access", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no FMC driver"), []),
    erratum!("2.4.1", "Wrong data written during SDIO hardware flow control", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no SDIO driver"), []),
    erratum!("2.4.2", "Wrong CCRCFAIL status after a response without CRC is received", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.4.3", "Data corruption in SDIO clock dephasing (NEGEDGE) mode", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no SDIO driver"), []),
    erratum!("2.4.4", "CE-ATA multiple write command and card busy signal management", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.4.5", "No underrun detection with wrong data transmission", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no SDIO driver"), []),
    erratum!("2.5.1", "ADC sequencer modification during conversion", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("SQR is written only with no conversion running: a read waits for EOC, and a scan holds the ADC"), []),
    erratum!("2.6.1", "DMA request not automatically cleared by clearing DMAEN", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no DAC driver"), []),
    erratum!("2.6.2", "DMA underrun flag not set when an internal trigger is detected on the clock cycle of the DMA request acknowledge", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no DAC driver"), []),
    erratum!("2.7.1", "PWM re-enabled in automatic output enable mode despite of system break", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Partial, Avoided("no break input: AOE and BKE stay clear"), []),
    erratum!("2.7.3", "Consecutive compare event missed in specific conditions", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Workaround("a lane refuses a duty within `Pwm::top_margin` of the period's end followed by 0 (TIMER-PWM-009; the H753 measured it, 2026-11-17)"), []),
    erratum!("2.7.4", "Output compare clear not working with external counter reset", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Partial, Avoided("no ocref_clr and no reset slave mode"), []),
    erratum!("2.8.1", "RVU flag not reset in Stop", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.8.2", "PVU flag not reset in Stop", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("start returns once PVU and RVU are clear, and FerroHAL never enters Stop"), []),
    erratum!("2.8.3", "RVU flag not cleared at low APB clock frequency", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.8.4", "PVU flag not cleared at low APB clock frequency", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("keep PCLK1 above 64 kHz, twice the LSI; below it the flags never clear, and start reports Error::Update"), []),
    erratum!("2.9.1", "Spurious tamper detection when disabling the tamper channel", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.2", "RTC calendar registers are not locked properly", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.3", "RTC interrupt can be masked by another RTC interrupt", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.4", "Calendar initialization may fail in case of consecutive INIT mode entry", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.5", "Alarm flag may be repeatedly set when the core is stopped in debug", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no LSE or RTC driver"), []),
    erratum!("2.9.6", "Detection of a tamper event occurring before enabling the tamper detection is not supported in edge detection mode", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no LSE or RTC driver"), []),
    erratum!("2.10.1", "Spurious bus error detection in controller mode", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Workaround("BERR is cleared and counted, and the transfer carries on"), []),
    erratum!("2.10.2", "SMBus standard not fully supported", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("I2C mode only: no SMBus"), []),
    erratum!("2.10.3", "Start cannot be generated after a misplaced Stop", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Workaround("recover resets through SWRST and writes the timing back"), []),
    erratum!("2.10.4", "Mismatch on the “Setup time for a repeated Start condition” timing parameter", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("a repeated start in Standard mode wants 88 kHz at most, or Fast mode"), []),
    erratum!("2.10.5", "Data valid time (tVD;DAT) violated without the OVR flag being set", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("controller only: no target mode"), []),
    erratum!("2.10.6", "Both SDA and SCL maximum rise times (tr) violated when the VDD_I2C bus voltage is higher than ((VDD + 0.3) / 0.7) V", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Board("pull the bus up to at most (VDD + 0.3) / 0.7 V: 5.14 V at 3.3 V"), []),
    erratum!("2.11.1", "Idle frame is not detected if the receiver clock speed is deviated", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Firmware("IDLE can be missed when the sender runs faster: a read ending at idle needs a timeout fallback"), []),
    erratum!("2.11.2", "In full-duplex mode, the Parity Error (PE) flag can be cleared by writing to the data register", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("parity is reliable only while the receiving USART is not transmitting"), []),
    erratum!("2.11.3", "Parity Error (PE) flag is not set when receiving in Mute mode using address mark detection", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Avoided("no mute mode"), []),
    erratum!("2.11.4", "Break frame is transmitted regardless of CTS input line status", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.11.5", "RTS signal abnormally driven low after a protocol violation", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.11.6", "Start bit detected too soon when sampling for NACK signal from the smartcard", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Avoided("no smartcard mode"), []),
    erratum!("2.11.7", "Break request can prevent the transmission complete flag (TC) from being set", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("no CTS flow control: CTSE stays clear"), []),
    erratum!("2.11.8", "Guard time not respected when data are sent on TXE events", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("no smartcard mode"), []),
    erratum!("2.11.9", "RTS is active while RE or UE = 0", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("no RTS flow control: RTSE stays clear"), []),
    erratum!("2.12.1", "BSY bit may stay high when SPI is disabled", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("SPE drops only in configure, with the bus idle"), []),
    erratum!("2.12.2", "Anticipated communication upon SPI transit from slave receiver to master", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("master only: never a slave first"), []),
    erratum!("2.12.3", "Wrong CRC calculation when the polynomial is even", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("CRC off"), []),
    erratum!("2.12.4", "Corrupted last bit of data and/or CRC received in Master mode with delayed SCK feedback", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Firmware("SCK at high or very high speed allows APB at 84 MHz, medium 75, low 25: the over-drive's 90 MHz APB2 is above the limit, so a master receiving on SPI1, SPI4, SPI5 or SPI6 wants PCLK2 at or below 84 MHz"), []),
    erratum!("2.12.5", "BSY flag may stay high at the end of a data transfer in Slave mode", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("master only: no slave mode"), []),
    erratum!("2.12.6", "In I2S Slave mode, the WS level must be set by the external master when enabling the I2S", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no I2S driver"), []),
    erratum!("2.13.1", "bxCAN time-triggered communication mode not supported", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no bxCAN driver"), []),
    erratum!("2.14.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with accesses to certain OTG_FS registers", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Workaround("rule of use: the deferred vector's service and every bus call never preempt one another, from one context or under one lock at the vector's priority, so no OTG_FS access cuts into a FIFO write (usb/mod.rs, USB-002)"), []),
    erratum!("2.14.2", "Host packet transmission may hang when connecting through a hub to a low-speed device", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, Avoided("device mode only"), []),
    erratum!("2.14.3", "Data in RxFIFO is overwritten when all channels are disabled simultaneously", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("device mode only"), []),
    erratum!("2.14.4", "OTG host blocks the receive channel when receiving IN packets and no TxFIFO is configured", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("device mode only"), []),
    erratum!("2.14.5", "Host channel-halted interrupt not generated when the channel is disabled", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("device mode only"), []),
    erratum!("2.14.6", "Wrong software-read OTG_FS_DCFG register values", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, Avoided("synopsys-usb-otg never reads DAD or PFIVL back right after writing them"), []),
    erratum!("2.15.1", "Transmit data FIFO is corrupted when a write sequence to the FIFO is interrupted with accesses to certain OTG_HS registers", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("OTG_HS is not offered: only the full-speed port"), []),
    erratum!("2.15.2", "Host packet transmission may hang when connecting the full speed interface through a hub to a low-speed device", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("OTG_HS is not offered: only the full-speed port"), []),
    erratum!("2.16.1", "Incorrect L3 checksum is inserted in transmitted IPv6 packets without TCP, UDP or ICMP payloads", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.2", "The Ethernet MAC processes invalid extension headers in the received IPv6 frames", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], None, NotReached("no Ethernet driver"), []),
    erratum!("2.16.3", "MAC stuck in the idle state on receiving the TxFIFO flush command exactly one clock cycle after a transmission completes", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Partial, NotReached("no Ethernet driver"), []),
    erratum!("2.16.4", "Transmit frame data corruption", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.5", "Incorrect status and corrupted frames when RxFIFO overflow occurs on the penultimate word of Rx frames", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.6", "Successive write operations to the same register might not be fully taken into account", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.7", "Incorrect remote wakeup on global unicast packet", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Partial, NotReached("no Ethernet driver"), []),
    erratum!("2.16.8", "Overflow status bits of missed frame and buffer overflow counters are cleared without a read operation", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.9", "MAC may provide incorrect Rx status for the MAC control frames when receive checksum offload is enabled", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.10", "MAC may provide an inaccurate Rx status when receive checksum offload is enabled in cut- through mode", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Partial, NotReached("no Ethernet driver"), []),
    erratum!("2.16.11", "MAC may not drop received giant error frames", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
    erratum!("2.16.12", "Ethernet automatic pause frame functionality is not supported", [0x1000, 0x1003, 0x1007, 0x2001, 0x2003], Workaround, NotReached("no Ethernet driver"), []),
];
