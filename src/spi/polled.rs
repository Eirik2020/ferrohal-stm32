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
//! Requirements: SPIPOLL-001, SPIPOLL-002

use crate::{
    gpio::AltPin,
    rcc::Rcc,
    resource::{Refused, Resource, Signal},
    spi::{
        BlockingWiring, Config, ConfigError, Error,
        stream_dma::{Info, Instance, MisoPin, MosiPin, SckPin},
    },
    spi_ip::Unit,
    time::{CycleClock, Hertz},
    wait::Deadline,
};

/// The most bytes one hardware transfer moves (the H7's TSIZE is 16 bits).
const CHUNK: usize = 65_535;
/// Added to every bound, in CPU cycles.
const MARGIN_CYCLES: u32 = 20_000;

/// An SPI master moving bytes by polling its data register.
pub struct BlockingSpi {
    info: &'static Info,
    kernel_clock: Hertz,
    cpu: Hertz,
    frequency: Hertz,
    sck_cycles: u32,
    clock: Option<&'static dyn CycleClock>,
    name: &'static str,
}

impl BlockingSpi {
    /// Enables and resets the SPI and configures it as a master. Chip select
    /// is the firmware's GPIO.
    ///
    /// A refusal hands back `wiring` with the reason. Every refusal comes
    /// before anything is written to hardware.
    #[allow(clippy::type_complexity)]
    pub fn new<U, Sck, Miso, Mosi>(
        rcc: &mut Rcc,
        wiring: BlockingWiring<U, Sck, Miso, Mosi>,
        config: &Config,
    ) -> Result<Self, Refused<ConfigError, BlockingWiring<U, Sck, Miso, Mosi>>>
    where
        U: Instance,
        Sck: SckPin<U>,
        Miso: MisoPin<U>,
        Mosi: MosiPin<U>,
    {
        let info = U::info();
        let clocks = rcc.clocks();
        let Some(kernel_clock) = U::KERNEL.frequency(&clocks) else {
            return Err(Refused::new(ConfigError::KernelClock, wiring));
        };
        let code = match config.divider(kernel_clock) {
            Ok(code) => code,
            Err(error) => return Err(Refused::new(error, wiring)),
        };
        // Past the last refusal: the wiring's tokens end when `new` returns.
        rcc.enable(info.enable());
        rcc.reset(info.enable());
        info.unit().configure(code, config);
        let frequency = Hertz(kernel_clock.0 >> (code + 1));
        Ok(Self {
            info,
            kernel_clock,
            cpu: clocks.cpu,
            frequency,
            sck_cycles: clocks.cpu.0.div_ceil(frequency.0.max(1)),
            clock: None,
            name: U::NAME,
        })
    }

    /// Changes the bus settings. Returns the new SCK.
    pub fn set_config(&mut self, config: &Config) -> Result<Hertz, ConfigError> {
        let code = config.divider(self.kernel_clock)?;
        self.info.unit().configure(code, config);
        self.frequency = Hertz(self.kernel_clock.0 >> (code + 1));
        self.sck_cycles = self.cpu.0.div_ceil(self.frequency.0.max(1));
        Ok(self.frequency)
    }

    /// The SCK in use.
    pub fn frequency(&self) -> Hertz {
        self.frequency
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
            .saturating_mul(self.sck_cycles)
            .saturating_mul(2)
            .saturating_add(MARGIN_CYCLES);
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
        if Unit::HOLD_AFTER_END {
            // ES0392 2.22.6, as for DMA transfers.
            cortex_m::asm::delay(self.sck_cycles);
        }
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
            .reset_state(self.sck_cycles.saturating_mul(32));
        Err(error)
    }
}

impl embedded_hal::spi::ErrorType for BlockingSpi {
    type Error = Error;
}

/// Every operation is bounded. The bytes sent while reading are 0x00.
impl embedded_hal::spi::SpiBus<u8> for BlockingSpi {
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

impl<U, Sck, Miso, Mosi> BlockingWiring<U, Sck, Miso, Mosi>
where
    U: Instance,
    Sck: SckPin<U> + AltPin,
    Miso: MisoPin<U> + AltPin,
    Mosi: MosiPin<U> + AltPin,
{
    /// Everything this wiring uses, as data: the instance and its three pins
    /// with their alternate functions. No streams, no vectors.
    pub const RESOURCES: [Resource; 4] = [
        Resource::Peripheral(U::NAME),
        Resource::Pin {
            signal: Signal::Sck,
            port: Sck::PORT,
            pin: Sck::PIN,
            af: Sck::AF,
        },
        Resource::Pin {
            signal: Signal::Miso,
            port: Miso::PORT,
            pin: Miso::PIN,
            af: Miso::AF,
        },
        Resource::Pin {
            signal: Signal::Mosi,
            port: Mosi::PORT,
            pin: Mosi::PIN,
            af: Mosi::AF,
        },
    ];
}
