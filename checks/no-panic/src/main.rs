//! Proves FerroHAL's public API has no reachable panic (§5, §13 of the
//! architecture brief), for one chip at a time:
//!
//!     cargo build --release --features stm32f401re
//!     cargo build --release --features stm32f411ce
//!     cargo build --release --features stm32f405rg
//!     cargo build --release --features stm32h743vi
//!     cargo build --release --features stm32h753zi
//!
//! `main` calls every public operation, with every input passed through
//! `black_box` so the optimiser cannot fold a check away. The panic handler
//! calls a function that exists nowhere, so the build links only if the
//! optimised program contains no call to it: no panic path survives. A
//! failing build names `ferrohal_panic_is_reachable` as undefined.
//!
//! It is never flashed; linking is the test.

#![no_std]
#![no_main]

use core::{hint::black_box, ptr::addr_of_mut};

use cortex_m_rt::entry;
use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};
use ferrohal_stm32::{
    chip::Peripherals,
    dma::{Flags, StreamInstance},
    i2c::{self, I2c},
    spi::{self, Spi},
    time::Hertz,
    uart::{self, Buffers, Parity, StopBits, Uart},
};

static mut TX: [u8; 64] = [0; 64];
static mut RX: [u8; 256] = [0; 256];
static mut RX2: [u8; 256] = [0; 256];
static mut SPI_TX: [u8; 32] = [0; 32];
static mut LANE: [u32; 20] = [0; 20];
static mut LANE2: [u32; 20] = [0; 20];
static mut COMPLEMENT: [u32; 20] = [0; 20];
static mut TWO_WAY: [u32; 8] = [0; 8];
static mut TWO_WAY_READS: [u16; 8] = [0; 8];
static mut SAMPLES: [u16; 64] = [0; 64];
static mut FRAME: [u32; 8] = [0; 8];
static mut LENT: [u16; 8] = [0; 8];
#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
static mut READER_LANE: [u32; 8] = [0; 8];
#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
static mut READER_READS: [u16; 8] = [0; 8];
static mut SPI_RX: [u8; 32] = [0; 32];

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else { park() };
    let Some((mut rcc, wiring, rx_wiring, spi_wiring, i2c_wiring, mut spare, mut out, mut input)) =
        chip::wire(p)
    else {
        park()
    };
    keep(rcc.clocks());
    exercise_pure();
    // The fugit conversions.
    let rate = fugit::HertzU32::from(black_box(rcc.clocks()).hclk);
    keep(ferrohal_stm32::time::Hertz::from(black_box(rate)));

    // GPIO, through the embedded-hal traits and inherently.
    let _ = OutputPin::set_high(&mut out);
    let _ = OutputPin::set_low(&mut out);
    keep(StatefulOutputPin::is_set_high(&mut out));
    keep(StatefulOutputPin::is_set_low(&mut out));
    keep(InputPin::is_high(&mut input));
    keep(InputPin::is_low(&mut input));
    out.set_high();
    keep(out.is_set_high());

    // SAFETY: `main` runs once, and these are its only references.
    let buffers = Buffers {
        tx: black_box(unsafe { &mut *addr_of_mut!(TX) }),
        rx: black_box(unsafe { &mut *addr_of_mut!(RX) }),
    };
    let config = black_box(uart::Config::new(Hertz(115_200)));
    // A refusal hands back what `new` took: retry once with it, 8N1.
    let mut uart = match Uart::new(&mut rcc, wiring, &config, buffers) {
        Ok(opened) => opened,
        Err(refused) => {
            keep(refused.error());
            let (wiring, buffers) = refused.into_parts();
            let Ok(opened) = Uart::new(
                &mut rcc,
                wiring,
                &uart::Config::new(Hertz(115_200)),
                buffers,
            ) else {
                park()
            };
            opened
        }
    };
    exercise_uart(&mut uart);

    // A receive-only UART, and the frame check with inversion asked for.
    let sbus = black_box(
        uart::Config::new(Hertz(100_000))
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(black_box(true)),
    );
    keep(ferrohal_stm32::uart::check::<chip::RxUart>(
        &black_box(rcc.clocks()),
        &sbus,
    ));
    // SAFETY: as TX and RX above.
    let ring = black_box(unsafe { &mut *addr_of_mut!(RX2) });
    let mut rx_only = match Uart::new_rx(&mut rcc, rx_wiring, &sbus, ring) {
        Ok(opened) => opened,
        Err(refused) => {
            keep(refused.error());
            let (rx_wiring, ring) = refused.into_parts();
            let Ok(opened) = Uart::new_rx(
                &mut rcc,
                rx_wiring,
                &uart::Config::new(Hertz(100_000)),
                ring,
            ) else {
                park()
            };
            opened
        }
    };
    let mut chunk = [0u8; 32];
    keep(rx_only.read_chunk(black_box(&mut chunk)));
    keep(rx_only.try_read(black_box(&mut chunk)));
    keep(rx_only.on_interrupt());
    keep(ferrohal_stm32::Service::on_interrupt(&mut rx_only));
    keep(rx_only.set_config(&sbus));
    rx_only.mask();
    rx_only.unmask();
    keep(rx_only.health());
    keep(rx_only.generation());
    // The buffer check on its own, MPU walk included on the H7.
    keep(ferrohal_stm32::chip::check_dma_buffer(black_box(
        &[0u8; 8][..],
    )));
    // The interrupt requirements, checked against the NVIC, and the
    // resource lists.
    let vectors = uart.vectors();
    keep(vectors.verify());
    if let Some(vector) = black_box(vectors).iter().next() {
        keep(vectors.contains(vector));
    }
    keep(rx_only.vectors().verify());
    vectors.iter().for_each(keep);
    black_box(&chip::UartWiring::RESOURCES)
        .iter()
        .for_each(keep);
    black_box(&chip::RxOnlyWiring::RESOURCES)
        .iter()
        .for_each(keep);
    black_box(&chip::SpiWiring::RESOURCES).iter().for_each(keep);
    keep(uart.on_interrupt().any());
    keep(ferrohal_stm32::Service::on_interrupt(&mut uart));

    // SPI: the frame check, then every operation.
    let spi_config = black_box(
        spi::Config::new(Hertz::mhz(1))
            .with_mode(spi::MODE_3)
            .with_bit_order(spi::BitOrder::LsbFirst),
    );
    keep(ferrohal_stm32::spi::check::<chip::SpiUnit>(
        &black_box(rcc.clocks()),
        &spi_config,
    ));
    // SAFETY: as TX and RX above.
    let spi_buffers = spi::Buffers {
        tx: black_box(unsafe { &mut *addr_of_mut!(SPI_TX) }),
        rx: black_box(unsafe { &mut *addr_of_mut!(SPI_RX) }),
    };
    let spi = match Spi::new(&mut rcc, spi_wiring, &spi_config, spi_buffers) {
        Ok(bus) => Ok(bus),
        Err(refused) => {
            keep(refused.error());
            let (spi_wiring, spi_buffers) = refused.into_parts();
            Spi::new(
                &mut rcc,
                spi_wiring,
                &spi::Config::new(Hertz::mhz(1)),
                spi_buffers,
            )
        }
    };
    if let Ok(mut bus) = spi {
        exercise_spi(&mut bus);
        keep(bus.free());
    }

    // I2C: the timing check, then every operation of the bus trait.
    black_box(&chip::I2cWiring::RESOURCES).iter().for_each(keep);
    keep(
        i2c::Config::new(Hertz::khz(400))
            .check(<chip::I2cUnit as i2c::Instance>::KERNEL.clock(&black_box(rcc.clocks()))),
    );
    keep(i2c::check::<chip::I2cUnit>(
        &black_box(rcc.clocks()),
        &black_box(i2c::Config::new(Hertz::khz(400))),
    ));
    // A refusal hands back the wiring: retry once with it, at 100 kHz.
    let mut i2c = match I2c::new(
        &mut rcc,
        i2c_wiring,
        &black_box(i2c::Config::new(Hertz::khz(400))),
    ) {
        Ok(bus) => bus,
        Err(refused) => {
            keep(refused.error());
            let i2c_wiring = refused.into_parts();
            let Ok(bus) = I2c::new(
                &mut rcc,
                i2c_wiring,
                &black_box(i2c::Config::new(Hertz::khz(100))),
            ) else {
                park()
            };
            bus
        }
    };
    exercise_i2c(&mut i2c);

    // The DMA API, on a stream the UART does not use.
    let mut channel = spare.take_stream();
    keep(channel.flags());
    keep(channel.flags().any_error());
    keep(channel.flags().failed());
    channel.clear_flags(black_box(Flags::ALL));
    channel.set_interrupts(black_box(ferrohal_stm32::dma::Interrupts {
        complete: true,
        half: false,
        error: true,
        fifo_error: true,
    }));
    keep(channel.remaining());
    keep(channel.is_running());
    channel.request_stop();
    keep(channel.stop(black_box(1_000)));
    let handle = channel.flag_handle();
    keep(handle.flags());
    handle.clear(black_box(Flags::default()));
    keep(handle.remaining());

    // The teardowns: each stops what it ran and hands the token back.
    keep(uart.free());
    keep(rx_only.free());
    keep(i2c.free());
    park()
}

/// The pure functions and accessors: clock solving and checking, I2C timing,
/// the errata table, and the ADC's conversions from factory calibration.
fn exercise_pure() {
    use ferrohal_stm32::rcc::{Config, Hse, HseMode, Request};

    keep(black_box(Hertz::mhz(8)).to_hz());
    let hse = Hse {
        freq: black_box(Hertz::mhz(8)),
        mode: black_box(HseMode::Bypass),
    };
    #[cfg(not(any(feature = "stm32h743vi", feature = "stm32h753zi")))]
    let request = Request {
        hse,
        sysclk: black_box(Hertz::mhz(84)),
        usb: black_box(Some(Hertz::mhz(48))),
    };
    #[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
    let request = Request {
        hse,
        sysclk: black_box(Hertz::mhz(400)),
        q: black_box(Some(Hertz::mhz(100))),
    };
    if let Ok(config) = Config::solve(request) {
        keep(black_box(config).validate());
    }
    #[cfg(not(any(feature = "stm32h743vi", feature = "stm32h753zi")))]
    keep(black_box(Config::hsi()).validate());

    keep(i2c::Config::new(black_box(Hertz::khz(400))).check(black_box(Hertz::mhz(42))));

    for erratum in ferrohal_stm32::chip::ERRATA {
        keep(erratum.affects(black_box(0x2003)));
        keep(erratum.id());
    }
    keep(ferrohal_stm32::signature::revision());
    keep(ferrohal_stm32::adc::vdda_from_vrefint(black_box(1500)));
    keep(ferrohal_stm32::adc::temperature_from_samples(
        black_box(900),
        black_box(3300),
    ));
}

fn exercise_uart(uart: &mut Uart<chip::UartWiring>) {
    use embedded_io::{Read, ReadReady, Write, WriteReady};

    let mut buf = [0u8; 32];
    let data = black_box(&[0x55u8; 40][..]);
    keep(uart.write(data));
    // The transmit queue.
    keep(uart.try_send(data));
    keep(uart.queued());
    keep(uart.tx_health());
    keep(uart.flush());
    keep(uart.is_idle());
    keep(uart.send_break());
    keep(uart.try_read(black_box(&mut buf)));
    keep(uart.read_within(black_box(&mut buf), black_box(10)));
    keep(uart.available());
    keep(Read::read(uart, black_box(&mut buf)));
    keep(Read::read_exact(uart, black_box(&mut buf)));
    keep(Write::write(uart, data));
    keep(Write::write_all(uart, data));
    keep(Write::flush(uart));
    keep(ReadReady::read_ready(uart));
    keep(WriteReady::write_ready(uart));
    keep(uart.write_ready());
    let config = black_box(
        uart::Config::new(Hertz(9_600))
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(true)
            .with_inverted_tx(true)
            .with_half_duplex(black_box(true))
            .with_swap(black_box(true)),
    );
    keep(uart.set_config(&config));
    // The line's event enables, off and on again.
    uart.mask();
    uart.unmask();
    // The chunk reader, the fault counts and the caller clock.
    keep(uart.read_chunk(black_box(&mut buf)));
    keep(uart.generation());
    keep(uart.health());
    uart.set_clock(Some(&CLOCK));
    keep(uart.read_within(black_box(&mut buf), black_box(10)));
    keep(uart.flush());
    uart.set_clock(None);
}

/// EXTI: route, every operation, release. Each chip's `wire` calls it on
/// a spare pin's line.
pub fn exercise_exti<const P: char, const N: u8>(
    ctrl: &mut ferrohal_stm32::exti::Ctrl,
    line: ferrohal_stm32::exti::Line<N>,
) where
    ferrohal_stm32::gpio::Port<P>: ferrohal_stm32::gpio::PortInstance,
{
    use embedded_hal::digital::InputPin;
    use ferrohal_stm32::exti::Edge;

    let mut exti = line.route::<P>(ctrl, black_box(Edge::Both));
    keep(exti.vectors().verify());
    keep(exti.is_pending());
    exti.trigger();
    exti.clear();
    keep(exti.is_high());
    keep(InputPin::is_low(&mut exti));
    exti.set_edge(ctrl, black_box(Edge::Falling));
    exti.mask(ctrl);
    exti.unmask(ctrl);
    let handle = exti.handle();
    keep(handle.is_pending());
    handle.clear();
    handle.trigger();
    keep(handle.vectors().verify());
    keep(handle.line());
    keep(ferrohal_stm32::exti::vector(black_box(12)));
    keep(exti.free(ctrl));
}

/// Timers: a tick and a timebase through every operation. Each chip's
/// `wire` calls it.
pub fn exercise_timers<T: ferrohal_stm32::timer::Instance, U: ferrohal_stm32::timer::Instance32>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    tick_timer: T,
    base_timer: U,
) {
    use ferrohal_stm32::timer::{Tick, Timebase, divider};

    keep(divider(
        black_box(Hertz::mhz(84)),
        black_box(Hertz::khz(1)),
        black_box(16),
    ));
    keep(Tick::<T>::check(
        &black_box(rcc.clocks()),
        black_box(Hertz::khz(8)),
    ));
    keep(Timebase::<U>::check(
        &black_box(rcc.clocks()),
        black_box(Hertz::mhz(1)),
    ));
    let mut tick = match Tick::new(rcc, tick_timer, black_box(Hertz::khz(1))) {
        Ok(tick) => tick,
        Err(refused) => {
            let (error, tick_timer) = refused.into_inner();
            keep(error);
            match Tick::new(rcc, tick_timer, black_box(Hertz::khz(1))) {
                Ok(tick) => tick,
                Err(_) => park(),
            }
        }
    };
    keep(tick.on_interrupt());
    keep(tick.rate());
    keep(tick.vectors().verify());
    tick.pause();
    tick.resume();
    let base = match Timebase::new(rcc, base_timer, black_box(Hertz::mhz(1))) {
        Ok(base) => base,
        Err(refused) => {
            keep(refused.error());
            let base_timer = refused.into_parts();
            match Timebase::new(rcc, base_timer, black_box(Hertz::mhz(1))) {
                Ok(base) => base,
                Err(_) => park(),
            }
        }
    };
    keep(base.now());
    let reader = base.handle();
    keep(reader.now());
    keep(reader.frequency());
    let tick_token = tick.free();
    keep(tick_token);
    let base_token = base.free();
    keep(base_token);
}

/// A PWM at `rate`, retrying once from a refusal's hand-back: the parts come
/// back exactly as they went in, so the retry type-checks unchanged.
fn retried_pwm<T: ferrohal_stm32::timer::ChannelTimer>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    rate: Hertz,
) -> ferrohal_stm32::timer::Pwm<T> {
    match ferrohal_stm32::timer::Pwm::new(rcc, timer, black_box(rate)) {
        Ok(pwm) => pwm,
        Err(refused) => {
            keep(refused.error());
            let timer = refused.into_parts();
            match ferrohal_stm32::timer::Pwm::new(rcc, timer, black_box(rate)) {
                Ok(pwm) => pwm,
                Err(_) => park(),
            }
        }
    }
}

/// A channel freed back to its pin; a lane assembled from that pin, its
/// refusal retried; the lane freed to its wiring and buffer and assembled
/// again from them.
pub fn exercise_lane_free<T, P, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
) where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<T, 1>,
{
    use ferrohal_stm32::timer::LaneWiring;

    let pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    let channel = pwm.channel::<1, P>(pin);
    let pin = channel.free();
    // SAFETY: as TX and RX in `main`.
    let buffer = black_box(unsafe { &mut *addr_of_mut!(LANE2) });
    let mut lane = match LaneWiring::<T, 1, P, _>::new(pin, stream).assemble(&pwm, buffer) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (wiring, buffer) = refused.into_parts();
            match wiring.assemble(&pwm, buffer) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    keep(lane.arm(&pwm, black_box(&[1, 2, 3][..])));
    lane.abort(&pwm);
    let Ok((wiring, buffer, _)) = lane.free(&pwm) else {
        park()
    };
    match wiring.assemble(&pwm, buffer) {
        Ok(lane) => keep(lane.free(&pwm)),
        Err(_) => park(),
    }
    keep(pwm.free());
}

/// PWM with a compare lane on channel 1, through every operation.
pub fn exercise_lane<T, P, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
) where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<T, 1>,
{
    use ferrohal_stm32::timer::{Pwm, nearest};

    keep(nearest(
        black_box(Hertz::mhz(200)),
        black_box(Hertz::khz(600)),
        black_box(16),
    ));
    keep(Pwm::<T>::check(
        &black_box(rcc.clocks()),
        black_box(Hertz::khz(600)),
    ));
    let pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    keep(pwm.period());
    keep(pwm.rate());
    pwm.set_output::<1>(black_box(false));
    pwm.set_output::<1>(black_box(true));
    // The wiring's constructor, from the pin and the stream: a refusal hands
    // both back, and the retry takes them.
    let wiring =
        ferrohal_stm32::timer::LaneWiring::<T, 1, P, ferrohal_stm32::dma::Stream<CD, SN>>::new(
            pin, stream,
        );
    // SAFETY: as TX and RX in `main`.
    let buffer = black_box(unsafe { &mut *addr_of_mut!(LANE) });
    let mut lane = match wiring.assemble(&pwm, buffer) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (wiring, buffer) = refused.into_parts();
            match wiring.assemble(&pwm, buffer) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    lane.set_duty(&pwm, black_box(70));
    keep(lane.duty(&pwm));
    keep(lane.arm(&pwm, black_box(&[1, 2, 3][..])));
    pwm.stage(black_box(1));
    pwm.start();
    keep(lane.on_interrupt());
    keep(ferrohal_stm32::Service::on_interrupt(&mut lane));
    keep(lane.is_busy());
    keep(lane.vectors().verify());
    lane.set_duty(&pwm, black_box(0));
    lane.abort(&pwm);
    keep(pwm.top_margin());
    pwm.close_requests();
    pwm.stop();
    // The generator over the same lane.
    let mut awg = ferrohal_stm32::timer::Awg::single(pwm, (lane,));
    keep(awg.lanes());
    keep(awg.period());
    keep(awg.sample_rate());
    awg.set_outputs(black_box(true));
    keep(awg.load(black_box(0), black_box(&[1, 2, 3][..])));
    keep(awg.is_busy());
    keep(awg.play());
    keep(awg.on_interrupt());
    keep(ferrohal_stm32::Service::on_interrupt(&mut awg));
    keep(awg.vectors().verify());
    awg.abort();
    awg.set_active_low(black_box(true));
    keep(awg.prepare_listen(black_box(Hertz::mhz(1)), black_box(4)));
    keep(awg.listen());
    keep(awg.samples(black_box(0)).len());
    keep(awg.speak());
    let mut out = [0u32; 4];
    keep(ferrohal_stm32::timer::recover(
        black_box(&[0u16, 4, 4, 0][..]),
        black_box(2),
        black_box(false),
        black_box(32),
        black_box(100),
        &mut out,
    ));
    keep(awg.free());
}

/// A lane that reads as well, on channel 1.
pub fn exercise_two_way<T, P, const CD: u8, const SN: u8, const PORT: char>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
) where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>:
        ferrohal_stm32::timer::CompareDma<T, 1> + ferrohal_stm32::timer::GpioDma,
    ferrohal_stm32::gpio::Port<PORT>: ferrohal_stm32::gpio::PortInstance,
{
    let pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    pwm.set_active_low::<1>(black_box(true));
    let wiring =
        ferrohal_stm32::timer::LaneWiring::<T, 1, P, ferrohal_stm32::dma::Stream<CD, SN>>::new(
            pin, stream,
        );
    // SAFETY: as TX and RX in `main`.
    let (buffer, reads) = black_box(unsafe {
        (
            &mut *addr_of_mut!(TWO_WAY),
            &mut *addr_of_mut!(TWO_WAY_READS),
        )
    });
    let mut lane = match wiring.assemble_two_way::<PORT>(&pwm, buffer, reads) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (wiring, (buffer, reads)) = refused.into_parts();
            match wiring.assemble_two_way::<PORT>(&pwm, buffer, reads) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    keep(lane.prepare_listen(black_box(4)));
    keep(lane.listen(&pwm, black_box(10)));
    keep(lane.samples().len());
    lane.abort(&pwm);
    keep(lane.is_prepared());
    keep(lane.port());
    keep(lane.is_complementary());
    keep(ferrohal_stm32::timer::lane_ccer_bits(
        black_box(1),
        black_box(false),
        black_box(false),
        black_box(true),
    ));
    // A frame in lent memory, checked and closed, played in place.
    keep(ferrohal_stm32::timer::FrameLimits::new(black_box(100), black_box(2)));
    // SAFETY: as TX and RX in `main`.
    let words = black_box(unsafe { &mut *addr_of_mut!(FRAME) });
    match ferrohal_stm32::timer::Frame::new(words, black_box(3), pwm.frame_limits()) {
        Ok(frame) => {
            keep(frame.duties().len());
            match lane.arm_frame(&pwm, frame) {
                Ok(()) => {
                    lane.abort(&pwm);
                    keep(lane.take_frame().map(ferrohal_stm32::timer::Frame::into_words));
                }
                Err(refused) => keep(refused.error()),
            }
        }
        Err(refused) => keep(refused.error()),
    }
    // Samples memory lent and taken back.
    // SAFETY: as TX and RX in `main`.
    let lent = black_box(unsafe { &mut *addr_of_mut!(LENT) });
    match lane.lend_samples(lent) {
        Ok(()) => keep(lane.take_samples()),
        Err(refused) => keep(refused.error()),
    }
    // A rate computed ahead, then applied, as the switch does.
    let mut pwm = pwm;
    keep(pwm.current_rate());
    if let Ok(rate) = pwm.prepare_rate(black_box(Hertz::mhz(1))) {
        pwm.apply_rate(&rate);
    }
    // The generator's frame and lent-memory calls over the same lane.
    let mut awg = ferrohal_stm32::timer::Awg::single(pwm, (lane,));
    keep(awg.frame_limits(black_box(0)));
    // SAFETY: as TX and RX in `main`; the frame above came back or was
    // refused, so the memory is free again.
    let words = black_box(unsafe { &mut *addr_of_mut!(FRAME) });
    if let Some(limits) = awg.frame_limits(0) {
        match ferrohal_stm32::timer::Frame::new(words, black_box(3), limits) {
            Ok(frame) => keep(awg.load_frame(black_box(0), frame).map_err(|r| r.error())),
            Err(refused) => keep(refused.error()),
        }
    }
    keep(awg.play());
    keep(awg.listen_on_end());
    keep(awg.on_interrupt());
    awg.abort();
    keep(awg.take_frame(black_box(0)).map(ferrohal_stm32::timer::Frame::into_words));
    // SAFETY: as TX and RX in `main`.
    let lent = black_box(unsafe { &mut *addr_of_mut!(LENT) });
    keep(awg.lend_samples(black_box(0), lent).map_err(|r| r.error()));
    keep(awg.prepare_listen(black_box(Hertz::mhz(1)), black_box(4)));
    keep(awg.prepared());
    keep(awg.reader_of(black_box(0)));
    keep(awg.is_listening());
    keep(awg.take_samples(black_box(0)));
}

/// The three reader forms: a reader armed ahead on the lane timer's update
/// and on a compare request no lane uses, and a reader paced by a timer no
/// lane uses. Every stream reaches GPIO through DMAMUX on the H7, so one
/// chip links all three.
#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
#[allow(clippy::too_many_arguments)]
pub fn exercise_readers<T, P, U, const CD: u8, const SN: u8, SA, SC, SP, S2, S3, const PORT: char>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
    ahead: SA,
    compare: SC,
    pacer: U,
    paced: SP,
    second: (S2, S3),
) where
    T: ferrohal_stm32::timer::ChannelTimer,
    U: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>:
        ferrohal_stm32::timer::CompareDma<T, 1> + ferrohal_stm32::timer::GpioDma,
    SA: ferrohal_stm32::timer::UpdateDma<T> + ferrohal_stm32::timer::GpioDma,
    SC: ferrohal_stm32::timer::CompareDma<T, 2> + ferrohal_stm32::timer::GpioDma,
    SP: ferrohal_stm32::timer::UpdateDma<U> + ferrohal_stm32::timer::GpioDma,
    S2: ferrohal_stm32::timer::UpdateDma<T> + ferrohal_stm32::timer::GpioDma,
    S3: ferrohal_stm32::timer::CompareDma<T, 3> + ferrohal_stm32::timer::GpioDma,
    ferrohal_stm32::gpio::Port<PORT>: ferrohal_stm32::gpio::PortInstance,
{
    let pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    let pace = retried_pwm(rcc, pacer, Hertz::mhz(2));
    let wiring =
        ferrohal_stm32::timer::LaneWiring::<T, 1, P, ferrohal_stm32::dma::Stream<CD, SN>>::new(
            pin, stream,
        );
    // SAFETY: as TX and RX in `main`.
    let (buffer, reads) = black_box(unsafe {
        (
            &mut *addr_of_mut!(READER_LANE),
            &mut *addr_of_mut!(READER_READS),
        )
    });
    let Ok(lane) = wiring.assemble_two_way::<PORT>(&pwm, buffer, reads) else {
        park()
    };
    let mut awg = ferrohal_stm32::timer::Awg::single(pwm, (lane,));
    keep(awg.read_ahead_first_update::<_, PORT>(ahead));
    keep(awg.read_ahead_first_compare::<2, _, PORT>(compare));
    keep(awg.read_ahead_paced::<_, _, PORT>(&pace, paced));
    // A one-timer generator has no second timer: both are refused, linked.
    keep(awg.read_ahead_second_update::<_, PORT>(second.0));
    keep(awg.read_ahead_second_compare::<3, _, PORT>(second.1));
    keep(awg.prepare_listen(black_box(Hertz::mhz(1)), black_box(4)));
    keep(awg.listen());
    keep(awg.speak());
    awg.abort();
}

/// A complementary output (CH1N) of a timer with a break unit, assembled
/// from its wiring.
pub fn exercise_complementary<B, N, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: B,
    pin: N,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
) where
    B: ferrohal_stm32::timer::ChannelTimer,
    N: ferrohal_stm32::timer::ComplementaryPin<B, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<B, 1>,
{
    let mut pwm = retried_pwm(rcc, timer, Hertz::khz(1));
    keep(pwm.set_rate(black_box(Hertz::khz(2))));
    // The plain complementary channel first, freed back to its pin.
    let channel = pwm.complementary::<1, N>(pin);
    let pin = channel.free();
    let wiring = ferrohal_stm32::timer::ComplementaryLaneWiring::<
        B,
        1,
        N,
        ferrohal_stm32::dma::Stream<CD, SN>,
    >::new(pin, stream);
    // SAFETY: as TX and RX in `main`.
    let buffer = black_box(unsafe { &mut *addr_of_mut!(COMPLEMENT) });
    let mut lane = match wiring.assemble(&mut pwm, buffer) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (wiring, buffer) = refused.into_parts();
            match wiring.assemble(&mut pwm, buffer) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    lane.set_duty(&pwm, black_box(10));
    pwm.set_output::<1>(black_box(false));
    pwm.set_output::<1>(black_box(true));
    pwm.start();
    pwm.start_one_pulse();
    lane.abort(&pwm);
    keep(pwm.free());
}

/// A sampler of port B through every operation.
pub fn exercise_sampler<U, S>(rcc: &mut ferrohal_stm32::rcc::Rcc, timer: U, stream: S)
where
    U: ferrohal_stm32::timer::Instance,
    S: ferrohal_stm32::timer::UpdateDma<U> + ferrohal_stm32::timer::GpioDma,
{
    // SAFETY: as TX and RX in `main`.
    let samples = black_box(unsafe { &mut *addr_of_mut!(SAMPLES) });
    let mut sampler = match ferrohal_stm32::timer::Sampler::new::<'B'>(
        rcc,
        timer,
        stream,
        black_box(Hertz::mhz(4)),
        samples,
    ) {
        Ok(sampler) => sampler,
        Err(refused) => {
            keep(refused.error());
            let (timer, stream, samples) = refused.into_parts();
            match ferrohal_stm32::timer::Sampler::new::<'B'>(
                rcc,
                timer,
                stream,
                black_box(Hertz::mhz(4)),
                samples,
            ) {
                Ok(sampler) => sampler,
                Err(_) => park(),
            }
        }
    };
    keep(sampler.rate());
    keep(sampler.start(black_box(32)));
    keep(sampler.on_interrupt());
    keep(ferrohal_stm32::Service::on_interrupt(&mut sampler));
    keep(sampler.samples().len());
    keep(sampler.vectors().verify());
    keep(sampler.set_rate(black_box(Hertz::mhz(2))));
    sampler.abort();
    keep(sampler.free());
}

/// The watchdog and the reset cause.
pub fn exercise_watchdog(rcc: &mut ferrohal_stm32::rcc::Rcc, iwdg: ferrohal_stm32::chip::IWDG) {
    use ferrohal_stm32::{
        iwdg::{Watchdog, divider, divider_with_lsi, window_divider},
        reset,
        time::Hertz,
    };

    keep(rcc.reset_cause());
    keep(ferrohal_stm32::signature::device_uid());
    keep(reset::peek_cause());
    keep(reset::bootloader_requested());
    reset::enter_bootloader_if_requested();
    if black_box(false) {
        reset::reboot_to_bootloader();
    }
    keep(divider(black_box(50_000)));
    keep(divider_with_lsi(
        black_box(50_000),
        ferrohal_stm32::time::Hertz::hz(30_000),
    ));
    keep(window_divider(black_box(50_000), black_box(10_000)));
    keep(Watchdog::check(black_box(50_000)));
    // Both refusals hand the token back before the watchdog is touched, so
    // those paths are exercised and `start` still has the token.
    let iwdg = match Watchdog::start_with_lsi(iwdg, black_box(50_000), Hertz::hz(0)) {
        Ok(_) => park(),
        Err(refused) => {
            keep(refused.error());
            refused.into_parts()
        }
    };
    #[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
    let iwdg = match Watchdog::start_windowed(iwdg, black_box(50_000), black_box(60_000)) {
        Ok(_) => park(),
        Err(refused) => {
            keep(refused.error());
            refused.into_parts()
        }
    };
    let dog = match Watchdog::start(iwdg, black_box(50_000)) {
        Ok(dog) => dog,
        Err(refused) => {
            keep(refused.error());
            let iwdg = refused.into_parts();
            match Watchdog::start(iwdg, black_box(50_000)) {
                Ok(dog) => dog,
                Err(_) => park(),
            }
        }
    };
    dog.feed();
    keep(dog.timeout_us());
    keep(dog.handle().timeout_us());
    keep(Watchdog::is_frozen_on_halt());
}

/// The polled SPI through every operation.
pub fn exercise_blocking<U, K, I, O>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    wiring: spi::BlockingWiring<U, K, I, O>,
) where
    U: spi::Instance,
    K: spi::SckPin<U> + ferrohal_stm32::gpio::AltPin,
    I: spi::MisoPin<U> + ferrohal_stm32::gpio::AltPin,
    O: spi::MosiPin<U> + ferrohal_stm32::gpio::AltPin,
{
    use embedded_hal::spi::SpiBus;

    black_box(&spi::BlockingWiring::<U, K, I, O>::RESOURCES)
        .iter()
        .for_each(keep);
    // A refusal hands back the wiring: retry once with it, at 1 MHz.
    let mut bus =
        match spi::BlockingSpi::new(rcc, wiring, &black_box(spi::Config::new(Hertz::mhz(10)))) {
            Ok(bus) => bus,
            Err(refused) => {
                keep(refused.error());
                let wiring = refused.into_parts();
                let Ok(bus) =
                    spi::BlockingSpi::new(rcc, wiring, &black_box(spi::Config::new(Hertz::mhz(1))))
                else {
                    return;
                };
                bus
            }
        };
    let data = black_box(&[0x5au8; 300][..]);
    let mut buf = [0u8; 300];
    keep(SpiBus::read(&mut bus, black_box(&mut buf)));
    keep(SpiBus::write(&mut bus, data));
    keep(SpiBus::transfer(&mut bus, black_box(&mut buf[..10]), data));
    keep(SpiBus::transfer_in_place(&mut bus, black_box(&mut buf)));
    keep(SpiBus::flush(&mut bus));
    keep(bus.set_config(&black_box(
        spi::Config::new(Hertz::khz(400)).with_mode(spi::MODE_3),
    )));
    keep(bus.frequency());
    keep(bus.name());
    keep(bus.kernel_clock());
    bus.set_clock(Some(&CLOCK));
    keep(SpiBus::write(&mut bus, data));
    keep(bus.free());
}

/// The I2C through every operation of the bus trait, and the empty
/// operation inside a transaction.
fn exercise_i2c<W: i2c::Parts>(bus: &mut I2c<W>) {
    use embedded_hal::i2c::{I2c as HalI2c, Operation};

    let data = black_box(&[0x5au8; 4][..]);
    let mut buf = [0u8; 4];
    keep(HalI2c::read(bus, black_box(0x18), black_box(&mut buf)));
    keep(HalI2c::write(bus, black_box(0x18), data));
    keep(HalI2c::write_read(
        bus,
        black_box(0x18),
        data,
        black_box(&mut buf),
    ));
    keep(HalI2c::transaction(
        bus,
        black_box(0x18),
        black_box(&mut [
            Operation::Write(data),
            Operation::Read(&mut buf),
            Operation::Write(&data[..0]),
            Operation::Write(&data[..0]),
            Operation::Write(data),
        ]),
    ));
    keep(bus.frequency());
    keep(bus.kernel_clock());
    keep(bus.name());
    keep(bus.health());
    bus.set_clock(Some(&CLOCK));
    keep(HalI2c::write(bus, black_box(0x18), data));
    bus.set_clock(None);
    keep(HalI2c::write(bus, black_box(0x18), data));
}

/// The USB clock check alone.
#[cfg(any(
    feature = "stm32f401re",
    feature = "stm32f410rb",
    feature = "stm32f411ce",
    feature = "stm32f412zg",
    feature = "stm32f413zh",
    feature = "stm32f446re"
))]
fn keep_usb_clock(rcc: &mut ferrohal_stm32::rcc::Rcc) {
    keep(rcc.usb_clock());
}

/// USB's own part: the clock and the constructor. The controller's driver is
/// third-party and sits outside this check.
#[cfg(not(any(
    feature = "stm32f401re",
    feature = "stm32f410rb",
    feature = "stm32f411ce",
    feature = "stm32f412zg",
    feature = "stm32f413zh",
    feature = "stm32f446re"
)))]
pub fn exercise_usb(rcc: &mut ferrohal_stm32::rcc::Rcc, wiring: ferrohal_stm32::usb::Wiring) {
    use ferrohal_stm32::usb::{UsbFs, Wiring};

    keep(UsbFs::vectors().verify());
    black_box(&Wiring::RESOURCES).iter().for_each(keep);
    keep(rcc.usb_clock());
    let usb = match UsbFs::new(rcc, wiring) {
        Ok(usb) => usb,
        Err(refused) => {
            keep(refused.error());
            match UsbFs::new(rcc, refused.into_parts()) {
                Ok(usb) => usb,
                Err(_) => park(),
            }
        }
    };
    // Freed, and reopened from what came back.
    let usb = match UsbFs::new(rcc, usb.free()) {
        Ok(usb) => usb,
        Err(_) => park(),
    };
    keep(usb);
}

fn exercise_spi<W: ferrohal_stm32::spi::Parts>(bus: &mut Spi<W>) {
    use embedded_hal::spi::SpiBus;

    let data = black_box(&[0xa5u8; 40][..]);
    let mut buf = [0u8; 40];
    keep(bus.start(data));
    keep(bus.on_interrupt());
    keep(ferrohal_stm32::Service::on_interrupt(bus));
    keep(bus.received().len());
    keep(bus.is_busy());
    bus.abort();
    keep(bus.transfer(data).map(<[u8]>::len));
    keep(SpiBus::read(bus, black_box(&mut buf)));
    keep(SpiBus::write(bus, data));
    keep(SpiBus::transfer(bus, black_box(&mut buf), data));
    keep(SpiBus::transfer_in_place(bus, black_box(&mut buf)));
    keep(SpiBus::flush(bus));
    keep(bus.set_config(&black_box(spi::Config::new(Hertz::khz(400)))));
    keep(bus.frequency());
    keep(bus.capacity());
    keep(bus.name());
    keep(bus.kernel_clock());
    keep(bus.health());
    keep(bus.vectors().verify());
    bus.set_clock(Some(&CLOCK));
    keep(bus.transfer(data).map(<[u8]>::len));
}

/// Hands a result to the optimiser as used: the check is about which code
/// paths exist, not what they return.
fn keep<T>(value: T) {
    black_box(value);
}

/// A caller clock for the check; any cycle source will do.
struct Clock;

impl uart::CycleClock for Clock {
    fn cycles(&self) -> u32 {
        black_box(0)
    }
}

static CLOCK: Clock = Clock;

fn park() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

/// A DMA stream the UART does not use, for the raw DMA API.
struct Spare<C: StreamInstance> {
    stream: Option<C>,
}

impl<C: StreamInstance> Spare<C> {
    fn take_stream(&mut self) -> ferrohal_stm32::dma::AnyStream {
        match self.stream.take() {
            Some(stream) => stream.into_any(),
            None => park(),
        }
    }
}

#[cfg(any(
    feature = "stm32f401re",
    feature = "stm32f411ce",
    feature = "stm32f412zg",
    feature = "stm32f413zh",
    feature = "stm32f446re"
))]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI2, USART1, USART6},
        dma::Stream,
        gpio::{Alternate, Input, OpenDrain, Output, Pin, Pull, Speed},
        i2c,
        rcc::{AhbPrescaler, ApbPrescaler, Config, Pll, PllP, PllSource, Rcc, Supply, Sysclk},
        spi,
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = USART6;
    #[cfg(not(feature = "stm32f446re"))]
    pub type RxOnlyWiring = RxWiring<USART6, Pin<'A', 12, Alternate<8>>, Stream<2, 1>>;
    /// The F446 bonds USART6 RX to PC7 and PG9 only (DS10693 Table 11).
    #[cfg(feature = "stm32f446re")]
    pub type RxOnlyWiring = RxWiring<USART6, Pin<'C', 7, Alternate<8>>, Stream<2, 1>>;
    pub type SpiUnit = SPI2;
    pub type I2cUnit = I2C1;
    pub type SpiWiring = spi::Wiring<
        SPI2,
        Pin<'B', 13, Alternate<5>>,
        Pin<'B', 14, Alternate<5>>,
        Pin<'B', 15, Alternate<5>>,
        Stream<1, 4>,
        Stream<1, 3>,
    >;

    /// I2C1 on PB8 and PB9, the pins the Nucleo bench's Arduino connector
    /// carries.
    pub type I2cWiring = i2c::Wiring<
        I2C1,
        Pin<'B', 8, Alternate<4, OpenDrain>>,
        Pin<'B', 9, Alternate<4, OpenDrain>>,
    >;

    const CLOCKS: Config = Config {
        hse: None,
        pll: Some(Pll {
            source: PllSource::Hsi,
            prediv: 16,
            mul: 336,
            divp: PllP::Div4,
            divq: Some(7),
        }),
        sys: Sysclk::PllP,
        ahb_pre: AhbPrescaler::Div1,
        apb1_pre: ApbPrescaler::Div2,
        apb2_pre: ApbPrescaler::Div1,
        supply: Supply::V2_7To3_6,
    };

    pub type UartWiring = Wiring<
        USART1,
        Pin<'A', 9, Alternate<7>>,
        Pin<'A', 10, Alternate<7>>,
        Stream<2, 7>,
        Stream<2, 2>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        SpiWiring,
        I2cWiring,
        Spare<Stream<2, 0>>,
        Pin<'A', 5, Output>,
        Pin<'A', 0, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let mut b = p.GPIOB.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        // A second set of tokens for the exercises that need a stream the
        // wiring below does not take. The build never runs, so the aliasing
        // exists only in the linked code.
        let spare1 = unsafe { Peripherals::steal() }.DMA1.split(&mut rcc);
        let spare2 = unsafe { Peripherals::steal() }.DMA2.split(&mut rcc);
        let mut tx = a.p9.into_alternate::<7>(&mut a.ctrl);
        tx.set_speed(&mut a.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = a.p10.into_alternate::<7>(&mut a.ctrl);
        rx.set_pull(&mut a.ctrl, core::hint::black_box(Pull::Up));
        let out = a.p5.into_push_pull_output(&mut a.ctrl);
        let input = a.p0.into_input(&mut a.ctrl);
        #[cfg(not(feature = "stm32f446re"))]
        let rx6 = a.p12.into_alternate::<8>(&mut a.ctrl);
        #[cfg(feature = "stm32f446re")]
        let rx6 = {
            let mut c = p.GPIOC.split(&mut rcc);
            c.p7.into_alternate::<8>(&mut c.ctrl)
        };
        let analog = a.p1.into_analog(&mut a.ctrl);
        let mut exti = p.EXTI.split(p.SYSCFG.split(&mut rcc).exticr);
        super::exercise_exti::<'A', 1>(&mut exti.ctrl, exti.l1);
        super::exercise_timers(&mut rcc, p.TIM10, p.TIM5);
        super::exercise_watchdog(&mut rcc, p.IWDG);
        super::exercise_adc(
            &mut rcc,
            p.ADC1,
            a.p4.into_analog(&mut a.ctrl),
            dma2.s4,
            super::adc_buffer(),
        );
        super::exercise_adc_internal(&mut rcc, unsafe { Peripherals::steal() }.ADC1);
        super::exercise_trigger(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            unsafe { Peripherals::steal() }.TIM3,
        );
        super::exercise_two_way::<_, _, _, _, 'A'>(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            a.p8.into_alternate::<1>(&mut a.ctrl),
            spare2.s1,
        );
        // PA12 is USART6's on the F401 and F411, so the constructor is left
        // to the F405, which shares this code; the F4's clock check runs on
        // its own.
        super::keep_usb_clock(&mut rcc);
        super::exercise_blocking(
            &mut rcc,
            spi::BlockingWiring {
                spi: p.SPI3,
                sck: b.p3.into_alternate::<6>(&mut b.ctrl),
                miso: b.p4.into_alternate::<6>(&mut b.ctrl),
                mosi: b.p5.into_alternate::<6>(&mut b.ctrl),
            },
        );
        // An open-drain alternate, as I2C.s shared lines are wired.
        super::keep(b.p7.into_alternate_open_drain::<1>(&mut b.ctrl));
        super::exercise_lane(
            &mut rcc,
            p.TIM4,
            b.p6.into_alternate::<2>(&mut b.ctrl),
            dma1.s0,
        );
        super::exercise_lane_free(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM2,
            a.p15.into_alternate::<1>(&mut a.ctrl),
            spare1.s5,
        );
        // TIM1 is the F401's only break-unit timer and its only update
        // request on DMA2, so it samples here and the F405 covers CHxN.
        super::exercise_sampler(&mut rcc, p.TIM1, dma2.s5);
        let _ = analog;
        Some((
            rcc,
            Wiring {
                uart: p.USART1,
                tx,
                rx,
                tx_dma: dma2.s7,
                rx_dma: dma2.s2,
            },
            RxWiring {
                uart: p.USART6,
                rx: rx6,
                rx_dma: dma2.s1,
            },
            spi::Wiring {
                spi: p.SPI2,
                sck: b.p13.into_alternate::<5>(&mut b.ctrl),
                miso: b.p14.into_alternate::<5>(&mut b.ctrl),
                mosi: b.p15.into_alternate::<5>(&mut b.ctrl),
                tx_dma: dma1.s4,
                rx_dma: dma1.s3,
            },
            i2c::Wiring {
                i2c: p.I2C1,
                scl: b.p8.into_alternate_open_drain::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate_open_drain::<4>(&mut b.ctrl),
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

/// The STM32F410RB: no USB port, no TIM2 to TIM4 and no trigger route, so
/// it takes its own wiring (RM0401; pins and streams from its glue).
#[cfg(feature = "stm32f410rb")]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI2, USART1, USART6},
        dma::Stream,
        gpio::{Alternate, Input, OpenDrain, Output, Pin, Pull, Speed},
        i2c,
        rcc::{AhbPrescaler, ApbPrescaler, Config, Pll, PllP, PllSource, Rcc, Supply, Sysclk},
        spi,
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = USART6;
    pub type RxOnlyWiring = RxWiring<USART6, Pin<'A', 12, Alternate<8>>, Stream<2, 1>>;
    pub type SpiUnit = SPI2;
    pub type I2cUnit = I2C1;
    pub type SpiWiring = spi::Wiring<
        SPI2,
        Pin<'B', 13, Alternate<5>>,
        Pin<'B', 14, Alternate<5>>,
        Pin<'B', 15, Alternate<5>>,
        Stream<1, 4>,
        Stream<1, 3>,
    >;
    pub type I2cWiring = i2c::Wiring<
        I2C1,
        Pin<'B', 8, Alternate<4, OpenDrain>>,
        Pin<'B', 9, Alternate<4, OpenDrain>>,
    >;

    /// 100 MHz from HSI: a 400 MHz VCO, P 4; APB1 at 50 MHz.
    const CLOCKS: Config = Config {
        hse: None,
        pll: Some(Pll {
            source: PllSource::Hsi,
            prediv: 16,
            mul: 400,
            divp: PllP::Div4,
            divq: None,
        }),
        sys: Sysclk::PllP,
        ahb_pre: AhbPrescaler::Div1,
        apb1_pre: ApbPrescaler::Div2,
        apb2_pre: ApbPrescaler::Div1,
        supply: Supply::V2_7To3_6,
    };

    pub type UartWiring = Wiring<
        USART1,
        Pin<'A', 9, Alternate<7>>,
        Pin<'A', 10, Alternate<7>>,
        Stream<2, 7>,
        Stream<2, 2>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        SpiWiring,
        I2cWiring,
        Spare<Stream<2, 0>>,
        Pin<'A', 5, Output>,
        Pin<'C', 13, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let mut b = p.GPIOB.split(&mut rcc);
        let mut c = p.GPIOC.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        // A second set of tokens for the exercises that need a stream the
        // wiring below does not take. The build never runs, so the aliasing
        // exists only in the linked code.
        let spare1 = unsafe { Peripherals::steal() }.DMA1.split(&mut rcc);
        let spare2 = unsafe { Peripherals::steal() }.DMA2.split(&mut rcc);
        let mut tx = a.p9.into_alternate::<7>(&mut a.ctrl);
        tx.set_speed(&mut a.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = a.p10.into_alternate::<7>(&mut a.ctrl);
        rx.set_pull(&mut a.ctrl, core::hint::black_box(Pull::Up));
        let out = a.p5.into_push_pull_output(&mut a.ctrl);
        let input = c.p13.into_input(&mut c.ctrl);
        let rx6 = a.p12.into_alternate::<8>(&mut a.ctrl);
        let analog = a.p1.into_analog(&mut a.ctrl);
        let mut exti = p.EXTI.split(p.SYSCFG.split(&mut rcc).exticr);
        super::exercise_exti::<'A', 1>(&mut exti.ctrl, exti.l1);
        super::exercise_timers(&mut rcc, p.TIM11, p.TIM5);
        super::exercise_watchdog(&mut rcc, p.IWDG);
        super::exercise_adc(
            &mut rcc,
            p.ADC1,
            a.p4.into_analog(&mut a.ctrl),
            dma2.s4,
            super::adc_buffer(),
        );
        super::exercise_adc_internal(&mut rcc, unsafe { Peripherals::steal() }.ADC1);
        super::exercise_two_way::<_, _, _, _, 'A'>(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            a.p8.into_alternate::<1>(&mut a.ctrl),
            spare2.s1,
        );
        super::keep_usb_clock(&mut rcc);
        super::exercise_blocking(
            &mut rcc,
            spi::BlockingWiring {
                spi: p.SPI1,
                sck: b.p3.into_alternate::<5>(&mut b.ctrl),
                miso: b.p4.into_alternate::<5>(&mut b.ctrl),
                mosi: b.p5.into_alternate::<5>(&mut b.ctrl),
            },
        );
        // An open-drain alternate, as I2C.s shared lines are wired.
        super::keep(b.p7.into_alternate_open_drain::<1>(&mut b.ctrl));
        super::exercise_lane(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM5,
            b.p12.into_alternate::<2>(&mut b.ctrl),
            dma1.s2,
        );
        super::exercise_lane_free(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM5,
            a.p0.into_alternate::<2>(&mut a.ctrl),
            spare1.s2,
        );
        super::exercise_sampler(&mut rcc, p.TIM1, dma2.s5);
        let _ = analog;
        Some((
            rcc,
            Wiring {
                uart: p.USART1,
                tx,
                rx,
                tx_dma: dma2.s7,
                rx_dma: dma2.s2,
            },
            RxWiring {
                uart: p.USART6,
                rx: rx6,
                rx_dma: dma2.s1,
            },
            spi::Wiring {
                spi: p.SPI2,
                sck: b.p13.into_alternate::<5>(&mut b.ctrl),
                miso: b.p14.into_alternate::<5>(&mut b.ctrl),
                mosi: b.p15.into_alternate::<5>(&mut b.ctrl),
                tx_dma: dma1.s4,
                rx_dma: dma1.s3,
            },
            i2c::Wiring {
                i2c: p.I2C1,
                scl: b.p8.into_alternate_open_drain::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate_open_drain::<4>(&mut b.ctrl),
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

/// The Foxeer F405 V2's ports, from its 8 MHz crystal: HSE in the path. The
/// F407 is the same die, and bonds every pin these name; the F427VI and
/// F429ZI (die 0x419) carry the same IP versions and bond them too. The F7s
/// bond the same pins with the same alternate functions and DMA requests, so
/// their usart_v3, spi_v3 and i2c_v2 build through the same wiring.
#[cfg(any(
    feature = "stm32f405rg",
    feature = "stm32f407vg",
    feature = "stm32f427vi",
    feature = "stm32f429zi",
    feature = "stm32f722re",
    feature = "stm32f745vg",
    feature = "stm32f767zi"
))]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI2, UART4, USART2},
        dma::Stream,
        gpio::{Alternate, Input, OpenDrain, Output, Pin, Pull, Speed},
        i2c,
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Hse, HseMode, Pll, PllP, PllSource, Rcc, Supply,
            Sysclk,
        },
        spi,
        time::Hertz,
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = UART4;
    pub type RxOnlyWiring = RxWiring<UART4, Pin<'A', 1, Alternate<8>>, Stream<1, 2>>;
    pub type SpiUnit = SPI2;
    pub type I2cUnit = I2C1;
    /// The Foxeer's flash bus: PB13, PC2, PC3.
    pub type SpiWiring = spi::Wiring<
        SPI2,
        Pin<'B', 13, Alternate<5>>,
        Pin<'C', 2, Alternate<5>>,
        Pin<'C', 3, Alternate<5>>,
        Stream<1, 4>,
        Stream<1, 3>,
    >;

    /// I2C1 on PB8 and PB9, the bus the Foxeer's barometer sits on.
    pub type I2cWiring = i2c::Wiring<
        I2C1,
        Pin<'B', 8, Alternate<4, OpenDrain>>,
        Pin<'B', 9, Alternate<4, OpenDrain>>,
    >;

    const CLOCKS: Config = Config {
        hse: Some(Hse {
            freq: Hertz::mhz(8),
            mode: HseMode::Oscillator,
        }),
        pll: Some(Pll {
            source: PllSource::Hse,
            prediv: 8,
            mul: 336,
            divp: PllP::Div2,
            divq: Some(7),
        }),
        sys: Sysclk::PllP,
        ahb_pre: AhbPrescaler::Div1,
        apb1_pre: ApbPrescaler::Div4,
        apb2_pre: ApbPrescaler::Div2,
        supply: Supply::V2_7To3_6,
    };

    pub type UartWiring = Wiring<
        USART2,
        Pin<'A', 2, Alternate<7>>,
        Pin<'A', 3, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 5>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        SpiWiring,
        I2cWiring,
        Spare<Stream<2, 0>>,
        Pin<'B', 0, Output>,
        Pin<'B', 1, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let mut b = p.GPIOB.split(&mut rcc);
        let mut c = p.GPIOC.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        // A second set of tokens for the exercises that need a stream the
        // wiring below does not take. The build never runs, so the aliasing
        // exists only in the linked code.
        let spare1 = unsafe { Peripherals::steal() }.DMA1.split(&mut rcc);
        let spare2 = unsafe { Peripherals::steal() }.DMA2.split(&mut rcc);
        let mut tx = a.p2.into_alternate::<7>(&mut a.ctrl);
        tx.set_speed(&mut a.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = a.p3.into_alternate::<7>(&mut a.ctrl);
        rx.set_pull(&mut a.ctrl, core::hint::black_box(Pull::Up));
        let out = b.p0.into_push_pull_output(&mut b.ctrl);
        let input = b.p1.into_input(&mut b.ctrl);
        let rx4 = a.p1.into_alternate::<8>(&mut a.ctrl);
        let analog = b.p2.into_analog(&mut b.ctrl);
        let mut exti = p.EXTI.split(p.SYSCFG.split(&mut rcc).exticr);
        super::exercise_exti::<'B', 2>(&mut exti.ctrl, exti.l2);
        super::exercise_timers(&mut rcc, p.TIM6, p.TIM2);
        super::exercise_watchdog(&mut rcc, p.IWDG);
        super::exercise_adc(
            &mut rcc,
            p.ADC1,
            a.p4.into_analog(&mut a.ctrl),
            dma2.s4,
            super::adc_buffer(),
        );
        super::exercise_adc_internal(&mut rcc, unsafe { Peripherals::steal() }.ADC1);
        super::exercise_trigger(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            unsafe { Peripherals::steal() }.TIM3,
        );
        super::exercise_two_way::<_, _, _, _, 'A'>(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            a.p8.into_alternate::<1>(&mut a.ctrl),
            spare2.s1,
        );
        super::exercise_usb(
            &mut rcc,
            ferrohal_stm32::usb::Wiring {
                usb: p.USB_OTG_FS,
                dm: a.p11.into_alternate(&mut a.ctrl),
                dp: a.p12.into_alternate(&mut a.ctrl),
            },
        );
        super::exercise_blocking(
            &mut rcc,
            spi::BlockingWiring {
                spi: p.SPI3,
                sck: b.p3.into_alternate::<6>(&mut b.ctrl),
                miso: b.p4.into_alternate::<6>(&mut b.ctrl),
                mosi: b.p5.into_alternate::<6>(&mut b.ctrl),
            },
        );
        // An open-drain alternate, as I2C.s shared lines are wired.
        super::keep(b.p7.into_alternate_open_drain::<1>(&mut b.ctrl));
        super::exercise_lane(
            &mut rcc,
            p.TIM4,
            b.p6.into_alternate::<2>(&mut b.ctrl),
            dma1.s0,
        );
        super::exercise_lane_free(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM2,
            a.p15.into_alternate::<1>(&mut a.ctrl),
            spare1.s5,
        );
        super::exercise_complementary(
            &mut rcc,
            p.TIM8,
            a.p7.into_alternate::<3>(&mut a.ctrl),
            dma2.s2,
        );
        super::exercise_sampler(&mut rcc, p.TIM1, dma2.s5);
        let _ = analog;
        Some((
            rcc,
            Wiring {
                uart: p.USART2,
                tx,
                rx,
                tx_dma: dma1.s6,
                rx_dma: dma1.s5,
            },
            RxWiring {
                uart: p.UART4,
                rx: rx4,
                rx_dma: dma1.s2,
            },
            spi::Wiring {
                spi: p.SPI2,
                sck: b.p13.into_alternate::<5>(&mut b.ctrl),
                miso: c.p2.into_alternate::<5>(&mut c.ctrl),
                mosi: c.p3.into_alternate::<5>(&mut c.ctrl),
                tx_dma: dma1.s4,
                rx_dma: dma1.s3,
            },
            i2c::Wiring {
                i2c: p.I2C1,
                scl: b.p8.into_alternate_open_drain::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate_open_drain::<4>(&mut b.ctrl),
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI1, USART2, USART3},
        dma::Stream,
        gpio::{Alternate, Input, OpenDrain, Output, Pin, Pull, Speed},
        i2c,
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Pll, PllSource, Rcc, Supply, Sysclk, VoltageScale,
        },
        spi,
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = USART3;
    pub type RxOnlyWiring = RxWiring<USART3, Pin<'B', 11, Alternate<7>>, Stream<1, 2>>;
    pub type SpiUnit = SPI1;
    pub type I2cUnit = I2C1;
    /// The Lucid's IMU bus.
    pub type SpiWiring = spi::Wiring<
        SPI1,
        Pin<'A', 5, Alternate<5>>,
        Pin<'A', 6, Alternate<5>>,
        Pin<'D', 7, Alternate<5>>,
        Stream<1, 5>,
        Stream<1, 4>,
    >;

    /// I2C1 on PB8 and PB9, the pins the Nucleo-H753ZI's bench carries.
    pub type I2cWiring = i2c::Wiring<
        I2C1,
        Pin<'B', 8, Alternate<4, OpenDrain>>,
        Pin<'B', 9, Alternate<4, OpenDrain>>,
    >;

    const CLOCKS: Config = Config {
        hse: None,
        pll1: Some(Pll {
            source: PllSource::Hsi,
            prediv: 4,
            mul: 50,
            divp: Some(2),
            divq: Some(8),
            divr: None,
        }),
        sys: Sysclk::Pll1P,
        cpu_pre: AhbPrescaler::Div1,
        ahb_pre: AhbPrescaler::Div2,
        apb1_pre: ApbPrescaler::Div2,
        apb2_pre: ApbPrescaler::Div2,
        apb3_pre: ApbPrescaler::Div2,
        apb4_pre: ApbPrescaler::Div2,
        supply: Supply::Ldo,
        voltage_scale: VoltageScale::Scale1,
    };

    pub type UartWiring = Wiring<
        USART2,
        Pin<'D', 5, Alternate<7>>,
        Pin<'D', 6, Alternate<7>>,
        Stream<1, 1>,
        Stream<1, 0>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        SpiWiring,
        I2cWiring,
        Spare<Stream<2, 3>>,
        Pin<'B', 0, Output>,
        Pin<'B', 1, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let mut b = p.GPIOB.split(&mut rcc);
        let mut d = p.GPIOD.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        // A second set of tokens for the exercises that need a stream the
        // wiring below does not take. The build never runs, so the aliasing
        // exists only in the linked code.
        let spare1 = unsafe { Peripherals::steal() }.DMA1.split(&mut rcc);
        let spare2 = unsafe { Peripherals::steal() }.DMA2.split(&mut rcc);
        let mut tx = d.p5.into_alternate::<7>(&mut d.ctrl);
        tx.set_speed(&mut d.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = d.p6.into_alternate::<7>(&mut d.ctrl);
        rx.set_pull(&mut d.ctrl, core::hint::black_box(Pull::Up));
        let out = b.p0.into_push_pull_output(&mut b.ctrl);
        let input = b.p1.into_input(&mut b.ctrl);
        let rx3 = b.p11.into_alternate::<7>(&mut b.ctrl);
        let analog = b.p2.into_analog(&mut b.ctrl);
        let mut exti = p.EXTI.split(p.SYSCFG.split(&mut rcc).exticr);
        super::exercise_exti::<'B', 2>(&mut exti.ctrl, exti.l2);
        super::exercise_timers(&mut rcc, p.TIM6, p.TIM2);
        super::exercise_watchdog(&mut rcc, p.IWDG1);
        super::exercise_adc(
            &mut rcc,
            p.ADC1,
            a.p4.into_analog(&mut a.ctrl),
            dma2.s4,
            super::adc_buffer(),
        );
        super::exercise_adc_internal(&mut rcc, p.ADC3);
        {
            let mut c = p.GPIOC.split(&mut rcc);
            super::exercise_sdmmc(
                &mut rcc,
                ferrohal_stm32::sdmmc::Wiring {
                    sdmmc: p.SDMMC1,
                    ck: c.p12.into_alternate::<12>(&mut c.ctrl),
                    cmd: d.p2.into_alternate::<12>(&mut d.ctrl),
                    d0: c.p8.into_alternate::<12>(&mut c.ctrl),
                    d1: c.p9.into_alternate::<12>(&mut c.ctrl),
                    d2: c.p10.into_alternate::<12>(&mut c.ctrl),
                    d3: c.p11.into_alternate::<12>(&mut c.ctrl),
                },
            );
        }
        super::exercise_trigger(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            unsafe { Peripherals::steal() }.TIM8,
        );
        super::exercise_two_way::<_, _, _, _, 'A'>(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM1,
            a.p8.into_alternate::<1>(&mut a.ctrl),
            spare2.s1,
        );
        super::exercise_usb(
            &mut rcc,
            ferrohal_stm32::usb::Wiring {
                usb: p.USB_OTG_FS,
                dm: a.p11.into_alternate(&mut a.ctrl),
                dp: a.p12.into_alternate(&mut a.ctrl),
            },
        );
        super::exercise_blocking(
            &mut rcc,
            spi::BlockingWiring {
                spi: p.SPI3,
                sck: b.p3.into_alternate::<6>(&mut b.ctrl),
                miso: b.p4.into_alternate::<6>(&mut b.ctrl),
                mosi: b.p5.into_alternate::<7>(&mut b.ctrl),
            },
        );
        // An open-drain alternate, as I2C's shared lines are wired.
        super::keep(b.p7.into_alternate_open_drain::<1>(&mut b.ctrl));
        super::exercise_lane(
            &mut rcc,
            p.TIM4,
            b.p6.into_alternate::<2>(&mut b.ctrl),
            dma2.s0,
        );
        super::exercise_lane_free(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM2,
            a.p15.into_alternate::<1>(&mut a.ctrl),
            spare1.s5,
        );
        super::exercise_complementary(
            &mut rcc,
            p.TIM1,
            a.p7.into_alternate::<1>(&mut a.ctrl),
            dma1.s3,
        );
        super::exercise_sampler(&mut rcc, p.TIM8, dma2.s1);
        super::exercise_readers::<_, _, _, 2, 2, _, _, _, _, _, 'A'>(
            &mut rcc,
            p.TIM5,
            a.p0.into_alternate::<2>(&mut a.ctrl),
            dma2.s2,
            dma2.s5,
            dma2.s6,
            p.TIM3,
            dma2.s7,
            (dma1.s6, dma1.s7),
        );
        let _ = analog;
        Some((
            rcc,
            Wiring {
                uart: p.USART2,
                tx,
                rx,
                tx_dma: dma1.s1,
                rx_dma: dma1.s0,
            },
            RxWiring {
                uart: p.USART3,
                rx: rx3,
                rx_dma: dma1.s2,
            },
            spi::Wiring {
                spi: p.SPI1,
                sck: a.p5.into_alternate::<5>(&mut a.ctrl),
                miso: a.p6.into_alternate::<5>(&mut a.ctrl),
                mosi: d.p7.into_alternate::<5>(&mut d.ctrl),
                tx_dma: dma1.s5,
                rx_dma: dma1.s4,
            },
            i2c::Wiring {
                i2c: p.I2C1,
                scl: b.p8.into_alternate_open_drain::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate_open_drain::<4>(&mut b.ctrl),
            },
            Spare {
                stream: Some(dma2.s3),
            },
            out,
            input,
        ))
    }
}

#[cfg(not(feature = "locate"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe extern "C" {
        fn ferrohal_panic_is_reachable() -> !;
    }
    // SAFETY: never linked: if this call survives optimisation, the build
    // fails, which is the point.
    unsafe { ferrohal_panic_is_reachable() }
}

/// With `--features locate`, the build links, so the disassembly shows which
/// functions still call into `core::panicking`.
#[cfg(feature = "locate")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    park()
}

/// The ADC, one channel at a time and as a sequence over DMA.
pub fn exercise_adc<A, P, S>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    token: A,
    pin: P,
    stream: S,
    buffer: &'static mut [u16],
) where
    A: ferrohal_stm32::adc::Instance,
    P: ferrohal_stm32::adc::AdcPin<A>,
    S: ferrohal_stm32::adc::Dma<A>,
{
    use ferrohal_stm32::adc::{Adc, Config, Wiring};

    // The wiring as a board states it, taken apart again.
    let wiring = Wiring::<A, P, S>::new(pin, stream);
    let (pin, stream) = (wiring.pin, wiring.stream);
    keep(Config::new(black_box(480)));
    let mut adc = match Adc::new(rcc, token, &Config::slow_source()) {
        Ok(adc) => adc,
        Err(refused) => {
            keep(refused.error());
            let token = refused.into_parts();
            match Adc::new(rcc, token, &Config::slow_source()) {
                Ok(adc) => adc,
                Err(_) => park(),
            }
        }
    };
    let channel = adc.channel(&pin);
    keep(channel.number());
    keep(ferrohal_stm32::adc::Channel::of(&pin).number());
    keep(adc.read(channel));
    keep(adc.max_sample());
    keep(adc.to_millivolts(black_box(1), black_box(3300)));
    let mut scan = match adc.into_scan(stream, &[channel, channel], buffer) {
        Ok(scan) => scan,
        Err(refused) => {
            keep(refused.error());
            let (adc, stream, buffer) = refused.into_parts();
            match adc.into_scan(stream, &[channel, channel], buffer) {
                Ok(scan) => scan,
                Err(_) => park(),
            }
        }
    };
    keep(scan.start());
    keep(ferrohal_stm32::Service::on_interrupt(&mut scan));
    keep(scan.on_interrupt());
    keep(scan.samples().first().copied());
    scan.abort();
    keep(scan.is_busy());
    keep(scan.vectors().verify());
    keep(scan.max_sample());
    keep(scan.to_millivolts(black_box(1), black_box(3300)));
    // Freed back to the parts, a second scan from them, then the ADC freed
    // and reopened from its token.
    let Ok((adc, stream, buffer)) = scan.free() else {
        park()
    };
    let scan = match adc.into_scan(stream, &[channel], buffer) {
        Ok(scan) => scan,
        Err(_) => park(),
    };
    let Ok((adc, _stream, _buffer)) = scan.free() else {
        park()
    };
    let token = adc.free();
    match Adc::new(rcc, token, &Config::slow_source()) {
        Ok(adc) => keep(adc.free()),
        Err(_) => park(),
    }
}

/// The internal channels, on the ADC that has them.
pub fn exercise_adc_internal<A: ferrohal_stm32::adc::InternalChannels>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    token: A,
) {
    use ferrohal_stm32::adc::{Adc, Config};

    let Ok(mut adc) = Adc::new(rcc, token, &Config::slow_source()) else {
        return;
    };
    keep(adc.vdda_millivolts());
    keep(adc.temperature_decidegrees());
    keep(adc.vrefint_channel().number());
    keep(adc.temperature_channel().number());
    keep(ferrohal_stm32::adc::Channel::<A>::vrefint().number());
    keep(ferrohal_stm32::adc::Channel::<A>::temperature().number());
}

static mut ADC_BUFFER: [u16; 4] = [0; 4];

/// The ADC sequence's buffer. Called once.
fn adc_buffer() -> &'static mut [u16] {
    // SAFETY: called once, from one chip's check.
    unsafe { &mut *core::ptr::addr_of_mut!(ADC_BUFFER) }
}

/// One timer starting another.
pub fn exercise_trigger<M, S>(rcc: &mut ferrohal_stm32::rcc::Rcc, master: M, slave: S)
where
    M: ferrohal_stm32::timer::ChannelTimer,
    S: ferrohal_stm32::timer::TriggeredBy<M>,
{
    let master = retried_pwm(rcc, master, Hertz::khz(600));
    let slave = retried_pwm(rcc, slave, Hertz::khz(600));
    master.lead();
    slave.follow(&master);
    master.start();
    keep(slave.counter());
    keep(slave.is_running());
    master.stop();
    slave.stop();
    // The generator over both timers: linked by the trigger, then freed and
    // built again started in software.
    use ferrohal_stm32::timer::Awg;
    let mut awg = match Awg::linked(master, slave, (), ()) {
        Ok(awg) => awg,
        Err(refused) => {
            keep(refused.error());
            let (master, slave, a, b) = refused.into_parts();
            match Awg::linked(master, slave, a, b) {
                Ok(awg) => awg,
                Err(_) => park(),
            }
        }
    };
    keep(awg.play());
    awg.abort();
    let (master, Some(slave), a, b) = awg.free() else {
        park()
    };
    let mut awg = match Awg::independent(master, slave, a, b) {
        Ok(awg) => awg,
        Err(_) => park(),
    };
    keep(awg.play());
    keep(awg.free());
}

/// The SD card driver, on the chips that have it.
#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
pub fn exercise_sdmmc<U, Ck, Cmd, D0, D1, D2, D3>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    wiring: ferrohal_stm32::sdmmc::Wiring<U, Ck, Cmd, D0, D1, D2, D3>,
) where
    U: ferrohal_stm32::sdmmc::Instance,
    Ck: ferrohal_stm32::sdmmc::CkPin<U> + ferrohal_stm32::gpio::AltPin,
    Cmd: ferrohal_stm32::sdmmc::CmdPin<U> + ferrohal_stm32::gpio::AltPin,
    D0: ferrohal_stm32::sdmmc::D0Pin<U> + ferrohal_stm32::gpio::AltPin,
    D1: ferrohal_stm32::sdmmc::D1Pin<U> + ferrohal_stm32::gpio::AltPin,
    D2: ferrohal_stm32::sdmmc::D2Pin<U> + ferrohal_stm32::gpio::AltPin,
    D3: ferrohal_stm32::sdmmc::D3Pin<U> + ferrohal_stm32::gpio::AltPin,
{
    use ferrohal_stm32::sdmmc::{BLOCK_SIZE, Config, Sdmmc};

    black_box(&ferrohal_stm32::sdmmc::Wiring::<U, Ck, Cmd, D0, D1, D2, D3>::RESOURCES)
        .iter()
        .for_each(keep);

    let mut sd = match Sdmmc::new(rcc, wiring, &Config::new(black_box(Hertz::mhz(25)))) {
        Ok(sd) => sd,
        Err(refused) => {
            keep(refused.error());
            let wiring = refused.into_parts();
            match Sdmmc::new(rcc, wiring, &Config::new(black_box(Hertz::mhz(25)))) {
                Ok(sd) => sd,
                Err(_) => park(),
            }
        }
    };
    keep(sd.init_card());
    keep(sd.block_count());
    let lba = black_box(0);
    let mut blocks = [[0u8; BLOCK_SIZE]; 2];
    keep(sd.read_blocks(lba, black_box(&mut blocks)));
    keep(sd.write_blocks(lba, black_box(&blocks)));
    let [block, _] = &mut blocks;
    keep(sd.read_block(lba, block));
    keep(sd.write_block(lba, block));
    // Freed back to its wiring, and reopened from it.
    let wiring = sd.free();
    match Sdmmc::new(rcc, wiring, &Config::new(black_box(Hertz::mhz(25)))) {
        Ok(sd) => keep(sd.free()),
        Err(_) => park(),
    }
}
