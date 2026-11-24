//! The SPI master without DMA: [`BlockingSpi`], for a bus whose DMA routes
//! are taken or not worth a stream, such as the Foxeer F405 V2's SPI NOR on
//! SPI2, whose only streams (DMA1 3 and 4) its UARTs use.
//!
//! One byte is in flight at a time: written, its reply waited for, read.
//! The bus pauses between bytes, which an SPI slave doesn't mind, and the
//! receiver can never overrun. Every transfer is bounded: twice its time on
//! the wire plus a margin, measured with a [`CycleClock`] when one is set,
//! otherwise counted as register polls of at least a cycle each.
//!
//! Requirements: SPI-POLL-001, SPI-POLL-002, API-002

use crate::{
    rcc::Rcc,
    resource::Refused,
    spi::{
        BlockingWiring, Config, ConfigError, Error, Info, Instance, MisoPin, MosiPin, Parts,
        SckPin, instance::SckClock,
    },
    time::{CycleClock, Hertz},
    wait::Deadline,
};

/// The most bytes one hardware transfer moves (the H7's TSIZE is 16 bits).
const CHUNK: usize = 65_535;

/// An SPI master moving bytes by polling its data register. It keeps the
/// board's wiring, the instance and its three pins, and
/// [`free`](BlockingSpi::free) hands it back, so the same parts open again.
pub struct BlockingSpi<W: Parts> {
    info: &'static Info,
    sck: SckClock,
    clock: Option<&'static dyn CycleClock>,
    name: &'static str,
    wiring: W,
}

impl<U, Sck, Miso, Mosi> BlockingSpi<BlockingWiring<U, Sck, Miso, Mosi>>
where
    U: Instance,
    Sck: SckPin<U>,
    Miso: MisoPin<U>,
    Mosi: MosiPin<U>,
{
    /// Enables and resets the SPI and configures it as a master. Chip select
    /// is the firmware's GPIO.
    ///
    /// A refusal hands back `wiring` with the reason. Every refusal comes
    /// before anything is written to hardware.
    ///
    /// On success the handle keeps the wiring; [`free`](Self::free) returns
    /// it.
    pub fn new(
        rcc: &mut Rcc,
        wiring: BlockingWiring<U, Sck, Miso, Mosi>,
        config: &Config,
    ) -> Result<Self, super::BlockingSpiRefused<U, Sck, Miso, Mosi>> {
        let info = U::info();
        let clocks = rcc.clocks();
        let Some(kernel_clock) = U::KERNEL.frequency(&clocks) else {
            return Err(Refused::new(ConfigError::KernelClock, wiring));
        };
        // Checked before the clock goes on, so every refusal comes before
        // anything is written to hardware.
        if let Err(error) = config.divider(kernel_clock) {
            return Err(Refused::new(error, wiring));
        }
        rcc.open(U::ENABLE);
        let sck = match SckClock::configure(info.unit(), kernel_clock, clocks.cpu, config) {
            Ok(sck) => sck,
            Err(error) => return Err(Refused::new(error, wiring)),
        };
        Ok(Self {
            info,
            sck,
            clock: None,
            name: U::NAME,
            wiring,
        })
    }

    /// Closes the SPI and hands back what [`new`](Self::new) took, the
    /// wiring: what a `Drop` would do, made explicit so the parts can move
    /// out. The SPI is idle between polled transfers, so SPE drops with the
    /// bus quiet. A reopen runs `BlockingSpi::new` again on the same parts.
    pub fn free(self) -> BlockingWiring<U, Sck, Miso, Mosi> {
        self.info.unit().disable();
        self.wiring
    }
}

impl<W: Parts> BlockingSpi<W> {
    /// Changes the bus settings. Returns the new SCK.
    pub fn set_config(&mut self, config: &Config) -> Result<Hertz, ConfigError> {
        self.sck.reconfigure(self.info.unit(), config)
    }

    /// The SCK in use.
    pub fn frequency(&self) -> Hertz {
        self.sck.frequency()
    }

    /// The kernel clock SCK is divided from.
    pub fn kernel_clock(&self) -> Hertz {
        self.sck.kernel_clock()
    }

    /// The instance's name in the reference manual.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Measures every bound with `clock` instead of counting polls.
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// One transfer of `len` bytes, at most `CHUNK`: `out(i)` gives byte `i`
    /// to send, `into(i, byte)` takes byte `i` received.
    fn exchange(
        &mut self,
        len: usize,
        out: impl Fn(usize) -> u8,
        mut into: impl FnMut(usize, u8),
    ) -> Result<(), Error> {
        let Ok(count) = u16::try_from(len) else {
            return Err(Error::TooLarge);
        };
        if count == 0 {
            return Ok(());
        }
        let unit = self.info.unit();
        let bits = u32::from(count).saturating_mul(8);
        let cycles = bits
            .saturating_mul(self.sck.cycles())
            .saturating_mul(2)
            .saturating_add(crate::wait::MARGIN_CYCLES);
        let mut deadline = Deadline::new(self.clock, cycles, 1);
        unit.start_polled(count);
        for i in 0..len {
            while !unit.tx_ready() {
                if deadline.expired() {
                    return self.fail(Error::Timeout);
                }
            }
            unit.write_byte(out(i));
            while !unit.rx_ready() {
                if deadline.expired() {
                    return self.fail(Error::Timeout);
                }
            }
            into(i, unit.read_byte());
        }
        while !unit.done() {
            if deadline.expired() {
                return self.fail(Error::Timeout);
            }
        }
        self.sck.hold_after_end();
        let (overrun, mode_fault) = unit.take_errors();
        unit.end_polled();
        if mode_fault {
            return self.fail(Error::ModeFault);
        }
        if overrun {
            return Err(Error::Overrun);
        }
        Ok(())
    }

    fn fail(&mut self, error: Error) -> Result<(), Error> {
        self.info
            .unit()
            .reset_state(self.sck.cycles().saturating_mul(32));
        Err(error)
    }
}

impl<W: Parts> embedded_hal::spi::ErrorType for BlockingSpi<W> {
    type Error = Error;
}

/// Every operation is bounded. The bytes sent while reading are 0x00.
impl<W: Parts> embedded_hal::spi::SpiBus<u8> for BlockingSpi<W> {
    fn read(&mut self, words: &mut [u8]) -> Result<(), Error> {
        for part in words.chunks_mut(CHUNK) {
            self.exchange(
                part.len(),
                |_| 0,
                |i, byte| {
                    if let Some(slot) = part.get_mut(i) {
                        *slot = byte;
                    }
                },
            )?;
        }
        Ok(())
    }

    fn write(&mut self, words: &[u8]) -> Result<(), Error> {
        for part in words.chunks(CHUNK) {
            self.exchange(part.len(), |i| part.get(i).copied().unwrap_or(0), |_, _| {})?;
        }
        Ok(())
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> Result<(), Error> {
        let total = read.len().max(write.len());
        let mut at = 0;
        while at < total {
            let len = CHUNK.min(total - at);
            let out = write.get(at..).unwrap_or(&[]);
            let into = read.get_mut(at..).unwrap_or(&mut []);
            self.exchange(
                len,
                |i| out.get(i).copied().unwrap_or(0),
                |i, byte| {
                    if let Some(slot) = into.get_mut(i) {
                        *slot = byte;
                    }
                },
            )?;
            at += len;
        }
        Ok(())
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> Result<(), Error> {
        for part in words.chunks_mut(CHUNK) {
            // Byte `i` goes out before byte `i` comes back, so each slot is
            // read before it is written.
            let cells = core::cell::Cell::from_mut(part).as_slice_of_cells();
            self.exchange(
                cells.len(),
                |i| cells.get(i).map_or(0, core::cell::Cell::get),
                |i, byte| {
                    if let Some(cell) = cells.get(i) {
                        cell.set(byte);
                    }
                },
            )?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        // Every operation has finished by the time it returns.
        Ok(())
    }
}
