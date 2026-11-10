//! Run U: the constructor refusals, as the F401's run V.
//!
//! - RCC: a config naming `Pll1P` with no PLL1 refuses, the RCC token
//!   comes back, and it freezes the bench's clocks. A compile-time assert
//!   pins the error before boot.
//! - UART: an unreachable baud refuses; the wiring and buffers open
//!   USART6, which echoes a frame over Z2.
//! - SPI: a 0 Hz stream config and a 0 Hz polled config refuse; the
//!   wirings transfer bytes over S1 and S3.
//! - I2C: an out-of-range speed refuses; the wiring opens the bus, which
//!   NACKs a vacant address.
//! - ADC: an empty scan sequence refuses; the converter, stream and buffer
//!   come back and scan the internal channels.
//! - PWM: a 0 rate refuses; the timer comes back and makes the lanes.
//! - Lane: a buffer in DTCM, which no DMA controller reaches, refuses; the
//!   channel and stream come back and play on PB0.
//! - Sampler: a 0 rate refuses; the timer, stream and buffer come back and
//!   decode the lane playing on PB0.
//! - AWG: two timers at different periods refuse; both PWMs and both lanes
//!   come back, the second re-rated, and the pair plays PB0.
//! - Tick and Timebase: rates the clock cannot divide to refuse; the
//!   timers come back and run. The tick pauses right away, its vector
//!   unbound.
//! - Watchdog: a window longer than the timeout refuses, then a 1 us
//!   timeout refuses; the peripheral comes back and starts, feeds for
//!   300 ms, then starves, and the second boot reports the bite — run
//!   K's shape, so this run's own reset clears its flags.
//!
//! USB is left out: on this chip the constructor starts its own 48 MHz
//! source (HSI48 and the supply), so nothing this bench can wire makes it
//! refuse; the F401's run V covers that constructor.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA1_STREAM6`, `DMA1_STREAM1` | USART6's `UartIrq` | 2 |
//! | `CEC` | RTIC, dispatching the priority 1 `checks` task | 1 |
//!
//! Needs Z2, S1 and S3 fitted; nothing else.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/pwm.rs"]
mod pwm;
#[path = "../../../common/tests.rs"]
mod tests;
#[path = "../../../common/watchdog.rs"]
mod watchdog;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        rcc::ClockError,
        uart::{
            Buffers as UartBuffers, Config as UartConfig, ConfigError as UartConfigError, Uart,
            UartIrq,
        },
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
        watchdog,
    };

    #[shared]
    struct Shared {
        // None until the checks task constructs the port from the parts a
        // refusal hands back.
        irq6: Option<UartIrq>,
    }

    #[local]
    struct Local {
        run: Option<board::RunU>,
        rcc_refusal: Option<ClockError>,
    }

    #[init]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let (run, rcc_refusal) = match board::run_u(cx.device) {
            Ok(both) => both,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run U, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (
            Shared { irq6: None },
            Local {
                run: Some(run),
                rcc_refusal: Some(rcc_refusal),
            },
        )
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_irq(mut cx: usart6_irq::Context) {
        cx.shared.irq6.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(binds = DMA1_STREAM6, priority = 2, shared = [irq6])]
    fn usart6_tx(mut cx: usart6_tx::Context) {
        cx.shared.irq6.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(binds = DMA1_STREAM1, priority = 2, shared = [irq6])]
    fn usart6_rx(mut cx: usart6_rx::Context) {
        cx.shared.irq6.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(priority = 1, local = [run, rcc_refusal], shared = [irq6])]
    async fn checks(mut cx: checks::Context) {
        use ferrohal_stm32::{
            adc::{Adc, Config as AdcConfig, Error as AdcError},
            i2c::{Config as I2cConfig, ConfigError as I2cConfigError, Error as I2cError, I2c},
            iwdg::Watchdog,
            spi::{self, Buffers as SpiBuffers, Config as SpiConfig, ConfigError as SpiConfigError},
            time::Hertz,
            timer::{Awg, ConfigError as TimerConfigError, Pwm, Sampler, Tick, Timebase},
        };
        use embedded_hal::i2c::I2c as _;
        use embedded_hal::spi::SpiBus as _;

        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        // Run K's two-boot shape: the second boot, arriving by the
        // watchdog's own reset at the end of the first, reports the bite
        // and ends cleanly; the first runs the refusals.
        let cause = run.rcc.reset_cause();
        if watchdog::after_reset(&mut report, "IWDG1 50 ms", cause, (29, 34), (47_500, 52_500)) {
            tests::finish(&report, || {});
        }
        static mut SPI_TX: [u8; 64] = [0; 64];
        static mut SPI_RX: [u8; 64] = [0; 64];
        static mut LANE3: [u32; 20] = [0; 20];
        static mut LANE5: [u32; 20] = [0; 20];
        static mut SAMPLES: [u16; 1024] = [0; 1024];
        static mut SCAN: [u16; 2] = [0; 2];
        // SAFETY: `checks` runs once, and these are the only references.
        let statics = unsafe {
            (
                &mut *core::ptr::addr_of_mut!(SPI_TX),
                &mut *core::ptr::addr_of_mut!(SPI_RX),
                &mut *core::ptr::addr_of_mut!(LANE3),
                &mut *core::ptr::addr_of_mut!(LANE5),
                &mut *core::ptr::addr_of_mut!(SAMPLES),
                &mut *core::ptr::addr_of_mut!(SCAN),
            )
        };
        let (spi_tx, spi_rx, lane3_buf, lane5_buf, samples, scan) = statics;

        // RCC: the no-PLL1 config refused, and the token froze these clocks.
        report.check(
            "run U",
            "a config naming Pll1P with no PLL1 refuses [API-001]",
            match cx.local.rcc_refusal.take() {
                Some(ClockError::PllNotConfigured) => Ok(()),
                Some(other) => Err(Fail::Count {
                    expected: ClockError::PllNotConfigured as u32,
                    got: other as u32,
                }),
                None => Err(Fail::Count { expected: 1, got: 0 }),
            },
        );

        // UART: a baud no clock reaches refuses; the wiring and buffers
        // come back, open the port, and echo a frame over Z2.
        static mut TX: [u8; 256] = [0; 256];
        static mut RX: [u8; 256] = [0; 256];
        // SAFETY: as above.
        let (tx, rx): (&'static mut [u8], &'static mut [u8]) =
            unsafe { (&mut *core::ptr::addr_of_mut!(TX), &mut *core::ptr::addr_of_mut!(RX)) };
        let (usart6, uart_refusal) = match Uart::new(
            &mut run.rcc,
            run.usart6,
            &UartConfig::new(1),
            UartBuffers { tx, rx },
        ) {
            Ok(_) => defmt::panic!("a 1-baud UART config was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (wiring, buffers) = refused.into_parts();
                match Uart::new(&mut run.rcc, wiring, &UartConfig::new(115_200), buffers) {
                    Ok((uart, irq)) => {
                        cx.shared.irq6.lock(|handle| *handle = Some(irq));
                        (uart, Some(error))
                    }
                    Err(refused) => defmt::panic!("USART6: {}", refused.error()),
                }
            }
        };
        report.check(
            "USART6 PC6->PC7",
            "an unreachable baud refuses [API-001]",
            match uart_refusal {
                Some(UartConfigError::BaudRate) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::NotRefused(UartConfigError::BaudRate)),
            },
        );
        let mut usart6 = usart6;
        let mut loop6 = tests::Loop {
            name: "USART6 PC6->PC7",
            uart: &mut usart6,
        };
        report.check(
            loop6.name,
            "the handed-back wiring echoes a frame [API-001]",
            tests::echo(&mut loop6, b"refusal\n"),
        );

        // SPI: a 0 Hz config refuses for the stream and the polled driver,
        // and the handed-back wirings transfer over S1 and S3.
        let sent = b"refusal run";
        report.check(
            "SPI1 PA5/PA6/PD7",
            "a 0 Hz config refuses; the wiring transfers over S1 [API-001]",
            match spi::Spi::new(
                &mut run.rcc,
                run.spi1,
                &SpiConfig::new(Hertz(0)),
                SpiBuffers {
                    tx: spi_tx,
                    rx: spi_rx,
                },
            ) {
                Ok(_) => Err(Fail::SpiConfig(SpiConfigError::Frequency)),
                Err(refused) => {
                    if refused.error() != SpiConfigError::Frequency {
                        Err(Fail::SpiConfig(refused.error()))
                    } else {
                        let (wiring, buffers) = refused.into_parts();
                        match spi::Spi::new(&mut run.rcc, wiring, &board::SPI, buffers) {
                            Ok(mut spi) => match spi.transfer(sent) {
                                Ok(got) if got == sent => Ok(()),
                                Ok(_) => Err(Fail::Count {
                                    expected: sent.len() as u32,
                                    got: 0,
                                }),
                                Err(error) => Err(Fail::Spi(error)),
                            },
                            Err(refused) => Err(Fail::SpiConfig(refused.error())),
                        }
                    }
                }
            },
        );
        report.check(
            "SPI2 PB10/PC2/PC3 (polled)",
            "a 0 Hz config refuses; the wiring writes over S3 [API-001]",
            match spi::BlockingSpi::new(&mut run.rcc, run.spi2_polled, &SpiConfig::new(Hertz(0))) {
                Ok(_) => Err(Fail::SpiConfig(SpiConfigError::Frequency)),
                Err(refused) => {
                    if refused.error() != SpiConfigError::Frequency {
                        Err(Fail::SpiConfig(refused.error()))
                    } else {
                        let wiring = refused.into_parts();
                        match spi::BlockingSpi::new(&mut run.rcc, wiring, &board::SPI) {
                            Ok(mut spi) => match spi.write(sent) {
                                Ok(()) => Ok(()),
                                Err(error) => Err(Fail::Spi(error)),
                            },
                            Err(refused) => Err(Fail::SpiConfig(refused.error())),
                        }
                    }
                }
            },
        );

        // I2C: an out-of-range speed refuses; the wiring opens the bus,
        // which NACKs a vacant address all by itself.
        report.check(
            "I2C3 PA8/PC9",
            "an out-of-range speed refuses; the bus NACKs a vacant address [API-001]",
            match I2c::new(&mut run.rcc, run.i2c3, &I2cConfig::new(Hertz::mhz(5))) {
                Ok(_) => Err(Fail::I2cConfig(I2cConfigError::Speed)),
                Err(refused) => {
                    if refused.error() != I2cConfigError::Speed {
                        Err(Fail::I2cConfig(refused.error()))
                    } else {
                        let wiring = refused.into_parts();
                        match I2c::new(&mut run.rcc, wiring, &I2cConfig::new(Hertz::khz(50))) {
                            Ok(mut bus) => match bus.write(0x1E, &[0xA5]) {
                                Err(I2cError::Nack) => Ok(()),
                                Err(error) => Err(Fail::I2c(error)),
                                Ok(()) => Err(Fail::I2cNotNack(None)),
                            },
                            Err(refused) => Err(Fail::I2cConfig(refused.error())),
                        }
                    }
                }
            },
        );

        // ADC: an empty sequence refuses; the converter, stream and buffer
        // come back and scan the internal channels.
        let adc = match Adc::new(&mut run.rcc, run.adc1, &AdcConfig::slow_source()) {
            Ok(adc) => adc,
            Err(refused) => defmt::panic!("ADC1: {}", refused.error()),
        };
        let pin = adc.channel(&run.analog1);
        report.check(
            "ADC1 scan",
            "an empty sequence refuses; the parts scan the pin twice [API-001]",
            match adc.into_scan(run.scan_stream, &[], scan) {
                Ok(_) => Err(Fail::Count {
                    expected: AdcError::Sequence as u32,
                    got: 0,
                }),
                Err(refused) => {
                    if refused.error() != AdcError::Sequence {
                        Err(Fail::Count {
                            expected: AdcError::Sequence as u32,
                            got: refused.error() as u32,
                        })
                    } else {
                        let (adc, stream, buffer) = refused.into_parts();
                        let Ok(mut scan) = adc.into_scan(stream, &[pin, pin], buffer) else {
                            defmt::panic!("the scan")
                        };
                        let mut result = Err(Fail::Count { expected: 1, got: 0 });
                        if scan.start().is_ok() {
                            let mut spins = cpu.0 / 8;
                            loop {
                                if let Some(done) = scan.poll() {
                                    if let Ok(samples) = done {
                                        result = if samples.len() == 2 {
                                            Ok(())
                                        } else {
                                            Err(Fail::Count {
                                                expected: 2,
                                                got: samples.len() as u32,
                                            })
                                        };
                                    }
                                    break;
                                }
                                if spins == 0 {
                                    break;
                                }
                                spins -= 1;
                            }
                        }
                        result
                    }
                }
            },
        );

        // PWM: a 0 rate refuses; the timer comes back at 500 kHz.
        let (mut pwm3, pwm3_refusal) = match Pwm::new(&mut run.rcc, run.tim3, Hertz(0)) {
            Ok(_) => defmt::panic!("a 0-rate PWM config was accepted"),
            Err(refused) => {
                let error = refused.error();
                let tim3 = refused.into_parts();
                match Pwm::new(&mut run.rcc, tim3, Hertz::khz(500)) {
                    Ok(pwm) => (pwm, Some(error)),
                    Err(refused) => defmt::panic!("TIM3 PWM: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM3 PWM",
            "a 0 rate refuses; the timer comes back at 500 kHz [API-001]",
            match pwm3_refusal {
                Some(TimerConfigError::Inexact) if pwm3.frequency() == Hertz::khz(500) => Ok(()),
                Some(TimerConfigError::Inexact) => Err(Fail::Count {
                    expected: 500_000,
                    got: pwm3.frequency().0,
                }),
                Some(other) => Err(Fail::Timer(other)),
                None => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Lane: a buffer in DTCM, which no DMA controller reaches (the H7's
        // DMA_RAM holds only the AXI and SRAM1 to 4 blocks), refuses; the
        // channel and stream come back and play.
        // SAFETY: the DTCM at 0x20010000 is real memory the CPU can use,
        // but no DMA controller reaches it, and the constructor refuses
        // the buffer before programming anything. Nothing else touches it.
        let dtcm: &'static mut [u32] =
            unsafe { core::slice::from_raw_parts_mut(0x2001_0000 as *mut u32, 16) };
        let (mut lane3, lane_refusal) = match pwm3
            .channel::<3, _>(run.pb0)
            .into_lane(run.lane3_stream, dtcm)
        {
            Ok(_) => defmt::panic!("a DTCM buffer was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (channel, stream, _dtcm) = refused.into_parts();
                match channel.into_lane(stream, lane3_buf) {
                    Ok(lane) => (lane, Some(error)),
                    Err(refused) => defmt::panic!("the TIM3 lane: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM3 CH3",
            "a DTCM buffer refuses; the channel and stream come back [API-001]",
            match lane_refusal {
                Some(UartConfigError::BufferUnreachable) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::NotRefused(UartConfigError::BufferUnreachable)),
            },
        );

        // Sampler: a 0 rate refuses; the timer, stream and buffer come back.
        let (mut sampler, sampler_refusal) = match Sampler::new::<_, _, 'B'>(
            &mut run.rcc,
            run.tim8,
            run.sampler_stream,
            Hertz(0),
            samples,
        ) {
            Ok(_) => defmt::panic!("a 0-rate sampler was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (tim8, stream, buffer) = refused.into_parts();
                match Sampler::new::<_, _, 'B'>(
                    &mut run.rcc,
                    tim8,
                    stream,
                    Hertz::mhz(1),
                    buffer,
                ) {
                    Ok(sampler) => (sampler, Some(error)),
                    Err(refused) => defmt::panic!("the TIM8 sampler: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM8 sampler",
            "a 0 rate refuses; the timer, stream and buffer come back [API-001]",
            match sampler_refusal {
                Some(UartConfigError::Unsupported) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::NotRefused(UartConfigError::Unsupported)),
            },
        );

        // The use: the handed-back lane plays, and the handed-back sampler
        // decodes it from PB0, as run J does.
        let pattern: u16 = 0x3c96;
        let budget = cpu.0 / 100;
        let lane_use = (|| {
            sampler
                .set_rate(Hertz(pwm3.frequency().0 * 12))
                .map_err(Fail::Timer)?;
            let per_bit16 =
                (u64::from(sampler.rate().0) * 16 / u64::from(pwm3.frequency().0)) as u32;
            let count = (per_bit16 as usize * (16 + 8)) / 16;
            lane3
                .arm(&pwm::duties(pattern, pwm3.period()))
                .map_err(|_| Fail::Count { expected: 16, got: 0 })?;
            sampler
                .start(count)
                .map_err(|_| Fail::Count { expected: count as u32, got: 0 })?;
            pwm3.stage(0b0100);
            pwm3.start();
            let done = pwm::wait(budget, || lane3.poll());
            let sampled = pwm::wait(budget, || sampler.poll());
            pwm3.stop();
            pwm3.close_requests();
            if !matches!(done, Some(Ok(()))) || !matches!(sampled, Some(Ok(_))) {
                return Err(Fail::Count { expected: 2, got: 0 });
            }
            pwm::decode(sampler.samples(), 0, pattern, per_bit16, false)
        })();
        report.check(
            "TIM3 CH3 PB0",
            "the handed-back lane and sampler play and decode [API-001]",
            lane_use,
        );

        // AWG: two timers at different periods refuse; both PWMs and both
        // lanes come back, the second re-rated, and the pair plays PB0.
        let mut pwm5 = match Pwm::new(&mut run.rcc, run.tim5, Hertz::khz(100)) {
            Ok(pwm) => pwm,
            Err(refused) => defmt::panic!("TIM5 PWM: {}", refused.error()),
        };
        let lane5 = match pwm5
            .channel::<1, _>(run.pa0)
            .into_lane(run.lane5_stream, lane5_buf)
        {
            Ok(lane) => lane,
            Err(refused) => defmt::panic!("the TIM5 lane: {}", refused.error()),
        };
        let (mut awg, awg_refusal) = match Awg::independent(pwm3, pwm5, (lane3,), (lane5,)) {
            Ok(_) => defmt::panic!("mismatched periods were accepted"),
            Err(refused) => {
                let error = refused.error();
                let (pwm3, mut pwm5, lane3, lane5) = refused.into_parts();
                if pwm5.set_frequency(Hertz::khz(500)).is_err() {
                    defmt::panic!("re-rating TIM5");
                }
                match Awg::independent(pwm3, pwm5, lane3, lane5) {
                    Ok(awg) => (awg, Some(error)),
                    Err(refused) => defmt::panic!("the AWG: {}", refused.error()),
                }
            }
        };
        let awg_pattern: u16 = 0xa5c3;
        let awg_use = (|| {
            if awg_refusal != Some(TimerConfigError::PeriodMismatch) {
                return Err(Fail::Timer(TimerConfigError::PeriodMismatch));
            }
            sampler
                .set_rate(Hertz(awg.sample_rate().0 * 12))
                .map_err(Fail::Timer)?;
            let period = awg.period();
            if awg.load(0, &pwm::duties(awg_pattern, period)).is_err() {
                return Err(Fail::Count { expected: 16, got: 0 });
            }
            let awg_per_bit16 =
                (u64::from(sampler.rate().0) * 16 / u64::from(awg.sample_rate().0)) as u32;
            let count = (awg_per_bit16 as usize * (16 + 8)) / 16;
            sampler
                .start(count)
                .map_err(|_| Fail::Count { expected: count as u32, got: 0 })?;
            if awg.play().is_err() {
                return Err(Fail::Count { expected: 1, got: 0 });
            }
            if !matches!(pwm::wait(budget, || awg.poll(0)), Some(Ok(()))) {
                return Err(Fail::Count { expected: count as u32, got: 0 });
            }
            let sampled = pwm::wait(budget, || sampler.poll());
            if !matches!(sampled, Some(Ok(_))) {
                return Err(Fail::Count { expected: count as u32, got: 0 });
            }
            pwm::decode(sampler.samples(), 0, awg_pattern, awg_per_bit16, false)
        })();
        report.check(
            "TIM3 CH3 PB0 (AWG)",
            "mismatched periods refuse; the parts play after re-rating [API-001]",
            awg_use,
        );

        // Tick: a rate the clock cannot divide to refuses; the timer comes
        // back. Its vector is unbound, so it pauses right away.
        let (tick, tick_refusal) = match Tick::new(&mut run.rcc, run.tim15, Hertz::mhz(7)) {
            Ok(_) => defmt::panic!("a 7 MHz tick at 200 MHz was accepted"),
            Err(refused) => {
                let error = refused.error();
                let tim15 = refused.into_parts();
                match Tick::new(&mut run.rcc, tim15, Hertz::khz(1)) {
                    Ok(mut tick) => {
                        tick.pause();
                        (tick, Some(error))
                    }
                    Err(refused) => defmt::panic!("TIM15 tick: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM15 tick",
            "an inexact rate refuses; the timer comes back and pauses [API-001]",
            match (tick_refusal, tick.rate()) {
                (Some(TimerConfigError::Inexact), Hertz(1_000)) => Ok(()),
                (Some(other), _) => Err(Fail::Timer(other)),
                (None, _) => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Timebase: same refusal; the use is the counter running.
        let (timebase, timebase_refusal) =
            match Timebase::new(&mut run.rcc, run.tim2, Hertz::mhz(7)) {
                Ok(_) => defmt::panic!("a 7 MHz timebase at 200 MHz was accepted"),
                Err(refused) => {
                    let error = refused.error();
                    let tim2 = refused.into_parts();
                    match Timebase::new(&mut run.rcc, tim2, Hertz::mhz(1)) {
                        Ok(timebase) => (timebase, Some(error)),
                        Err(refused) => defmt::panic!("TIM2 timebase: {}", refused.error()),
                    }
                }
            };
        let first = timebase.now();
        cortex_m::asm::delay(200_000);
        let second = timebase.now();
        report.check(
            "TIM2 timebase",
            "an inexact rate refuses; the timer comes back and counts [API-001]",
            match (timebase_refusal, second > first) {
                (Some(TimerConfigError::Inexact), true) => Ok(()),
                (Some(other), _) => Err(Fail::Timer(other)),
                (None, _) => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Watchdog: a window longer than the timeout refuses, then a 1 us
        // timeout refuses; the peripheral comes back and starts, feeds for
        // 300 ms, then starves. The reset the starve causes is the second
        // boot's arrival, which reports the bite and ends cleanly.
        let name = "IWDG1 50 ms";
        let dog = match Watchdog::start_windowed(
            run.iwdg,
            watchdog::TIMEOUT_US,
            watchdog::TIMEOUT_US + 10_000,
        ) {
            Ok(_) => defmt::panic!("a window longer than the timeout was accepted"),
            Err(refused) => {
                report.check(
                    name,
                    "a window longer than the timeout refuses [API-001]",
                    match refused.error() {
                        ferrohal_stm32::iwdg::Error::Window => Ok(()),
                        other => Err(Fail::Count {
                            expected: ferrohal_stm32::iwdg::Error::Window as u32,
                            got: other as u32,
                        }),
                    },
                );
                match Watchdog::start(refused.into_parts(), 1) {
                    Ok(_) => defmt::panic!("a 1 us timeout was accepted"),
                    Err(refused) => {
                        report.check(
                            name,
                            "a 1 us timeout refuses [API-001]",
                            match refused.error() {
                                ferrohal_stm32::iwdg::Error::Timeout => Ok(()),
                                other => Err(Fail::Count {
                                    expected: ferrohal_stm32::iwdg::Error::Timeout as u32,
                                    got: other as u32,
                                }),
                            },
                        );
                        Watchdog::start_with_lsi(
                            refused.into_parts(),
                            watchdog::TIMEOUT_US,
                            board::LSI_HZ,
                        )
                        .expect("the good timeout must be accepted")
                    }
                }
            }
        };
        let frozen = Watchdog::is_frozen_on_halt();
        watchdog::feed_then_starve(&mut report, name, cause, dog, cpu, frozen);
    }
}
