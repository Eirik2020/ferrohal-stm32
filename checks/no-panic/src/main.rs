//! Proves FerroHAL's public API has no reachable panic (§5, §13 of the
//! architecture brief), for one chip at a time:
//!
//!     cargo build --release --features stm32f401re
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
    uart::{self, Buffers, Parity, StopBits, Uart, UartRx},
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
static mut SPI_RX: [u8; 32] = [0; 32];

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        park()
    };
    let Some((mut rcc, wiring, rx_wiring, spi_wiring, i2c_wiring, mut spare, mut out, mut input)) =
        chip::wire(p)
    else {
        park()
    };
    keep(rcc.clocks());
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
    let config = black_box(uart::Config::new(115_200));
    // A refusal hands back what `new` took: retry once with it, 8N1.
    let (mut uart, mut irq) = match Uart::new(&mut rcc, wiring, &config, buffers) {
        Ok(opened) => opened,
        Err(refused) => {
            keep(refused.error());
            let (wiring, buffers) = refused.into_parts();
            let Ok(opened) = Uart::new(&mut rcc, wiring, &uart::Config::new(115_200), buffers)
            else {
                park()
            };
            opened
        }
    };
    exercise_uart(&mut uart);

    // A receive-only UART, and the frame check with inversion asked for.
    let sbus = black_box(
        uart::Config::new(100_000)
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(black_box(true)),
    );
    keep(Uart::check::<chip::RxUart>(&black_box(rcc.clocks()), &sbus));
    // SAFETY: as TX and RX above.
    let ring = black_box(unsafe { &mut *addr_of_mut!(RX2) });
    let (_rx_line, mut rx_only, mut rx_irq) = match UartRx::new(&mut rcc, rx_wiring, &sbus, ring) {
        Ok(opened) => opened,
        Err(refused) => {
            keep(refused.error());
            let (rx_wiring, ring) = refused.into_parts();
            let Ok(opened) = UartRx::new(&mut rcc, rx_wiring, &uart::Config::new(100_000), ring)
            else {
                park()
            };
            opened
        }
    };
    let mut chunk = [0u8; 32];
    keep(rx_only.read_chunk(black_box(&mut chunk)));
    keep(rx_only.try_read(black_box(&mut chunk)));
    rx_irq.on_interrupt();
    // The buffer check on its own, MPU walk included on the H7.
    keep(ferrohal_stm32::chip::check_dma_buffer(black_box(&[0u8; 8][..])));
    // The interrupt requirements, checked against the NVIC, and the
    // resource lists.
    let vectors = irq.interrupts();
    keep(vectors.verify());
    keep(rx_irq.interrupts().verify());
    vectors.iter().for_each(keep);
    black_box(&chip::UartWiring::RESOURCES).iter().for_each(keep);
    black_box(&chip::RxOnlyWiring::RESOURCES).iter().for_each(keep);
    black_box(&chip::SpiWiring::RESOURCES).iter().for_each(keep);
    irq.on_interrupt();
    irq.on_uart();
    irq.on_rx_dma();
    irq.on_tx_dma();

    // SPI: the frame check, then every operation.
    let spi_config = black_box(
        spi::Config::new(Hertz::mhz(1))
            .with_mode(spi::MODE_3)
            .with_bit_order(spi::BitOrder::LsbFirst),
    );
    keep(Spi::check::<chip::SpiUnit>(&black_box(rcc.clocks()), &spi_config));
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
            Spi::new(&mut rcc, spi_wiring, &spi::Config::new(Hertz::mhz(1)), spi_buffers)
        }
    };
    if let Ok(mut bus) = spi {
        exercise_spi(&mut bus);
    }

    // I2C: the timing check, then every operation of the bus trait.
    black_box(&chip::I2cWiring::RESOURCES).iter().for_each(keep);
    keep(
        i2c::Config::new(Hertz::khz(400)).check(
            black_box(rcc.clocks()).bus(<chip::I2cUnit as i2c::Instance>::KERNEL),
        ),
    );
    // A refusal hands back the wiring: retry once with it, at 100 kHz.
    let mut i2c =
        match I2c::new(&mut rcc, i2c_wiring, &black_box(i2c::Config::new(Hertz::khz(400)))) {
            Ok(bus) => bus,
            Err(refused) => {
                keep(refused.error());
                let i2c_wiring = refused.into_parts();
                let Ok(bus) =
                    I2c::new(&mut rcc, i2c_wiring, &black_box(i2c::Config::new(Hertz::khz(100))))
                else {
                    park()
                };
                bus
            }
        };
    exercise_i2c(&mut i2c);

    // The DMA API, on a stream the UART does not use.
    let mut channel = spare.take_channel();
    keep(channel.flags());
    channel.clear_flags(black_box(Flags::ALL));
    keep(channel.remaining());
    keep(channel.is_running());
    keep(channel.current_target());
    channel.request_stop();
    keep(channel.stop(black_box(1_000)));
    let handle = channel.flag_handle();
    keep(handle.flags());
    handle.clear(black_box(Flags::default()));
    keep(handle.remaining());
    park()
}

fn exercise_uart(uart: &mut Uart) {
    use embedded_io::{Read, ReadReady, Write, WriteReady};

    let mut buf = [0u8; 32];
    let data = black_box(&[0x55u8; 40][..]);
    keep(uart.tx().write(data));
    // The transmit queue.
    keep(uart.tx().try_send(data));
    uart.tx().poll();
    keep(uart.tx().queued());
    keep(uart.tx().health());
    keep(uart.tx().flush());
    keep(uart.tx().is_idle());
    keep(uart.send_break());
    keep(uart.rx().try_read(black_box(&mut buf)));
    keep(uart.rx().read_within(black_box(&mut buf), black_box(10)));
    keep(uart.rx().available());
    keep(Read::read(uart, black_box(&mut buf)));
    keep(Write::write(uart, data));
    keep(Write::flush(uart));
    keep(uart.read_ready());
    keep(uart.write_ready());
    let config = black_box(
        uart::Config::new(9_600)
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(true)
            .with_inverted_tx(true),
    );
    keep(uart.set_config(&config));
    // The line's event enables, off and on again.
    uart.line().mask();
    uart.line().unmask();
    let (_line, tx, rx) = uart.halves();
    keep(tx.write(data));
    keep(rx.try_read(&mut buf));
    // The chunk reader, the fault counts and the caller clock.
    keep(rx.read_chunk(black_box(&mut buf)));
    keep(rx.generation());
    keep(rx.health());
    rx.set_clock(Some(&CLOCK));
    keep(rx.read_within(black_box(&mut buf), black_box(10)));
    tx.set_clock(Some(&CLOCK));
    keep(tx.flush());
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
    keep(exti.interrupt());
    keep(exti.is_pending());
    exti.trigger();
    exti.clear();
    keep(exti.is_high());
    keep(InputPin::is_low(&mut exti));
    exti.set_edge(ctrl, black_box(Edge::Falling));
    exti.unlisten(ctrl);
    exti.listen(ctrl);
    let handle = exti.handle();
    keep(handle.is_pending());
    handle.clear();
    handle.trigger();
    keep(handle.interrupt());
    keep(handle.line());
    keep(ferrohal_stm32::exti::vector(black_box(12)));
    keep(exti.release(ctrl));
}

/// Timers: a tick and a timebase through every operation. Each chip's
/// `wire` calls it.
pub fn exercise_timers<T: ferrohal_stm32::timer::Instance, U: ferrohal_stm32::timer::Instance32>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    tick_timer: T,
    base_timer: U,
) {
    use ferrohal_stm32::timer::{Tick, Timebase, divider};

    keep(divider(black_box(Hertz::mhz(84)), black_box(Hertz::khz(1)), black_box(16)));
    keep(Tick::<T>::check(&black_box(rcc.clocks()), black_box(Hertz::khz(8))));
    keep(Timebase::<U>::check(&black_box(rcc.clocks()), black_box(Hertz::mhz(1))));
    let mut tick = match Tick::new(rcc, tick_timer, black_box(Hertz::khz(1))) {
        Ok(tick) => tick,
        Err(refused) => {
            keep(refused.error());
            let tick_timer = refused.into_parts();
            match Tick::new(rcc, tick_timer, black_box(Hertz::khz(1))) {
                Ok(tick) => tick,
                Err(_) => park(),
            }
        }
    };
    keep(tick.on_interrupt());
    keep(tick.rate());
    keep(tick.interrupt());
    keep(tick.verify());
    tick.pause();
    tick.resume();
    let mut base = match Timebase::new(rcc, base_timer, black_box(Hertz::mhz(1))) {
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
    let reader = base.reader();
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

/// A lane built by the direct conversion, with the hand-back retried.
pub fn exercise_into_lane<T, P, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
) where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<T, 1>,
{
    let mut pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    // SAFETY: as TX and RX in `main`.
    let buffer = black_box(unsafe { &mut *addr_of_mut!(LANE2) });
    let channel = pwm.channel::<1, P>(pin);
    let mut lane = match channel.into_lane(stream, buffer) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (channel, stream, buffer) = refused.into_parts();
            match channel.into_lane(stream, buffer) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    keep(lane.arm(black_box(&[1, 2, 3][..])));
    lane.abort();
}

/// PWM with a compare lane on channel 1, through every operation.
pub fn exercise_lane<T, P, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
)
where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<T, 1>,
{
    use ferrohal_stm32::timer::{Pwm, nearest};

    keep(nearest(black_box(Hertz::mhz(200)), black_box(Hertz::khz(600)), black_box(16)));
    keep(Pwm::<T>::check(&black_box(rcc.clocks()), black_box(Hertz::khz(600))));
    let mut pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    keep(pwm.period());
    keep(pwm.frequency());
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
    lane.channel().set_duty(black_box(70));
    keep(lane.channel().duty());
    keep(lane.arm(black_box(&[1, 2, 3][..])));
        pwm.stage(black_box(1));
        pwm.start();
        keep(lane.on_interrupt());
        keep(lane.poll());
        keep(lane.is_busy());
        keep(lane.interrupt());
        keep(lane.verify());
        lane.channel().set_duty(black_box(0));
        lane.abort();
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
        keep(awg.on_interrupt(black_box(0)));
        keep(awg.poll(black_box(0)));
        keep(awg.interrupt(black_box(0)));
        awg.abort();
        awg.set_active_low(black_box(true));
        keep(awg.listen(black_box(Hertz::mhz(1)), black_box(4)));
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
}

/// A lane that reads as well, on channel 1.
pub fn exercise_two_way<T, P, const CD: u8, const SN: u8, const PORT: char>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: T,
    pin: P,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
)
where
    T: ferrohal_stm32::timer::ChannelTimer,
    P: ferrohal_stm32::timer::ChannelPin<T, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<T, 1>
        + ferrohal_stm32::timer::GpioDma,
    ferrohal_stm32::gpio::Port<PORT>: ferrohal_stm32::gpio::PortInstance,
{
    use ferrohal_stm32::timer::Pwm;

    let mut pwm = retried_pwm(rcc, timer, Hertz::khz(600));
    pwm.set_active_low::<1>(black_box(true));
    let wiring =
        ferrohal_stm32::timer::LaneWiring::<T, 1, P, ferrohal_stm32::dma::Stream<CD, SN>>::new(
            pin, stream,
        );
    // SAFETY: as TX and RX in `main`.
    let (buffer, reads) = black_box(unsafe { (&mut *addr_of_mut!(TWO_WAY), &mut *addr_of_mut!(TWO_WAY_READS)) });
    let mut lane = match wiring.assemble_two_way::<PORT>(&mut pwm, buffer, reads) {
        Ok(lane) => lane,
        Err(refused) => {
            keep(refused.error());
            let (wiring, (buffer, reads)) = refused.into_parts();
            match wiring.assemble_two_way::<PORT>(&mut pwm, buffer, reads) {
                Ok(lane) => lane,
                Err(_) => park(),
            }
        }
    };
    keep(lane.listen(black_box(4), black_box(10)));
    keep(lane.samples().len());
    lane.abort();
}

/// A complementary output (CH1N) of a timer with a break unit, assembled
/// from its wiring.
pub fn exercise_complementary<B, N, const CD: u8, const SN: u8>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    timer: B,
    pin: N,
    stream: ferrohal_stm32::dma::Stream<CD, SN>,
)
where
    B: ferrohal_stm32::timer::ChannelTimer,
    N: ferrohal_stm32::timer::ComplementaryPin<B, 1>,
    ferrohal_stm32::dma::Stream<CD, SN>: ferrohal_stm32::timer::CompareDma<B, 1>,
{
    let mut pwm = retried_pwm(rcc, timer, Hertz::khz(1));
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
    lane.channel().set_duty(black_box(10));
    pwm.set_output::<1>(black_box(false));
    pwm.set_output::<1>(black_box(true));
    pwm.start();
    pwm.start_one_pulse();
}

/// A sampler of port B through every operation.
pub fn exercise_sampler<U, S>(rcc: &mut ferrohal_stm32::rcc::Rcc, timer: U, stream: S)
where
    U: ferrohal_stm32::timer::Instance,
    S: ferrohal_stm32::timer::UpdateDma<U> + ferrohal_stm32::timer::GpioDma,
{
    // SAFETY: as TX and RX in `main`.
    let samples = black_box(unsafe { &mut *addr_of_mut!(SAMPLES) });
    let mut sampler = match
        ferrohal_stm32::timer::Sampler::new::<U, S, 'B'>(rcc, timer, stream, black_box(Hertz::mhz(4)), samples)
    {
        Ok(sampler) => sampler,
        Err(refused) => {
            keep(refused.error());
            let (timer, stream, samples) = refused.into_parts();
            match ferrohal_stm32::timer::Sampler::new::<U, S, 'B'>(
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
    keep(sampler.poll());
    keep(sampler.samples().len());
    keep(sampler.interrupt());
    keep(sampler.verify());
    sampler.stop();
}

/// The watchdog and the reset cause.
pub fn exercise_watchdog(rcc: &mut ferrohal_stm32::rcc::Rcc, iwdg: ferrohal_stm32::chip::WatchdogPeripheral) {
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
pub fn exercise_blocking<U, K, I, O>(rcc: &mut ferrohal_stm32::rcc::Rcc, wiring: spi::BlockingWiring<U, K, I, O>)
where
    U: spi::Instance,
    K: spi::SckPin<U> + ferrohal_stm32::gpio::AltPin,
    I: spi::MisoPin<U> + ferrohal_stm32::gpio::AltPin,
    O: spi::MosiPin<U> + ferrohal_stm32::gpio::AltPin,
{
    use embedded_hal::spi::SpiBus;

    black_box(&spi::BlockingWiring::<U, K, I, O>::RESOURCES).iter().for_each(keep);
    // A refusal hands back the wiring: retry once with it, at 1 MHz.
    let mut bus = match spi::BlockingSpi::new(rcc, wiring, &black_box(spi::Config::new(Hertz::mhz(10)))) {
        Ok(bus) => bus,
        Err(refused) => {
            keep(refused.error());
            let wiring = refused.into_parts();
            let Ok(bus) = spi::BlockingSpi::new(rcc, wiring, &black_box(spi::Config::new(Hertz::mhz(1)))) else {
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
    keep(bus.set_config(&black_box(spi::Config::new(Hertz::khz(400)).with_mode(spi::MODE_3))));
    keep(bus.frequency());
    keep(bus.name());
    bus.set_clock(Some(&CLOCK));
    keep(SpiBus::write(&mut bus, data));
}

/// The I2C through every operation of the bus trait, and the empty
/// operation inside a transaction.
fn exercise_i2c(bus: &mut I2c) {
    use embedded_hal::i2c::{I2c as HalI2c, Operation};

    let data = black_box(&[0x5au8; 4][..]);
    let mut buf = [0u8; 4];
    keep(HalI2c::read(bus, black_box(0x18), black_box(&mut buf)));
    keep(HalI2c::write(bus, black_box(0x18), data));
    keep(HalI2c::write_read(bus, black_box(0x18), data, black_box(&mut buf)));
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
#[cfg(feature = "stm32f401re")]
fn keep_usb_clock(rcc: &mut ferrohal_stm32::rcc::Rcc) {
    keep(rcc.usb_clock());
}

/// USB's own part: the clock and the constructor. The controller's driver is
/// third-party and sits outside this check.
#[cfg(not(feature = "stm32f401re"))]
pub fn exercise_usb(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    token: ferrohal_stm32::chip::USB_OTG_FS,
    dm: ferrohal_stm32::gpio::Pin<'A', 11, ferrohal_stm32::gpio::Alternate<10>>,
    dp: ferrohal_stm32::gpio::Pin<'A', 12, ferrohal_stm32::gpio::Alternate<10>>,
) {
    keep(ferrohal_stm32::usb::UsbFs::interrupt());
    keep(rcc.usb_clock());
    let usb = match ferrohal_stm32::usb::UsbFs::new(rcc, token, dm, dp) {
        Ok(usb) => usb,
        Err(refused) => {
            keep(refused.error());
            let (token, dm, dp) = refused.into_parts();
            match ferrohal_stm32::usb::UsbFs::new(rcc, token, dm, dp) {
                Ok(usb) => usb,
                Err(_) => park(),
            }
        }
    };
    keep(usb);
}

fn exercise_spi(bus: &mut Spi) {
    use embedded_hal::spi::SpiBus;

    let data = black_box(&[0xa5u8; 40][..]);
    let mut buf = [0u8; 40];
    keep(bus.start(data));
    keep(bus.on_interrupt());
    keep(bus.poll());
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
    keep(bus.health());
    keep(bus.interrupts().verify());
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
    fn take_channel(&mut self) -> ferrohal_stm32::dma::Channel {
        match self.stream.take() {
            Some(stream) => stream.into_channel(),
            None => park(),
        }
    }
}

#[cfg(feature = "stm32f401re")]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI2, USART1, USART6},
        dma::Stream,
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{AhbPrescaler, ApbPrescaler, Config, Pll, PllP, PllSource, Rcc, Supply, Sysclk},
        i2c,
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

    /// I2C1 on PB8 and PB9, the pins the Nucleo bench's Arduino connector
    /// carries.
    pub type I2cWiring = i2c::Wiring<I2C1, Pin<'B', 8, Alternate<4>>, Pin<'B', 9, Alternate<4>>>;

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
        let rx6 = a.p12.into_alternate::<8>(&mut a.ctrl);
        let analog = a.p1.into_analog(&mut a.ctrl);
        let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
        super::exercise_exti::<'A', 1>(&mut exti.ctrl, exti.l1);
        super::exercise_timers(&mut rcc, p.TIM10, p.TIM5);
        super::exercise_watchdog(&mut rcc, p.IWDG);
        super::exercise_adc(&mut rcc, p.ADC1, a.p4.into_analog(&mut a.ctrl), dma2.s4, super::adc_buffer());
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
        // PA12 is USART6's here, so the constructor is left to the F405,
        // which shares this code; the F4's clock check runs on its own.
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
        super::exercise_lane(&mut rcc, p.TIM4, b.p6.into_alternate::<2>(&mut b.ctrl), dma1.s0);
        super::exercise_into_lane(
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
                scl: b.p8.into_alternate::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate::<4>(&mut b.ctrl),
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

/// The Foxeer F405 V2's ports, from its 8 MHz crystal: HSE in the path.
#[cfg(feature = "stm32f405rg")]
mod chip {
    use ferrohal_stm32::{
        chip::{I2C1, Peripherals, SPI2, UART4, USART2},
        dma::Stream,
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Hse, HseMode, Pll, PllP, PllSource, Rcc, Supply,
            Sysclk,
        },
        time::Hertz,
        i2c,
        spi,
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
    pub type I2cWiring = i2c::Wiring<I2C1, Pin<'B', 8, Alternate<4>>, Pin<'B', 9, Alternate<4>>>;

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
        let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
        super::exercise_exti::<'B', 2>(&mut exti.ctrl, exti.l2);
        super::exercise_timers(&mut rcc, p.TIM6, p.TIM2);
        super::exercise_watchdog(&mut rcc, p.IWDG);
        super::exercise_adc(&mut rcc, p.ADC1, a.p4.into_analog(&mut a.ctrl), dma2.s4, super::adc_buffer());
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
            p.USB_OTG_FS,
            a.p11.into_alternate::<10>(&mut a.ctrl),
            a.p12.into_alternate::<10>(&mut a.ctrl),
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
        super::exercise_lane(&mut rcc, p.TIM4, b.p6.into_alternate::<2>(&mut b.ctrl), dma1.s0);
        super::exercise_into_lane(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM2,
            a.p15.into_alternate::<1>(&mut a.ctrl),
            spare1.s5,
        );
        super::exercise_complementary(&mut rcc, p.TIM8, a.p7.into_alternate::<3>(&mut a.ctrl), dma2.s2);
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
                scl: b.p8.into_alternate::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate::<4>(&mut b.ctrl),
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
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Pll, PllSource, Rcc, Supply, Sysclk,
            VoltageScale,
        },
        i2c,
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
    pub type I2cWiring = i2c::Wiring<I2C1, Pin<'B', 8, Alternate<4>>, Pin<'B', 9, Alternate<4>>>;

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
        let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
        super::exercise_exti::<'B', 2>(&mut exti.ctrl, exti.l2);
        super::exercise_timers(&mut rcc, p.TIM6, p.TIM2);
        super::exercise_watchdog(&mut rcc, p.IWDG1);
        super::exercise_adc(&mut rcc, p.ADC1, a.p4.into_analog(&mut a.ctrl), dma2.s4, super::adc_buffer());
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
            p.USB_OTG_FS,
            a.p11.into_alternate::<10>(&mut a.ctrl),
            a.p12.into_alternate::<10>(&mut a.ctrl),
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
        super::exercise_lane(&mut rcc, p.TIM4, b.p6.into_alternate::<2>(&mut b.ctrl), dma2.s0);
        super::exercise_into_lane(
            &mut rcc,
            unsafe { Peripherals::steal() }.TIM2,
            a.p15.into_alternate::<1>(&mut a.ctrl),
            spare1.s5,
        );
        super::exercise_complementary(&mut rcc, p.TIM1, a.p7.into_alternate::<1>(&mut a.ctrl), dma1.s3);
        super::exercise_sampler(&mut rcc, p.TIM8, dma2.s1);
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
                scl: b.p8.into_alternate::<4>(&mut b.ctrl),
                sda: b.p9.into_alternate::<4>(&mut b.ctrl),
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
    S: ferrohal_stm32::adc::AdcDma<A>,
{
    use ferrohal_stm32::adc::{Adc, Config};

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
    keep(scan.poll().map(|result| result.map(|samples| samples.len())));
    keep(scan.on_interrupt().map(|result| result.map(|samples| samples.len())));
    keep(scan.samples().first().copied());
    scan.abort();
    keep(scan.is_busy());
    keep(scan.interrupt());
    keep(scan.max_sample());
    keep(scan.to_millivolts(black_box(1), black_box(3300)));
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
    use ferrohal_stm32::timer::Pwm;

    let mut master = retried_pwm(rcc, master, Hertz::khz(600));
    let mut slave = retried_pwm(rcc, slave, Hertz::khz(600));
    master.lead();
    slave.follow(&master);
    master.start();
    keep(slave.counter());
    keep(slave.is_running());
    master.stop();
    slave.stop();
}

/// The SD card driver, on the chips that have it.
#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
pub fn exercise_sdmmc<U, Ck, Cmd, D0, D1, D2, D3>(
    rcc: &mut ferrohal_stm32::rcc::Rcc,
    wiring: ferrohal_stm32::sdmmc::Wiring<U, Ck, Cmd, D0, D1, D2, D3>,
) where
    U: ferrohal_stm32::sdmmc::Instance,
    Ck: ferrohal_stm32::sdmmc::CkPin<U>,
    Cmd: ferrohal_stm32::sdmmc::CmdPin<U>,
    D0: ferrohal_stm32::sdmmc::D0Pin<U>,
    D1: ferrohal_stm32::sdmmc::D1Pin<U>,
    D2: ferrohal_stm32::sdmmc::D2Pin<U>,
    D3: ferrohal_stm32::sdmmc::D3Pin<U>,
{
    use ferrohal_stm32::sdmmc::{BLOCK_SIZE, Config, Sdmmc};

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
}
