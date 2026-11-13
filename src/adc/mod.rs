//! The ADC every chip offers, independent of the hardware underneath: regular
//! conversions, read one at a time or as a sequence over DMA. This module is
//! chip-neutral; each IP version (`adc_v2` on F4, `adc_v4` on H7) supplies the
//! register operations.
//!
//! - **init**: `Adc::new(&mut rcc, adc, &config)` powers the converter up and,
//!   on the H7, calibrates it. Every wait is bounded.
//! - **channels**: an analog pin names its [`Channel`] through
//!   [`Adc::channel`]; the routing trait rejects a pin the instance can't read
//!   at compile time. [`InternalChannels`] adds the temperature sensor and the
//!   internal reference, on the instance that has them.
//! - **one at a time**: [`Adc::read`] blocks for one conversion, some tens of
//!   microseconds at the sampling times a slow source needs.
//! - **a sequence**: [`AdcScan`] converts up to 16 channels into a buffer over
//!   DMA; `start` returns at once and the stream's interrupt, or `poll`,
//!   reports the end. FerroHAL never touches the NVIC.
//! - **volts**: [`Adc::to_millivolts`] takes the supply as an argument. Where
//!   the internal reference is available, [`Adc::vdda_millivolts`] measures it
//!   against the factory calibration; elsewhere a board states its nominal
//!   supply.
//!
//! Requirements: ADC-001, ADC-002, ADC-003, ADC-004, ADC-005, API-002

use core::marker::PhantomData;

use crate::{
    adc_ip::{MAX_SAMPLE, Unit},
    chip::{Interrupt, TS_CAL1, TS_CAL2, VREFIN_CAL},
    dma::{
        Channel as DmaChannel, Direction, FifoMode, Flags, Interrupts, Priority, StreamInstance,
        StreamInterrupt, Transfer, WordSize,
    },
    rcc::{Rcc, RccBit},
    resource::Refused,
};

/// What [`Adc::into_scan`](Adc::into_scan) hands back when it refuses: the
/// reason, and the ADC, the stream and the buffer it took.
pub type ScanRefused<A, S> = crate::resource::Refused<Error, (Adc<A>, S, &'static mut [u16])>;

/// How many register polls to give a stream to stop.
const STOP_SPINS: u32 = 100_000;
/// The calibration values' supply, and the temperatures they were taken at.
const CAL_VDDA_MV: u32 = 3300;
const TS_CAL1_C: i32 = 30;
const TS_CAL2_C: i32 = 110;

/// The conversion settings: sample time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Config {
    /// The sampling time in ADC clock cycles, rounded up to one the chip has.
    /// A source with high impedance, or the internal channels, need a long one.
    pub sample_cycles: u16,
}

impl Config {
    /// Long enough for a divider on a battery input and the internal channels
    /// (480 cycles at 21 MHz on the F4, 64.5 at 4 MHz on the H7).
    pub const fn slow_source() -> Self {
        Self {
            sample_cycles: crate::adc_ip::SLOW_SOURCE_CYCLES,
        }
    }
}

/// Why a conversion or setup failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The kernel clock isn't running (the H7's per_ck, from HSI).
    KernelClock,
    /// The converter didn't come up, calibrate or finish within its bound.
    Timeout,
    /// A conversion overwrote one nobody had read.
    Overrun,
    /// A DMA stream reported an error, or wouldn't start or stop.
    Dma,
    /// A sequence is running.
    Busy,
    /// No channels, or more than 16, or more than the buffer holds.
    Sequence,
    /// A buffer cannot serve DMA: out of the controller's reach, or cacheable.
    Buffer(crate::dma::BufferError),
    /// The calibration values or the reading can't give an answer.
    Calibration,
}

/// An ADC instance.
pub trait Instance: crate::Sealed {
    const NAME: &'static str;
    const ENABLE: RccBit;
    /// Its kernel clock, on chips where that decides its speed: the bus it
    /// sits on.
    const KERNEL: crate::rcc::Kernel;
    /// Whether its internal channels should be powered.
    const INTERNAL: bool;
    #[doc(hidden)]
    fn info() -> &'static Info;
}

/// Per-instance facts: the ADC's registers and its common block. Chip
/// facts: the glue layer holds one static `Info` per ADC.
pub struct Info {
    unit: Unit,
}

// SAFETY: `unit` is two addresses.
unsafe impl Sync for Info {}

impl Info {
    pub(crate) const fn new(unit: Unit) -> Self {
        Self { unit }
    }

    pub(crate) fn unit(&self) -> Unit {
        self.unit
    }
}

/// An ADC instance with the temperature sensor and internal reference.
pub trait InternalChannels: Instance {
    const TEMPERATURE: u8;
    const VREFINT: u8;
}

/// An analog pin that `A` can read, as its positive input.
pub trait AdcPin<A: Instance>: crate::Sealed {
    const CHANNEL: u8;
}

/// A DMA stream that serves `A`.
///
/// # Safety
///
/// `REQUEST` must route `A`'s request to this stream.
pub unsafe trait AdcDma<A: Instance>: StreamInstance + StreamInterrupt {
    const REQUEST: u8;
}

/// One channel of one kind of ADC.
pub struct Channel<A> {
    number: u8,
    _adc: PhantomData<A>,
}

/// One ADC input's wiring, as the board states it: the pin in analog mode
/// that `A` reads, and the DMA stream that serves `A`'s conversions. The IP
/// traits check that they belong together; this carries them and describes
/// itself as data for the board's manifest (`RESOURCES`), and gives the
/// channel its pin measures. The scan itself is assembled at runtime from
/// the wirings' channels (`Adc::into_scan`), whose refusal hands the ADC,
/// the stream and the buffer back.
pub struct AdcWiring<A, Pin, Stream> {
    /// The pin, in analog mode.
    pub pin: Pin,
    /// The DMA stream serving `A`.
    pub stream: Stream,
    _adc: PhantomData<fn() -> A>,
}

impl<A, Pin, Stream> AdcWiring<A, Pin, Stream> {
    /// A wiring for one of `A`'s inputs, from its pin and stream.
    pub const fn new(pin: Pin, stream: Stream) -> Self {
        Self {
            pin,
            stream,
            _adc: PhantomData,
        }
    }
}

impl<A, Pin, const CD: u8, const SN: u8> AdcWiring<A, Pin, crate::dma::Stream<CD, SN>>
where
    A: Instance,
    Pin: AdcPin<A> + crate::gpio::AltPin,
    crate::dma::Stream<CD, SN>: AdcDma<A>,
{
    /// Everything this wiring uses, as data: the instance, the analog pin,
    /// and the stream with its request.
    pub const RESOURCES: [crate::resource::Resource; 3] = [
        crate::resource::Resource::Peripheral(A::NAME),
        crate::resource::Resource::Analog {
            signal: crate::resource::Signal::Adc(Pin::CHANNEL),
            port: Pin::PORT,
            pin: Pin::PIN,
        },
        crate::resource::Resource::Dma {
            signal: crate::resource::Signal::Adc(Pin::CHANNEL),
            controller: CD,
            stream: SN,
            request: <crate::dma::Stream<CD, SN> as AdcDma<A>>::REQUEST,
        },
    ];

    /// The channel this wiring's pin measures.
    pub const fn channel(&self) -> Channel<A> {
        Channel::new(Pin::CHANNEL)
    }
}

impl<A> Clone for Channel<A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<A> Copy for Channel<A> {}

impl<A> Channel<A> {
    const fn new(number: u8) -> Self {
        Self {
            number,
            _adc: PhantomData,
        }
    }

    /// The channel's number, as the manuals give it.
    pub const fn number(self) -> u8 {
        self.number
    }
}

/// A powered-up ADC, converting one channel at a time.
pub struct Adc<A> {
    unit: Unit,
    token: A,
}

impl<A: Instance> Adc<A> {
    /// Enables and resets the ADC and powers it up: on the H7 that also
    /// calibrates it, which takes a few milliseconds. Refuses with the ADC
    /// token handed back; on a power-up refusal the ADC is already enabled
    /// and reset.
    pub fn new(rcc: &mut Rcc, token: A, config: &Config) -> Result<Self, Refused<Error, A>> {
        if let Err(error) = crate::adc_ip::start_clock(rcc) {
            return Err(Refused::new(error, token));
        }
        rcc.enable(A::ENABLE);
        rcc.reset(A::ENABLE);
        let unit = A::info().unit();
        if let Err(error) =
            unit.power_up(&rcc.clocks(), A::KERNEL, A::INTERNAL, config.sample_cycles)
        {
            return Err(Refused::new(error, token));
        }
        Ok(Self { unit, token })
    }

    /// Powers the ADC down and hands the token back: what a `Drop` would do,
    /// made explicit so the token can move out. A reopen runs
    /// [`Adc::new`] again, which resets, powers up and (on the H7)
    /// calibrates it.
    pub fn free(self) -> A {
        self.unit.power_down();
        self.token
    }

    /// The channel of `pin`, which must be in analog mode.
    pub const fn channel<P: AdcPin<A>>(&self, _pin: &P) -> Channel<A> {
        Channel::new(P::CHANNEL)
    }

    /// One conversion. Blocks for it, bounded.
    pub fn read(&mut self, channel: Channel<A>) -> Result<u16, Error> {
        self.unit.read(channel.number)
    }

    /// The largest sample: the full scale.
    pub const fn max_sample(&self) -> u16 {
        MAX_SAMPLE
    }

    /// What a sample means with `vdda_mv` on the reference.
    pub const fn to_millivolts(&self, sample: u16, vdda_mv: u32) -> u32 {
        sample as u32 * vdda_mv / MAX_SAMPLE as u32
    }

    /// Hands the ADC to a [`AdcScan`] converting `channels` in order over
    /// `stream` into `buffer`. Refuses with the ADC, the stream and the
    /// buffer handed back: the sequence and the buffer are checked, and the
    /// stream stopped through a second owner beside its token, before
    /// anything is consumed.
    pub fn into_scan<S>(
        self,
        stream: S,
        channels: &[Channel<A>],
        buffer: &'static mut [u16],
    ) -> Result<AdcScan<A, S>, ScanRefused<A, S>>
    where
        S: AdcDma<A>,
    {
        if channels.is_empty() || channels.len() > 16 || channels.len() > buffer.len() {
            return Err(Refused::new(Error::Sequence, (self, stream, buffer)));
        }
        if let Err(error) = crate::chip::check_dma_buffer(as_bytes(buffer)) {
            return Err(Refused::new(Error::Buffer(error), (self, stream, buffer)));
        }
        let mut numbers = [0u8; 16];
        for (number, channel) in numbers.iter_mut().zip(channels) {
            *number = channel.number;
        }
        let mut dma = crate::dma::claim_channel(&stream);
        if dma.stop(STOP_SPINS).is_err() {
            return Err(Refused::new(Error::Dma, (self, stream, buffer)));
        }
        dma.clear_flags(Flags::ALL);
        self.unit
            .set_sequence(numbers.get(..channels.len()).unwrap_or(&[]));
        Ok(AdcScan {
            adc: self,
            stream,
            dma,
            request: S::REQUEST,
            vector: S::INTERRUPT,
            buffer,
            len: channels.len() as u16,
            state: ScanState::Idle,
        })
    }
}

impl<A: InternalChannels> Adc<A> {
    pub const fn temperature_channel(&self) -> Channel<A> {
        Channel::new(A::TEMPERATURE)
    }

    pub const fn vrefint_channel(&self) -> Channel<A> {
        Channel::new(A::VREFINT)
    }

    /// The supply on VDDA, from the internal reference and its factory
    /// calibration.
    pub fn vdda_millivolts(&mut self) -> Result<u32, Error> {
        vdda_from_vrefint(self.read(self.vrefint_channel())?)
    }

    /// The die's temperature, in tenths of a degree, from the sensor's two
    /// factory calibration points.
    pub fn temperature_decidegrees(&mut self) -> Result<i32, Error> {
        let vdda = self.vdda_millivolts()?;
        temperature_from_samples(self.read(self.temperature_channel())?, vdda)
    }
}

/// VDDA in millivolts, from a VREFINT sample and the factory calibration.
pub fn vdda_from_vrefint(sample: u16) -> Result<u32, Error> {
    if sample == 0 {
        return Err(Error::Calibration);
    }
    // SAFETY: the chip's factory-programmed value, a read-only halfword.
    let calibration = unsafe { VREFIN_CAL.read_volatile() };
    Ok(CAL_VDDA_MV * u32::from(calibration) / u32::from(sample))
}

/// The die's temperature in tenths of a degree, from a sensor sample and the
/// VDDA it was taken with.
pub fn temperature_from_samples(sample: u16, vdda_mv: u32) -> Result<i32, Error> {
    // SAFETY: the chip's factory-programmed values, read-only halfwords.
    let (low, high) = unsafe { (TS_CAL1.read_volatile(), TS_CAL2.read_volatile()) };
    if high <= low {
        return Err(Error::Calibration);
    }
    let at_cal_vdda = (u32::from(sample) * vdda_mv / CAL_VDDA_MV) as i32;
    let span = i32::from(high) - i32::from(low);
    Ok(TS_CAL1_C * 10 + (at_cal_vdda - i32::from(low)) * (TS_CAL2_C - TS_CAL1_C) * 10 / span)
}

fn as_bytes(buffer: &[u16]) -> &[u8] {
    // SAFETY: the same memory, every byte of a u16 initialized, u8 aligned.
    unsafe { core::slice::from_raw_parts(buffer.as_ptr().cast(), core::mem::size_of_val(buffer)) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanState {
    Idle,
    Running,
    Done,
}

/// A fixed sequence of channels, converted over DMA. It holds the ADC, its
/// stream's token and the buffer, which [`free`](Self::free) hands back.
pub struct AdcScan<A, S> {
    adc: Adc<A>,
    stream: S,
    dma: DmaChannel,
    request: u8,
    vector: Interrupt,
    buffer: &'static mut [u16],
    len: u16,
    state: ScanState,
}

impl<A: Instance, S> AdcScan<A, S> {
    /// Converts the sequence once. Returns at once.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.state == ScanState::Running {
            return Err(Error::Busy);
        }
        let transfer = Transfer {
            request: self.request,
            #[cfg(feature = "dma_v1")]
            bufferable: false,
            direction: Direction::PeripheralToMemory,
            peripheral: self.adc.unit.data_address(),
            memory: self.buffer.as_mut_ptr() as u32,
            double_buffer: None,
            len: self.len,
            size: WordSize::HalfWord,
            memory_increment: true,
            circular: false,
            priority: Priority::Medium,
            fifo: FifoMode::Direct,
            interrupts: Interrupts {
                complete: true,
                error: true,
                ..Interrupts::default()
            },
        };
        self.dma.stop(STOP_SPINS).map_err(|_| Error::Dma)?;
        self.dma.clear_flags(Flags::ALL);
        // SAFETY: the buffer is this scan's own and holds `len` samples; the
        // address is this ADC's data register, whose request paces the stream.
        unsafe { self.dma.start(&transfer) }.map_err(|_| Error::Dma)?;
        self.adc.unit.start_sequence();
        self.state = ScanState::Running;
        Ok(())
    }

    /// Serves the stream's interrupt: `Some` once the sequence has finished.
    pub fn on_interrupt(&mut self) -> Option<Result<&[u16], Error>> {
        if self.state != ScanState::Running {
            self.dma.clear_flags(Flags::ALL);
            return None;
        }
        let flags = self.dma.flags();
        let failed = flags.transfer_error || flags.direct_mode_error;
        if !failed && !flags.complete {
            return None;
        }
        self.dma.clear_flags(Flags::ALL);
        let overrun = self.adc.unit.finish_sequence();
        if failed {
            self.state = ScanState::Idle;
            return Some(Err(Error::Dma));
        }
        if overrun {
            self.state = ScanState::Idle;
            return Some(Err(Error::Overrun));
        }
        self.state = ScanState::Done;
        Some(Ok(self.samples()))
    }

    /// As [`on_interrupt`](Self::on_interrupt), for a firmware that polls.
    pub fn poll(&mut self) -> Option<Result<&[u16], Error>> {
        self.on_interrupt()
    }

    /// The last finished sequence's samples, in channel order; empty otherwise.
    pub fn samples(&self) -> &[u16] {
        if self.state == ScanState::Done {
            self.buffer.get(..usize::from(self.len)).unwrap_or(&[])
        } else {
            &[]
        }
    }

    /// Stops a sequence that never finished.
    pub fn abort(&mut self) {
        let _ = self.dma.stop(STOP_SPINS);
        self.dma.clear_flags(Flags::ALL);
        self.adc.unit.finish_sequence();
        self.state = ScanState::Idle;
    }

    pub const fn is_busy(&self) -> bool {
        matches!(self.state, ScanState::Running)
    }

    /// Stops the stream and any sequence and hands back what
    /// [`Adc::into_scan`] took: the ADC, still powered, the stream's token and
    /// the buffer. [`Adc::free`] then powers the ADC down and returns its
    /// token; `into_scan` on the same parts starts a new scan.
    pub fn free(mut self) -> (Adc<A>, S, &'static mut [u16]) {
        self.abort();
        (self.adc, self.stream, self.buffer)
    }

    /// The vector to bind to the handler that calls `on_interrupt`: the
    /// stream's.
    pub const fn interrupt(&self) -> Interrupt {
        self.vector
    }

    /// The largest sample: the full scale.
    pub const fn max_sample(&self) -> u16 {
        MAX_SAMPLE
    }

    /// What a sample means with `vdda_mv` on the reference.
    pub const fn to_millivolts(&self, sample: u16, vdda_mv: u32) -> u32 {
        self.adc.to_millivolts(sample, vdda_mv)
    }
}
