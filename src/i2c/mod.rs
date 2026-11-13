//! The I2C master every chip offers, independent of the hardware underneath.
//!
//! A held line differs by IP: the F4's `i2c_v1` reports `Error::Busy` for a
//! slave holding SDA low, and its nine-clock recovery frees it; the H7's
//! `i2c_v2` recovery clears `ISR.BUSY`, so the driver cannot see the hold
//! and reports a bounded `Timeout` instead — the live-path runs accept
//! exactly that.
//!
//! This module is chip-neutral. It defines what an I2C driver takes and
//! reports; each IP version (`i2c_v1` on F4, `i2c_v2` on H7) supplies the
//! register operations under one driver of the same shape:
//!
//! - **init**: `I2c::new(&mut rcc, wiring, &config)`. The [`Wiring`] comes
//!   from the board file and names the instance and its two pins; the
//!   routing traits reject a wrong combination at compile time. A refusal
//!   is a [`Refused`]: the reason, with the wiring handed back. Every
//!   refusal comes before anything is written to hardware.
//! - **transfer**: the blocking `embedded_hal::i2c::I2c` trait, which the
//!   DPS310 and the u-blox DDC drivers take. Adjacent operations of the
//!   same kind are carried as one segment, with a repeated start only
//!   where the trait's contract asks for one, and a stop after the last.
//! - **waits**: every operation is bounded, twice its time on the wire
//!   plus a margin, in CPU cycles, measured with a [`CycleClock`] when one
//!   is set, otherwise counted as passes of at least `PASS_CYCLES`. A
//!   transfer that runs past its bound is stopped, the peripheral is put
//!   back, and the next transfer can start: [`Error::Timeout`].
//! - **errors**: a device's NACK, a bus error, lost arbitration and an
//!   overrun are reported, their flags cleared and the bus recovered, so
//!   the next transfer can start.
//! - **scope**: master only, 7-bit addresses, no SMBus, no clock
//!   stretching on our side (a slow device may stretch SCL; the bound
//!   covers only a device that answers within it). No DMA: an I2C
//!   exchange is a few bytes.
//!
//! Requirements: I2C-001, I2C-002, I2C-003, I2C-004, I2C-005, I2C-006, API-002

pub use crate::resource::{Refused, Resource, Signal};
pub use crate::time::CycleClock;
use crate::{
    gpio::AltPin,
    rcc::{Rcc, RccBit},
    time::Hertz,
    wait::Deadline,
};

use crate::i2c_ip::Unit;

/// The fewest CPU cycles one pass of a completion wait can take, used when
/// no [`CycleClock`] is set. SPI's measured floor, halved (H753 with caches
/// on: 233 cycles a pass); the same loop shape runs here, so the same floor
/// counts. Without a clock, waits run up to about 3 times the time asked
/// on the F401 and 6 on the uncached H753.
const PASS_CYCLES: u32 = 112;
/// Added to every blocking bound, in CPU cycles: the start-up and flag
/// handling an operation can see before its first SCL edge.
const MARGIN_CYCLES: u32 = 20_000;
/// Bits of wire time an operation of `len` bytes occupies: nine per byte,
/// plus a start, an address and an acknowledge, rounded up for the stop.
const fn wire_bits(len: usize) -> u32 {
    12 + 9 * len as u32
}

/// Per-instance facts. Chip facts: the glue layer holds one static `Info`
/// per I2C.
pub struct Info {
    unit: Unit,
}

// SAFETY: `unit` is an address.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(unit: Unit) -> Self {
        Self { unit }
    }

    pub(crate) fn unit(&self) -> Unit {
        self.unit
    }
}

/// An I2C instance this chip has. Chip facts: the glue layer implements it.
pub trait Instance: crate::Sealed + 'static {
    /// The bus whose clock is its kernel clock: APB1 everywhere but the
    /// H7's I2C4, which sits on APB4. `freeze` writes the H7's kernel
    /// muxes to match.
    const KERNEL: crate::rcc::Kernel;
    /// Its name in the reference manual.
    const NAME: &'static str;
    /// Its RCC enable (and reset) bit.
    const ENABLE: RccBit;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// A pin, in the right alternate function, that carries `U`'s SCL.
pub trait SclPin<U: Instance>: crate::Sealed {}
/// A pin, in the right alternate function, that carries `U`'s SDA.
pub trait SdaPin<U: Instance>: crate::Sealed {}

/// What the board file hands to `I2c::new`: one I2C instance and its two
/// pins. The driver's trait bounds check that they belong together; this
/// struct only carries them.
pub struct Wiring<U, Scl, Sda> {
    pub i2c: U,
    pub scl: Scl,
    pub sda: Sda,
}

impl<U, Scl, Sda> Wiring<U, Scl, Sda>
where
    U: Instance,
    Scl: SclPin<U> + AltPin,
    Sda: SdaPin<U> + AltPin,
{
    /// Everything this wiring uses, as data: the instance and its two pins
    /// with their alternate functions.
    pub const RESOURCES: [Resource; 3] = [
        Resource::Peripheral(U::NAME),
        Resource::Pin {
            signal: Signal::Scl,
            port: Scl::PORT,
            pin: Scl::PIN,
            af: Scl::AF,
        },
        Resource::Pin {
            signal: Signal::Sda,
            port: Sda::PORT,
            pin: Sda::PIN,
            af: Sda::AF,
        },
    ];
}

/// The bus settings: the SCL frequency asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The fastest SCL allowed. The driver computes its timing registers
    /// for it and reports the rate in use through [`I2c::frequency`].
    /// 1 kHz to 400 kHz on an F4, 1 kHz to 1 MHz on an H7.
    pub speed: Hertz,
}

impl Config {
    pub const fn new(speed: Hertz) -> Self {
        Self { speed }
    }

    /// Whether this speed can run from this kernel clock, and why not if
    /// it can't. A `const fn`, so a board can check its buses while
    /// compiling.
    pub const fn check(&self, kernel_clock: Hertz) -> Result<(), ConfigError> {
        if self.speed.0 == 0 {
            return Err(ConfigError::Speed);
        }
        match Unit::timing(kernel_clock, self.speed) {
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Config {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "Config {{ speed: {} Hz }}", self.speed.0)
    }
}

/// Why `I2c::new` refused a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ConfigError {
    /// The speed asked for is outside what the IP version carries, or the
    /// kernel clock cannot divide down to it.
    Speed,
    /// The kernel clock is not a frequency the IP version can time from:
    /// an F4 needs a whole number of MHz between 2 and 62.
    KernelClock,
}

/// Why a transfer failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The bus was busy when the transfer was asked for: a stuck line, or
    /// a transfer that never saw its stop.
    Busy,
    /// No device acknowledged the address, or a device acknowledged it and
    /// then refused a byte. The flags were cleared and the bus recovered.
    Nack,
    /// The transfer didn't finish within its bound. It was stopped and the
    /// bus recovered; the next one can start.
    Timeout,
    /// A bus error: a misplaced START or STOP, or a line that didn't
    /// behave. The flag was cleared and the bus recovered.
    Bus,
    /// Another master kept the bus (arbitration lost). The flag was
    /// cleared and the bus recovered.
    ArbitrationLost,
    /// A byte arrived before the last one was taken. The flag was cleared
    /// and the bus recovered.
    Overrun,
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        use embedded_hal::i2c::{ErrorKind, NoAcknowledgeSource};
        match self {
            Self::Nack => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
            Self::Bus => ErrorKind::Bus,
            Self::ArbitrationLost => ErrorKind::ArbitrationLoss,
            Self::Overrun => ErrorKind::Overrun,
            Self::Busy | Self::Timeout => ErrorKind::Other,
        }
    }
}

/// Counts since `new`, for health monitoring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Health {
    /// Transactions that completed.
    pub transactions: u32,
    /// Transfers that no device acknowledged.
    pub nacks: u32,
    /// Transfers that ran past their bound.
    pub timeouts: u32,
    /// Bus errors (BERR).
    pub bus_errors: u32,
    /// Arbitrations lost to another master (ARLO).
    pub arbitration_lost: u32,
    /// Overruns (OVR).
    pub overruns: u32,
    /// Transfers refused because the bus was busy.
    pub busy: u32,
}

impl Health {
    fn count(&mut self, error: Error) {
        match error {
            Error::Nack => self.nacks += 1,
            Error::Timeout => self.timeouts += 1,
            Error::Bus => self.bus_errors += 1,
            Error::ArbitrationLost => self.arbitration_lost += 1,
            Error::Overrun => self.overruns += 1,
            Error::Busy => self.busy += 1,
        }
    }
}

/// What an I2C was opened from: the instance its handle names. Implemented
/// by [`Wiring`] only, so a handle's type names the board's whole route.
pub trait I2cParts: crate::Sealed {
    type Instance: Instance;
}

impl<U, Scl, Sda> crate::Sealed for Wiring<U, Scl, Sda> {}
impl<U: Instance, Scl, Sda> I2cParts for Wiring<U, Scl, Sda> {
    type Instance = U;
}

/// An I2C master, blocking and bounded, for the `embedded-hal` bus trait
/// the device drivers take. It keeps the board's wiring, the instance and
/// both pins, and [`free`](I2c::free) hands it back, so the same parts open
/// again.
pub struct I2c<W: I2cParts> {
    info: &'static Info,
    kernel_clock: Hertz,
    speed: Hertz,
    /// CPU cycles per SCL bit, rounded up.
    bit_cycles: u32,
    clock: Option<&'static dyn CycleClock>,
    name: &'static str,
    health: Health,
    wiring: W,
}

impl<U, Scl, Sda> I2c<Wiring<U, Scl, Sda>>
where
    U: Instance,
    Scl: SclPin<U> + AltPin,
    Sda: SdaPin<U> + AltPin,
{
    /// Opens `wiring` with `config`: enables and resets the I2C and
    /// computes its timing registers. Returns the SCL in use through
    /// [`frequency`](Self::frequency).
    ///
    /// A refusal hands back `wiring` with the reason, to retry or use
    /// elsewhere. Every refusal comes before anything is written to
    /// hardware.
    ///
    /// On success the handle keeps the wiring; [`free`](Self::free) returns
    /// it.
    pub fn new(
        rcc: &mut Rcc,
        wiring: Wiring<U, Scl, Sda>,
        config: &Config,
    ) -> Result<Self, Refused<ConfigError, Wiring<U, Scl, Sda>>> {
        let info = U::info();
        let clocks = rcc.clocks();
        let kernel_clock = U::KERNEL.clock(&clocks);
        let timing = match config.check(kernel_clock) {
            Ok(()) => Unit::timing(kernel_clock, config.speed),
            Err(error) => Err(error),
        };
        let timing = match timing {
            Ok(timing) => timing,
            Err(error) => return Err(Refused::new(error, wiring)),
        };
        let speed = timing.speed();
        rcc.enable(U::ENABLE);
        rcc.reset(U::ENABLE);
        info.unit().apply(&timing);
        Ok(Self {
            info,
            kernel_clock,
            speed,
            bit_cycles: clocks.cpu.0.div_ceil(speed.0.max(1)),
            clock: None,
            name: U::NAME,
            health: Health::default(),
            wiring,
        })
    }

    /// Closes the I2C and hands back what [`new`](Self::new) took, the
    /// wiring: what a `Drop` would do, made explicit so the parts can move
    /// out. The bus is idle between blocking transfers, so PE drops with
    /// nothing in flight; a slave mid-byte sees a stop-like silence. A
    /// reopen runs `I2c::new` again on the same parts.
    pub fn free(self) -> Wiring<U, Scl, Sda> {
        self.info.unit().disable();
        self.wiring
    }
}

impl<W: I2cParts> I2c<W> {
    /// The SCL in use.
    pub fn frequency(&self) -> Hertz {
        self.speed
    }

    /// The kernel clock the timing was computed from.
    pub fn kernel_clock(&self) -> Hertz {
        self.kernel_clock
    }

    /// The driver's name, as `Wiring::RESOURCES` states it.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Sets the clock the bounds are measured with. Without one, waits
    /// count passes of at least `PASS_CYCLES`, so they run long, never
    /// short.
    pub fn set_clock(&mut self, clock: Option<&'static dyn CycleClock>) {
        self.clock = clock;
    }

    /// Counts since `new`.
    pub fn health(&self) -> Health {
        self.health
    }

    /// One bounded wait: ready, an error flag, or the bound.
    fn wait_ready(
        &self,
        unit: Unit,
        deadline: &mut Deadline,
        ready: impl FnMut() -> bool,
    ) -> Result<(), Error> {
        let mut ready = ready;
        loop {
            if let Some(error) = unit.error() {
                return Err(error);
            }
            if ready() {
                return Ok(());
            }
            if deadline.expired() {
                return Err(Error::Timeout);
            }
        }
    }

    /// Carries `operations` to `address`. Adjacent operations of one kind
    /// form one segment: the trait's contract sends no repeated start
    /// inside a segment, one between segments, and a stop after the last.
    /// `embedded_hal::i2c::I2c::transaction` covers this whole method;
    /// `read`, `write` and `write_read` come from the trait.
    fn carry(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Error> {
        // The bound: the wire time of everything asked for, twice, plus a
        // margin. Empty operations carry nothing.
        let mut total_bits = 0;
        let mut last_used = 0;
        let mut used = false;
        for (at, operation) in operations.iter().enumerate() {
            let len = match operation {
                embedded_hal::i2c::Operation::Read(buffer) => buffer.len(),
                embedded_hal::i2c::Operation::Write(buffer) => buffer.len(),
            };
            if len > 0 {
                total_bits += wire_bits(len);
                last_used = at;
                used = true;
            }
        }
        if !used {
            // Nothing asked for: no start, no stop, the bus untouched.
            return Ok(());
        }
        let unit = self.info.unit();
        if unit.busy() {
            self.health.busy += 1;
            return Err(Error::Busy);
        }
        let mut deadline = Deadline::new(
            self.clock,
            2 * total_bits.saturating_mul(self.bit_cycles) + MARGIN_CYCLES,
            PASS_CYCLES,
        );
        let mut result = Ok(());
        // The walk: `left` counts the bytes the current segment still
        // holds; at zero, the next non-empty operation starts one.
        let mut left = 0;
        let mut len = 0;
        let mut position = 0;
        let mut read = false;
        let mut segment_last = false;
        for at in 0..operations.len() {
            let this_read = matches!(operations[at], embedded_hal::i2c::Operation::Read(_));
            let op_len = match &operations[at] {
                embedded_hal::i2c::Operation::Read(buffer) => buffer.len(),
                embedded_hal::i2c::Operation::Write(buffer) => buffer.len(),
            };
            if op_len == 0 {
                continue;
            }
            if left == 0 {
                // A segment begins. Its length runs through the adjacent
                // operations of this kind, empty ones between included:
                // they carry nothing, so no start may appear for them.
                read = this_read;
                len = op_len;
                let mut end = at;
                for (next_at, next) in operations[at + 1..].iter().enumerate() {
                    let next_len = match next {
                        embedded_hal::i2c::Operation::Read(buffer) if this_read => buffer.len(),
                        embedded_hal::i2c::Operation::Write(buffer) if !this_read => buffer.len(),
                        _ => 0,
                    };
                    if next_len == 0 {
                        break;
                    }
                    len += next_len;
                    end = at + 1 + next_at;
                }
                left = len;
                position = 0;
                segment_last = end >= last_used;
                unit.arm(address, read, len, segment_last);
                result = self.wait_ready(unit, &mut deadline, || {
                    unit.addr_step(address, read, len, segment_last)
                });
                if result.is_err() {
                    break;
                }
            }
            match &mut operations[at] {
                embedded_hal::i2c::Operation::Read(buffer) => {
                    for slot in buffer.iter_mut() {
                        result = self.wait_ready(unit, &mut deadline, || {
                            unit.recv_ready(position, len, segment_last)
                        });
                        if result.is_err() {
                            break;
                        }
                        *slot = unit.recv(position, len, segment_last);
                        position += 1;
                        left -= 1;
                    }
                }
                embedded_hal::i2c::Operation::Write(buffer) => {
                    for slot in buffer.iter() {
                        result =
                            self.wait_ready(unit, &mut deadline, || unit.tx_ready(position, len));
                        if result.is_err() {
                            break;
                        }
                        unit.send(*slot);
                        position += 1;
                        left -= 1;
                    }
                }
            }
            if result.is_err() {
                break;
            }
            if left == 0 {
                // The segment ends: the last byte out (BTF, or the
                // hardware's count), then a stop or, through the next
                // segment's start bit, a repeated start. A read segment's
                // tail was armed while taking its last byte.
                result = self.wait_ready(unit, &mut deadline, || unit.op_done(read, segment_last));
                if result.is_err() {
                    break;
                }
                unit.end_op(read, segment_last);
            }
        }
        if result.is_ok() {
            result = self.wait_ready(unit, &mut deadline, || unit.stop_done());
            if result.is_ok() {
                unit.stop_clear();
            }
        }
        if let Err(error) = result {
            self.health.count(error);
            // The bus is put back, bounded: flags cleared and the
            // peripheral reset through its PE bit, which releases the
            // lines. A line a device holds low stays its owner's to
            // clear; the next transfer reports [`Error::Busy`] until
            // then.
            unit.recover();
        } else {
            self.health.transactions += 1;
        }
        result
    }
}

impl<W: I2cParts> embedded_hal::i2c::ErrorType for I2c<W> {
    type Error = Error;
}

impl<W: I2cParts> embedded_hal::i2c::I2c for I2c<W> {
    fn transaction(
        &mut self,
        address: embedded_hal::i2c::SevenBitAddress,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> Result<(), Error> {
        self.carry(address, operations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_bits_cover_a_byte_each() {
        assert_eq!(wire_bits(1), 21);
        assert_eq!(wire_bits(0), 12);
    }

    #[test]
    fn a_zero_speed_is_refused_by_the_check() {
        assert_eq!(
            Config::new(Hertz(0)).check(Hertz::mhz(42)),
            Err(ConfigError::Speed)
        );
    }
}
