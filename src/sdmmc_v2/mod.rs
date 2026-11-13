//! SD cards on `sdmmc_v2` (H7): a blocking, polled driver for high-capacity
//! cards at 3.3 V on a 4-bit bus, in default speed (up to 25 MHz).
//!
//! - **init**: `Sdmmc::new(&mut rcc, wiring, &config)` takes the instance and
//!   its pins; the routing traits reject a wrong combination at compile time.
//!   [`init_card`](Sdmmc::init_card) brings up whatever card is in the slot
//!   and can be called again after one is inserted. No card is
//!   [`Error::NoCard`], not a failure of the driver.
//! - **blocks**: 512 bytes each, addressed by block number, one at a time or
//!   many in one command, which a card writes far faster.
//!
//! Transfers go through the FIFO, so a buffer can be anywhere. The FIFO is
//! polled, with hardware flow control pausing the bus clock whenever it would
//! otherwise overrun, so interrupts can preempt a transfer at any point. Every
//! wait is bounded.
//!
//! Not offered: SDSC and pre-v2 cards (byte addressing), 1.8 V signalling and
//! the faster modes it needs, the internal DMA, eMMC, SDMMC2.
//!
//! Requirements: SDMMC-001, SDMMC-002, SDMMC-003, SDMMC-004, API-002

use stm32_metapac::sdmmc::{
    Sdmmc as Regs,
    regs::{Clkcr, Icr},
};

use crate::{
    rcc::{Rcc, RccBit},
    resource::Refused,
    time::Hertz,
};

pub const BLOCK_SIZE: usize = 512;
/// DBLOCKSIZE for 512 bytes.
const BLOCK_SIZE_CODE: u8 = 9;
/// What one half-full FIFO flag allows at a time: 8 words.
const FIFO_BURST_BYTES: usize = 32;

const IDENTIFICATION_HZ: u32 = 400_000;
const MAX_DEFAULT_SPEED_HZ: u32 = 25_000_000;
/// Data timeout, in bus clocks: 200 ms at 25 MHz, the SD limit for writes is
/// 250 ms, and the wait for busy below covers the rest.
const DATA_TIMEOUT_CLOCKS: u32 = 5_000_000;

/// Register polls for one command: far longer than any command takes, and the
/// peripheral times out a card that doesn't answer.
const COMMAND_SPINS: u32 = 1_000_000;
/// Polls of a transfer's status between two FIFO bursts. The hardware's data
/// timeout ends a silent transfer long before this does.
const DATA_SPINS: u32 = 40_000_000;
/// ACMD41 attempts: a card has a second to power up, each attempt about a
/// quarter of a millisecond at 400 kHz.
const POWER_UP_TRIES: u32 = 8_000;
/// CMD13 attempts while a card programs: each about 5 us at 25 MHz, far more
/// at 400 kHz.
const BUSY_TRIES: u32 = 400_000;

const CMD_GO_IDLE: u8 = 0;
const CMD_ALL_SEND_CID: u8 = 2;
const CMD_SEND_RELATIVE_ADDR: u8 = 3;
const CMD_SELECT: u8 = 7;
const CMD_SEND_IF_COND: u8 = 8;
const CMD_SEND_CSD: u8 = 9;
const CMD_STOP_TRANSMISSION: u8 = 12;
const CMD_SEND_STATUS: u8 = 13;
const CMD_SET_BLOCKLEN: u8 = 16;
const CMD_READ_SINGLE: u8 = 17;
const CMD_READ_MULTIPLE: u8 = 18;
const CMD_WRITE_SINGLE: u8 = 24;
const CMD_WRITE_MULTIPLE: u8 = 25;
const CMD_APP: u8 = 55;
const ACMD_SET_BUS_WIDTH: u8 = 6;
const ACMD_SEND_OP_COND: u8 = 41;

const IF_COND_PATTERN: u32 = 0x1AA;
/// ACMD41: high capacity supported, 3.2 to 3.4 V.
const OP_COND_ARGUMENT: u32 = (1 << 30) | (0b11 << 20);
const OCR_POWERED_UP: u32 = 1 << 31;
const OCR_HIGH_CAPACITY: u32 = 1 << 30;
const STATE_TRANSFER: u32 = 4;
const R1_ERRORS: u32 = 0xFDF9_8008;

/// Why an operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The clock isn't running or can't reach the bus speed asked for.
    Clock,
    /// Nothing answered: no card in the slot.
    NoCard,
    /// The card isn't a v2 high-capacity card at 3.3 V.
    Unsupported,
    /// `init_card` hasn't succeeded.
    NotReady,
    /// A block past the card's end, or an empty transfer.
    OutOfRange,
    /// A card didn't answer a command in time.
    CommandTimeout,
    /// A command's response failed its CRC.
    CommandCrc,
    /// The card's status reported an error.
    CardError(u32),
    /// A data block failed its CRC.
    DataCrc,
    /// The card didn't send or take data in time.
    DataTimeout,
    /// The receive FIFO overran, or the transmit FIFO ran out.
    Fifo,
    /// A wait of the driver's own ran out, without the peripheral saying why.
    Stuck,
}

/// The bus settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// The fastest SD clock allowed after identification, at most 25 MHz.
    pub frequency: Hertz,
}

impl Config {
    pub const fn new(frequency: Hertz) -> Self {
        Self { frequency }
    }
}

/// The controller's register block. Crate-internal: a public bound naming it
/// would hand metapac out.
pub(crate) trait Registers {
    fn regs() -> Regs;
}

/// An SDMMC instance. The kernel clock is PLL1Q, SDMMCSEL as reset leaves it.
#[allow(private_bounds)] // `Registers` is the crate-internal half.
pub trait Instance: crate::Sealed + Registers {
    const NAME: &'static str;
    const ENABLE: RccBit;
    /// Its kernel clock: PLL1's Q output, SDMMCSEL as reset leaves it.
    const KERNEL: crate::rcc::Kernel;
}

/// A pin, in the right alternate function, that carries `U`'s clock.
pub trait CkPin<U: Instance>: crate::Sealed {}
/// A pin that carries `U`'s command line.
pub trait CmdPin<U: Instance>: crate::Sealed {}
/// A pin that carries `U`'s data line 0.
pub trait D0Pin<U: Instance>: crate::Sealed {}
/// A pin that carries `U`'s data line 1.
pub trait D1Pin<U: Instance>: crate::Sealed {}
/// A pin that carries `U`'s data line 2.
pub trait D2Pin<U: Instance>: crate::Sealed {}
/// A pin that carries `U`'s data line 3.
pub trait D3Pin<U: Instance>: crate::Sealed {}

/// What the board file hands to `Sdmmc::new`: an instance and its six pins.
/// The command and data lines need pull-ups, internal or on the board.
pub struct Wiring<U, Ck, Cmd, D0, D1, D2, D3> {
    pub sdmmc: U,
    pub ck: Ck,
    pub cmd: Cmd,
    pub d0: D0,
    pub d1: D1,
    pub d2: D2,
    pub d3: D3,
}

/// What `init_card` found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Card {
    /// The card's identity: manufacturer, product, serial, in the SD format.
    pub cid: u128,
    pub blocks: u32,
    /// The bus clock in use.
    pub clock: Hertz,
}

/// The SDMMC divider for a clock at or below `target` from `kernel`: the clock
/// is `kernel` over 2 * divider, or `kernel` itself at divider 0.
const fn divider(kernel: u32, target: u32) -> Option<(u16, u32)> {
    if target == 0 {
        return None;
    }
    if kernel <= target {
        return Some((0, kernel));
    }
    let needed = kernel.div_ceil(target).div_ceil(2);
    if needed == 0 || needed > 1023 {
        return None;
    }
    Some((needed as u16, kernel / (needed * 2)))
}

/// The card's block count from a version 2 CSD (C_SIZE, bits 69 to 48), or
/// `None` for another structure version.
const fn blocks_from_csd(csd: u128) -> Option<u32> {
    if (csd >> 126) as u8 != 1 {
        return None;
    }
    let c_size = ((csd >> 48) & 0x3F_FFFF) as u64;
    let blocks = (c_size + 1) * 1024;
    Some(if blocks > u32::MAX as u64 {
        u32::MAX
    } else {
        blocks as u32
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Response {
    None,
    Short,
    /// R3: its CRC field isn't one, so a CRC failure means nothing.
    ShortNoCrc,
    Long,
}

/// The SDMMC, with or without a card brought up. It holds the board's
/// [`Wiring`], which [`free`](Self::free) hands back whole.
pub struct Sdmmc<W> {
    wiring: W,
    regs: Regs,
    kernel: Hertz,
    cpu: Hertz,
    target: Hertz,
    rca: u32,
    blocks: u32,
}

/// What [`Sdmmc::new`](Sdmmc::new) hands back when it refuses: the reason,
/// and the wiring it took.
pub type SdmmcNewRefused<U, Ck, Cmd, D0, D1, D2, D3> =
    Refused<Error, Wiring<U, Ck, Cmd, D0, D1, D2, D3>>;

impl<U, Ck, Cmd, D0, D1, D2, D3> Sdmmc<Wiring<U, Ck, Cmd, D0, D1, D2, D3>>
where
    U: Instance,
    Ck: CkPin<U>,
    Cmd: CmdPin<U>,
    D0: D0Pin<U>,
    D1: D1Pin<U>,
    D2: D2Pin<U>,
    D3: D3Pin<U>,
{
    /// Enables and resets the SDMMC and sets the identification clock. The
    /// card stays unpowered until [`init_card`](Self::init_card). Refuses
    /// with the wiring handed back: the clocks are checked before the
    /// peripheral is touched.
    /// The refusal's parts are the wiring, whose seven parameters are the
    /// board's statement of the route.
    #[allow(clippy::type_complexity)]
    pub fn new(
        rcc: &mut Rcc,
        wiring: Wiring<U, Ck, Cmd, D0, D1, D2, D3>,
        config: &Config,
    ) -> Result<Self, SdmmcNewRefused<U, Ck, Cmd, D0, D1, D2, D3>> {
        let clocks = rcc.clocks();
        let kernel = match U::KERNEL.frequency(&clocks) {
            Some(kernel) => kernel,
            None => return Err(Refused::new(Error::Clock, wiring)),
        };
        if config.frequency.0 > MAX_DEFAULT_SPEED_HZ
            || divider(kernel.0, IDENTIFICATION_HZ).is_none()
            || divider(kernel.0, config.frequency.0).is_none()
        {
            return Err(Refused::new(Error::Clock, wiring));
        }
        rcc.enable(U::ENABLE);
        rcc.reset(U::ENABLE);
        Ok(Self {
            wiring,
            regs: U::regs(),
            kernel,
            cpu: clocks.cpu,
            target: config.frequency,
            rca: 0,
            blocks: 0,
        })
    }

    /// Powers the card off, clears every flag and hands the wiring back, the
    /// instance and its six pins, exactly as [`Sdmmc::new`] took them: what a
    /// `Drop` would do, made explicit so the parts can move out. A transfer is
    /// never in flight here, since every transfer runs to its end inside one
    /// call. A reopen runs `Sdmmc::new` on the same wiring, which resets the
    /// controller, then [`init_card`](Self::init_card).
    pub fn free(self) -> Wiring<U, Ck, Cmd, D0, D1, D2, D3> {
        self.regs.power().write(|w| w.set_pwrctrl(0));
        self.clear_flags();
        self.wiring
    }
}

impl<W> Sdmmc<W> {
    fn set_clock(&self, target: u32, wide: bool) -> Result<u32, Error> {
        let (code, clock) = divider(self.kernel.0, target).ok_or(Error::Clock)?;
        self.wait_idle()?;
        let mut clkcr = Clkcr(0);
        clkcr.set_clkdiv(code);
        clkcr.set_widbus(u8::from(wide));
        clkcr.set_hwfc_en(true);
        self.regs.clkcr().write_value(clkcr);
        Ok(clock)
    }

    fn wait_idle(&self) -> Result<(), Error> {
        for _ in 0..COMMAND_SPINS {
            let star = self.regs.star().read();
            if !star.cpsmact() && !star.dpsmact() {
                return Ok(());
            }
        }
        Err(Error::Stuck)
    }

    fn clear_flags(&self) {
        // Every static flag; the write-one bits that aren't flags are reserved
        // as zero.
        self.regs.icr().write_value(Icr(0x1FE0_0FFF));
    }

    fn command(&self, index: u8, argument: u32, response: Response) -> Result<(), Error> {
        self.clear_flags();
        for _ in 0..COMMAND_SPINS {
            if !self.regs.star().read().cpsmact() {
                break;
            }
        }
        self.regs.argr().write(|w| w.set_cmdarg(argument));
        self.regs.cmdr().write(|w| {
            w.set_cmdindex(index);
            w.set_cmdstop(index == CMD_STOP_TRANSMISSION);
            w.set_waitresp(match response {
                Response::None => 0,
                Response::Short | Response::ShortNoCrc => 1,
                Response::Long => 3,
            });
            w.set_cpsmen(true);
        });
        for _ in 0..COMMAND_SPINS {
            let star = self.regs.star().read();
            if star.ctimeout() {
                return Err(Error::CommandTimeout);
            }
            if star.ccrcfail() {
                return if response == Response::ShortNoCrc {
                    Ok(())
                } else {
                    Err(Error::CommandCrc)
                };
            }
            if response == Response::None {
                if star.cmdsent() {
                    return Ok(());
                }
            } else if star.cmdrend() {
                return Ok(());
            }
        }
        Err(Error::Stuck)
    }

    fn short(&self) -> u32 {
        self.regs.respr(0).read().cardstatus()
    }

    fn long(&self) -> u128 {
        let first = u128::from(self.regs.respr(0).read().cardstatus());
        let second = u128::from(self.regs.respr(1).read().cardstatus());
        let third = u128::from(self.regs.respr(2).read().cardstatus());
        let fourth = u128::from(self.regs.respr(3).read().cardstatus());
        (first << 96) | (second << 64) | (third << 32) | fourth
    }

    /// A command with an R1 response, whose error bits count.
    fn r1(&self, index: u8, argument: u32) -> Result<u32, Error> {
        self.command(index, argument, Response::Short)?;
        let status = self.short();
        if status & R1_ERRORS != 0 {
            return Err(Error::CardError(status));
        }
        Ok(status)
    }

    fn app_command(&self, index: u8, argument: u32, response: Response) -> Result<(), Error> {
        self.command(CMD_APP, self.rca << 16, Response::Short)?;
        self.command(index, argument, response)
    }

    /// Powers the card up and brings it to 4-bit transfer mode at the
    /// configured speed. Whatever card was up before is forgotten.
    pub fn init_card(&mut self) -> Result<Card, Error> {
        self.rca = 0;
        self.blocks = 0;
        self.regs.power().write(|w| w.set_pwrctrl(0));
        self.set_clock(IDENTIFICATION_HZ, false)?;
        self.regs.power().write(|w| w.set_pwrctrl(3));
        // The card wants 74 clocks and a millisecond before its first command.
        cortex_m::asm::delay(self.cpu.0 / 1000 * 2);

        self.command(CMD_GO_IDLE, 0, Response::None)?;
        match self.command(CMD_SEND_IF_COND, IF_COND_PATTERN, Response::Short) {
            Ok(()) => {}
            Err(Error::CommandTimeout) => return Err(Error::NoCard),
            Err(error) => return Err(error),
        }
        if self.short() & 0xFFF != IF_COND_PATTERN {
            return Err(Error::Unsupported);
        }

        let mut ocr = 0;
        for _ in 0..POWER_UP_TRIES {
            self.app_command(ACMD_SEND_OP_COND, OP_COND_ARGUMENT, Response::ShortNoCrc)?;
            ocr = self.short();
            if ocr & OCR_POWERED_UP != 0 {
                break;
            }
        }
        if ocr & OCR_POWERED_UP == 0 {
            return Err(Error::CommandTimeout);
        }
        if ocr & OCR_HIGH_CAPACITY == 0 {
            return Err(Error::Unsupported);
        }

        self.command(CMD_ALL_SEND_CID, 0, Response::Long)?;
        let cid = self.long();
        self.command(CMD_SEND_RELATIVE_ADDR, 0, Response::Short)?;
        let rca = self.short() >> 16;
        self.command(CMD_SEND_CSD, rca << 16, Response::Long)?;
        let blocks = blocks_from_csd(self.long()).ok_or(Error::Unsupported)?;
        self.r1(CMD_SELECT, rca << 16)?;
        self.rca = rca;

        // Every high-capacity card has the 4-bit bus.
        self.app_command(ACMD_SET_BUS_WIDTH, 2, Response::Short)?;
        let clock = self.set_clock(self.target.0, true)?;
        self.r1(CMD_SET_BLOCKLEN, BLOCK_SIZE as u32)?;
        self.blocks = blocks;
        Ok(Card {
            cid,
            blocks,
            clock: Hertz(clock),
        })
    }

    /// The card's size in blocks; 0 until `init_card` has succeeded.
    pub const fn block_count(&self) -> u32 {
        self.blocks
    }

    fn check_range(&self, lba: u32, count: usize) -> Result<(), Error> {
        if self.blocks == 0 {
            return Err(Error::NotReady);
        }
        if count == 0
            || count as u64 > u64::from(self.blocks)
            || u64::from(lba) + count as u64 > u64::from(self.blocks)
        {
            return Err(Error::OutOfRange);
        }
        Ok(())
    }

    /// Starts the data path. The card's status command or CMD16 between two
    /// transfers keeps them the eight bus clocks apart ES0392 2.8.4 asks for.
    fn start_data(&self, blocks: usize, to_host: bool) -> Result<(), Error> {
        self.wait_idle()?;
        self.regs
            .dtimer()
            .write(|w| w.set_datatime(DATA_TIMEOUT_CLOCKS));
        self.regs
            .dlenr()
            .write(|w| w.set_datalength((blocks * BLOCK_SIZE) as u32));
        self.regs.dctrl().write(|w| {
            w.set_dblocksize(BLOCK_SIZE_CODE);
            w.set_dtdir(to_host);
            w.set_dten(true);
        });
        Ok(())
    }

    fn data_error(&self) -> Option<Error> {
        let star = self.regs.star().read();
        if star.dcrcfail() {
            Some(Error::DataCrc)
        } else if star.dtimeout() {
            Some(Error::DataTimeout)
        } else if star.rxoverr() || star.txunderr() {
            Some(Error::Fifo)
        } else {
            None
        }
    }

    /// Waits for the end of the data: a CRC error on the last block shows up
    /// only after its last byte is in.
    fn wait_data_end(&self) -> Result<(), Error> {
        for _ in 0..DATA_SPINS {
            if let Some(error) = self.data_error() {
                return Err(error);
            }
            if self.regs.star().read().dataend() {
                return Ok(());
            }
        }
        Err(Error::Stuck)
    }

    /// Waits for `ready` on the status register, or a data error.
    fn wait_fifo(
        &self,
        ready: impl Fn(stm32_metapac::sdmmc::regs::Star) -> bool,
    ) -> Result<(), Error> {
        for _ in 0..DATA_SPINS {
            if let Some(error) = self.data_error() {
                return Err(error);
            }
            if ready(self.regs.star().read()) {
                return Ok(());
            }
        }
        Err(Error::Stuck)
    }

    fn receive(&self, blocks: &mut [[u8; BLOCK_SIZE]]) -> Result<(), Error> {
        for block in blocks {
            let (bursts, _) = block.as_chunks_mut::<FIFO_BURST_BYTES>();
            for burst in bursts {
                self.wait_fifo(|star| star.rxfifohf())?;
                let (words, _) = burst.as_chunks_mut::<4>();
                for word in words {
                    *word = self.regs.fifor().read().fifodata().to_le_bytes();
                }
            }
        }
        self.wait_data_end()
    }

    fn send(&self, blocks: &[[u8; BLOCK_SIZE]]) -> Result<(), Error> {
        for block in blocks {
            let (bursts, _) = block.as_chunks::<FIFO_BURST_BYTES>();
            for burst in bursts {
                self.wait_fifo(|star| star.txfifohe())?;
                let (words, _) = burst.as_chunks::<4>();
                for word in words {
                    let value = u32::from_le_bytes(*word);
                    self.regs.fifor().write(|w| w.set_fifodata(value));
                }
            }
        }
        self.wait_data_end()
    }

    /// Waits for the card to finish programming, back in its transfer state.
    fn wait_ready(&self) -> Result<(), Error> {
        for _ in 0..BUSY_TRIES {
            let status = self.r1(CMD_SEND_STATUS, self.rca << 16)?;
            if (status >> 9) & 0xF == STATE_TRANSFER {
                return Ok(());
            }
        }
        Err(Error::DataTimeout)
    }

    /// Reads one block.
    pub fn read_block(&mut self, lba: u32, block: &mut [u8; BLOCK_SIZE]) -> Result<(), Error> {
        self.read_blocks(lba, core::slice::from_mut(block))
    }

    /// Writes one block; returns when the card has finished with it.
    pub fn write_block(&mut self, lba: u32, block: &[u8; BLOCK_SIZE]) -> Result<(), Error> {
        self.write_blocks(lba, core::slice::from_ref(block))
    }

    /// Reads consecutive blocks from `lba` in one command.
    pub fn read_blocks(&mut self, lba: u32, blocks: &mut [[u8; BLOCK_SIZE]]) -> Result<(), Error> {
        self.check_range(lba, blocks.len())?;
        let many = blocks.len() > 1;
        self.r1(CMD_SET_BLOCKLEN, BLOCK_SIZE as u32)?;
        self.start_data(blocks.len(), true)?;
        let index = if many {
            CMD_READ_MULTIPLE
        } else {
            CMD_READ_SINGLE
        };
        let started = self.r1(index, lba);
        let data = started.and_then(|_| self.receive(blocks));
        let stopped = if many {
            self.command(CMD_STOP_TRANSMISSION, 0, Response::Short)
        } else {
            Ok(())
        };
        let flags_clear = self.abort_data_on(&data);
        data?;
        stopped?;
        flags_clear
    }

    /// Writes consecutive blocks from `lba` in one command; returns when the
    /// card has finished with them.
    pub fn write_blocks(&mut self, lba: u32, blocks: &[[u8; BLOCK_SIZE]]) -> Result<(), Error> {
        self.check_range(lba, blocks.len())?;
        let many = blocks.len() > 1;
        self.r1(CMD_SET_BLOCKLEN, BLOCK_SIZE as u32)?;
        self.start_data(blocks.len(), false)?;
        let index = if many {
            CMD_WRITE_MULTIPLE
        } else {
            CMD_WRITE_SINGLE
        };
        let started = self.r1(index, lba);
        let data = started.and_then(|_| self.send(blocks));
        let stopped = if many {
            self.command(CMD_STOP_TRANSMISSION, 0, Response::Short)
        } else {
            Ok(())
        };
        let flags_clear = self.abort_data_on(&data);
        data?;
        stopped?;
        flags_clear?;
        self.wait_ready()
    }

    /// After a transfer: on a failure, stops the data path where it stands.
    fn abort_data_on(&self, data: &Result<(), Error>) -> Result<(), Error> {
        if data.is_err() {
            self.regs.dctrl().write(|w| {
                w.set_dten(false);
                w.set_fiforst(true);
            });
            self.regs.dctrl().write(|w| w.set_fiforst(false));
        }
        self.clear_flags();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divider_stays_at_or_below_the_target() {
        // The Lucid's PLL1Q.
        assert_eq!(divider(100_000_000, 25_000_000), Some((2, 25_000_000)));
        assert_eq!(divider(100_000_000, 400_000), Some((125, 400_000)));
        assert_eq!(divider(100_000_000, 24_000_000), Some((3, 16_666_666)));
        assert_eq!(divider(25_000_000, 25_000_000), Some((0, 25_000_000)));
        assert_eq!(divider(100_000_000, 0), None);
        assert_eq!(divider(4_000_000_000, 400_000), None);
    }

    #[test]
    fn csd_gives_the_size() {
        // A 32 GB card: C_SIZE 60 719, version 2.
        let csd = (1u128 << 126) | (60_719u128 << 48);
        assert_eq!(blocks_from_csd(csd), Some(62_177_280));
        // Version 1 isn't offered.
        assert_eq!(blocks_from_csd(60_719u128 << 48), None);
    }
}
