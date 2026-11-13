//! Run V: the constructor refusals. Every public constructor refuses on
//! hardware, hands its parts back, and the run uses them.
//!
//! - RCC: a config naming `PllP` with no PLL refuses, the RCC token comes
//!   back, and it freezes this run's clocks (56 MHz on PLLQ, so USB must
//!   refuse too). Compile-time asserts pin both errors before boot.
//! - USB: `UsbFs::new` refuses without a 48 MHz source; its two pins come
//!   back and carry a level over J2.
//! - UART: an unreachable baud refuses; the wiring and buffers open the
//!   port, which echoes a frame over J1.
//! - SPI: a 0 Hz stream config and a 0 Hz polled config refuse; the wirings
//!   transfer bytes over S1 and S3.
//! - I2C: an out-of-range speed refuses; the wiring opens the bus, which
//!   NACKs a vacant address.
//! - ADC: an empty scan sequence refuses; the converter, stream and buffer
//!   come back and scan the internal channels.
//! - PWM: a 0 rate refuses; the timer comes back and makes the lanes.
//! - Lane: a buffer in CCM RAM, which no DMA controller reaches (RM0090),
//!   refuses; the channel and stream come back and play on PB0.
//! - Sampler: a 0 rate refuses; the timer, stream and buffer come back and
//!   decode the lane playing on PB0.
//! - AWG: two timers at different periods refuse; both PWMs and both lanes
//!   come back, the second re-rated, and the pair plays PB6.
//! - Tick and Timebase: rates the clock cannot divide to refuse; the timers
//!   come back and run. The tick pauses right away, its vector unbound.
//! - Watchdog: a 1 us timeout refuses; the peripheral comes back and
//!   starts, and is fed before the run ends.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM2`, `DMA2_STREAM7` | USART1's `UartIrq` | 2 |
//! | `SDIO` | RTIC, dispatching the priority 1 `checks` task | 1 |
//!
//! Needs J1, J2, S1 and S3 fitted; nothing else.

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

/// What a `free` must leave: the handle freed at all (`handed_back`), the
/// peripheral off (`off`, read from its enable bit) and every one of its DMA
/// streams stopped. Counts what is still on.
fn freed(
    handed_back: bool,
    off: bool,
    streams: &[(stm32_metapac::dma::Dma, usize)],
) -> Result<(), tests::Fail> {
    let running = streams
        .iter()
        .filter(|(dma, stream)| dma.st(*stream).cr().read().en())
        .count() as u32;
    let on = u32::from(!handed_back) + u32::from(!off) + running;
    if on == 0 {
        Ok(())
    } else {
        Err(tests::Fail::Count {
            expected: 0,
            got: on,
        })
    }
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SDIO])]
mod app {
    use ferrohal_stm32::{
        rcc::ClockError,
        uart::{
            Buffers as UartBuffers, Config as UartConfig, ConfigError as UartConfigError, Uart,
            UartIrq,
        },
        usb::UsbFs,
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
        watchdog,
    };

    #[shared]
    struct Shared {
        // The UART's tasks run at one priority, so the handle needs no lock.
        // None until the checks task constructs the port from the parts a
        // refusal hands back.
        irq1: Option<UartIrq>,
    }

    #[local]
    struct Local {
        run: Option<board::RunV>,
        rcc_refusal: Option<ClockError>,
    }

    #[init]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let (run, rcc_refusal) = match board::run_v(cx.device) {
            Ok(both) => both,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::REFUSAL_CLOCKS);
        defmt::info!("run V, clocks: {}", board::REFUSAL_CLOCKS);
        checks::spawn().ok();
        (
            Shared { irq1: None },
            Local {
                run: Some(run),
                rcc_refusal: Some(rcc_refusal),
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_irq(mut cx: usart1_irq::Context) {
        cx.shared.irq1.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [irq1])]
    fn usart1_rx(mut cx: usart1_rx::Context) {
        cx.shared.irq1.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq1])]
    fn usart1_tx(mut cx: usart1_tx::Context) {
        cx.shared.irq1.lock(|irq| {
            if let Some(irq) = irq.as_mut() {
                irq.on_interrupt();
            }
        });
    }

    #[task(priority = 1, local = [run, rcc_refusal], shared = [irq1])]
    async fn checks(mut cx: checks::Context) {
        use embedded_hal::digital::InputPin as _;
        use embedded_hal::i2c::I2c as _;
        use embedded_hal::spi::SpiBus as _;
        use ferrohal_stm32::{
            adc::{Adc, Config as AdcConfig, Error as AdcError},
            i2c::{Config as I2cConfig, ConfigError as I2cConfigError, Error as I2cError, I2c},
            iwdg::Watchdog,
            spi::{
                self, Buffers as SpiBuffers, Config as SpiConfig, ConfigError as SpiConfigError,
            },
            time::Hertz,
            timer::{Awg, ConfigError as TimerConfigError, Pwm, Sampler, Tick, Timebase},
        };

        tests::set_cpu_hz(board::REFUSAL_CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::REFUSAL_CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        // Run J's two-boot shape: the second boot, arriving by the watchdog's
        // own reset at the end of the first, reports the bite and ends
        // cleanly; the first runs the refusals.
        let cause = run.rcc.reset_cause();
        if watchdog::after_reset(&mut report, "IWDG 50 ms", cause, (17, 47), (47_500, 52_500)) {
            let led = &mut run.led;
            tests::finish(&report, || led.set_high());
        }
        static mut SPI_TX: [u8; 64] = [0; 64];
        static mut SPI_RX: [u8; 64] = [0; 64];
        static mut LANE3: [u32; 20] = [0; 20];
        static mut LANE4: [u32; 20] = [0; 20];
        static mut LANE2: [u32; 20] = [0; 20];
        static mut SAMPLES: [u16; 1024] = [0; 1024];
        static mut SCAN: [u16; 2] = [0; 2];
        // SAFETY: `checks` runs once, and these are the only references.
        let statics = unsafe {
            (
                &mut *core::ptr::addr_of_mut!(SPI_TX),
                &mut *core::ptr::addr_of_mut!(SPI_RX),
                &mut *core::ptr::addr_of_mut!(LANE3),
                &mut *core::ptr::addr_of_mut!(LANE4),
                &mut *core::ptr::addr_of_mut!(LANE2),
                &mut *core::ptr::addr_of_mut!(SAMPLES),
                &mut *core::ptr::addr_of_mut!(SCAN),
            )
        };
        let (spi_tx, spi_rx, lane3_buf, lane4_buf, lane2_buf, samples, scan) = statics;

        // RCC: the no-PLL config refused, and the token froze these clocks.
        report.check(
            "run V",
            "a config naming PllP with no PLL refuses [API-001]",
            match cx.local.rcc_refusal.take() {
                Some(ClockError::PllNotConfigured) => Ok(()),
                Some(other) => Err(Fail::Count {
                    expected: ClockError::PllNotConfigured as u32,
                    got: other as u32,
                }),
                None => Err(Fail::Count {
                    expected: 1,
                    got: 0,
                }),
            },
        );

        // USB: without a 48 MHz source the constructor refuses, and its pins
        // come back as GPIO that still carries a level over J2.
        let (token, dm, dp) = run.usb;
        report.check(
            "USB OTG_FS PA11/PA12",
            "no 48 MHz source refuses; the pins come back over J2 [API-001]",
            match UsbFs::new(&mut run.rcc, token, dm, dp) {
                Ok(_) => Err(Fail::Count {
                    expected: 1,
                    got: 0,
                }),
                Err(refused) => {
                    let (error, (_token, dm, dp)) = refused.into_inner();
                    if error != ClockError::Usb {
                        Err(Fail::Count {
                            expected: ClockError::Usb as u32,
                            got: error as u32,
                        })
                    } else {
                        let mut feed = dm.into_push_pull_output(&mut run.ctrl_a);
                        let mut sense = dp.into_input(&mut run.ctrl_a);
                        feed.set_high();
                        cortex_m::asm::delay(84_000);
                        let high = sense.is_high().unwrap_or(false);
                        feed.set_low();
                        cortex_m::asm::delay(84_000);
                        let low = sense.is_high().unwrap_or(false);
                        defmt::info!("USB pins over J2: high {}, low {}", high, low);
                        if high && !low {
                            Ok(())
                        } else {
                            Err(Fail::Level {
                                expected_high: true,
                            })
                        }
                    }
                }
            },
        );

        // UART: a baud no clock reaches refuses; the wiring and buffers come
        // back, open the port, and echo a frame over J1.
        static mut TX: [u8; 256] = [0; 256];
        static mut RX: [u8; 256] = [0; 256];
        // SAFETY: `checks` runs once, and these are the only references.
        let (tx, rx): (&'static mut [u8], &'static mut [u8]) = unsafe {
            (
                &mut *core::ptr::addr_of_mut!(TX),
                &mut *core::ptr::addr_of_mut!(RX),
            )
        };
        let (usart1, uart_refusal) = match Uart::new(
            &mut run.rcc,
            run.usart1,
            &UartConfig::new(1),
            UartBuffers { tx, rx },
        ) {
            Ok(_) => defmt::panic!("a 1-baud UART config was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (wiring, buffers) = refused.into_parts();
                match Uart::new(&mut run.rcc, wiring, &UartConfig::new(115_200), buffers) {
                    Ok((uart, irq)) => {
                        cx.shared.irq1.lock(|handle| *handle = Some(irq));
                        (uart, Some(error))
                    }
                    Err(refused) => defmt::panic!("USART1: {}", refused.error()),
                }
            }
        };
        let mut usart1 = usart1;
        report.check(
            "USART1 PA9->PA10",
            "an unreachable baud refuses [API-001]",
            match uart_refusal {
                Some(UartConfigError::BaudRate) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::NotRefused(UartConfigError::BaudRate)),
            },
        );
        let mut loop1 = tests::Loop {
            name: "USART1 PA9->PA10",
            uart: &mut usart1,
        };
        report.check(
            loop1.name,
            "the handed-back wiring echoes a frame [API-001]",
            tests::echo(&mut loop1, b"refusal\n"),
        );
        // `free` stops the line and both streams, and hands the token back.
        let _token = usart1.free();
        report.check(
            "USART1 PA9->PA10",
            "free leaves the USART off and both streams stopped [API-002]",
            crate::freed(
                true,
                !stm32_metapac::USART1.cr1().read().ue(),
                &[(stm32_metapac::DMA2, 7), (stm32_metapac::DMA2, 2)],
            ),
        );

        // SPI: a 0 Hz config refuses for the stream and the polled driver,
        // and the handed-back wirings transfer over S1 and S3.
        let sent = b"refusal run";
        let mut spi_freed = false;
        report.check(
            "SPI3 PC10/PC11/PC12",
            "a 0 Hz config refuses; the wiring transfers over S5 [API-001]",
            match spi::Spi::new(
                &mut run.rcc,
                run.spi3,
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
                            Ok(mut spi) => {
                                let echoed = match spi.transfer(sent) {
                                    Ok(got) if got == sent => Ok(()),
                                    Ok(_) => Err(Fail::Count {
                                        expected: sent.len() as u32,
                                        got: 0,
                                    }),
                                    Err(error) => Err(Fail::Spi(error)),
                                };
                                let _token = spi.free();
                                spi_freed = true;
                                echoed
                            }
                            Err(refused) => Err(Fail::SpiConfig(refused.error())),
                        }
                    }
                }
            },
        );
        report.check(
            "SPI3 PC10/PC11/PC12",
            "free leaves the SPI off and both streams stopped [API-002]",
            crate::freed(
                spi_freed,
                !stm32_metapac::SPI3.cr1().read().spe(),
                &[(stm32_metapac::DMA1, 5), (stm32_metapac::DMA1, 2)],
            ),
        );
        let mut polled_freed = false;
        report.check(
            "SPI2 PB13/PB14/PB15 (polled)",
            "a 0 Hz config refuses; the wiring writes over S3 [API-001]",
            match spi::BlockingSpi::new(&mut run.rcc, run.spi2_polled, &SpiConfig::new(Hertz(0))) {
                Ok(_) => Err(Fail::SpiConfig(SpiConfigError::Frequency)),
                Err(refused) => {
                    if refused.error() != SpiConfigError::Frequency {
                        Err(Fail::SpiConfig(refused.error()))
                    } else {
                        let wiring = refused.into_parts();
                        match spi::BlockingSpi::new(&mut run.rcc, wiring, &board::SPI) {
                            Ok(mut spi) => {
                                let written = spi.write(sent).map_err(Fail::Spi);
                                let _token = spi.free();
                                polled_freed = true;
                                written
                            }
                            Err(refused) => Err(Fail::SpiConfig(refused.error())),
                        }
                    }
                }
            },
        );

        // I2C: an out-of-range speed refuses; the wiring opens the bus,
        // which NACKs a vacant address all by itself.
        report.check(
            "SPI2 PB13/PB14/PB15 (polled)",
            "free leaves the SPI off [API-002]",
            crate::freed(polled_freed, !stm32_metapac::SPI2.cr1().read().spe(), &[]),
        );
        let mut i2c_freed = false;
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
                            Ok(mut bus) => {
                                let nacked = match bus.write(0x1E, &[0xA5]) {
                                    Err(I2cError::Nack) => Ok(()),
                                    Err(error) => Err(Fail::I2c(error)),
                                    Ok(()) => Err(Fail::I2cNotNack(None)),
                                };
                                let _token = bus.free();
                                i2c_freed = true;
                                nacked
                            }
                            Err(refused) => Err(Fail::I2cConfig(refused.error())),
                        }
                    }
                }
            },
        );

        report.check(
            "I2C3 PA8/PC9",
            "free leaves the I2C off [API-002]",
            crate::freed(i2c_freed, !stm32_metapac::I2C3.cr1().read().pe(), &[]),
        );

        // ADC: an empty sequence refuses; the converter, stream and buffer
        // come back and scan the internal channels.
        let adc = match Adc::new(&mut run.rcc, run.adc1, &AdcConfig::slow_source()) {
            Ok(adc) => adc,
            Err(refused) => defmt::panic!("ADC1: {}", refused.error()),
        };
        let channels = [adc.vrefint_channel(), adc.temperature_channel()];
        report.check(
            "ADC1 scan",
            "an empty sequence refuses; the parts scan the internals [API-001]",
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
                        let Ok(mut scan) = adc.into_scan(stream, &channels, buffer) else {
                            defmt::panic!("the scan")
                        };
                        let mut result = Err(Fail::Count {
                            expected: 1,
                            got: 0,
                        });
                        if scan.start().is_ok() {
                            let mut spins = cpu.0 / 8;
                            loop {
                                if let Some(done) = scan.poll() {
                                    if let Ok(values) = done {
                                        let [reference, temperature] = values else {
                                            break;
                                        };
                                        result = if (1300..=1700).contains(reference)
                                            && (800..=1200).contains(temperature)
                                        {
                                            Ok(())
                                        } else {
                                            Err(Fail::Count {
                                                expected: u32::from(*reference),
                                                got: u32::from(*temperature),
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

        // PWM: a 0 rate refuses; the timer comes back at 600 kHz.
        let (pwm3, pwm3_refusal) = match Pwm::new(&mut run.rcc, run.tim3, Hertz(0)) {
            Ok(_) => defmt::panic!("a 0-rate PWM config was accepted"),
            Err(refused) => {
                let error = refused.error();
                let tim3 = refused.into_parts();
                match Pwm::new(&mut run.rcc, tim3, Hertz::khz(600)) {
                    Ok(pwm) => (pwm, Some(error)),
                    Err(refused) => defmt::panic!("TIM3 PWM: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM3 PWM",
            "a 0 rate refuses; the timer comes back at 600 kHz [API-001]",
            match pwm3_refusal {
                Some(TimerConfigError::Inexact) if pwm3.frequency() == Hertz::khz(600) => Ok(()),
                Some(TimerConfigError::Inexact) => Err(Fail::Count {
                    expected: 600_000,
                    got: pwm3.frequency().0,
                }),
                Some(other) => Err(Fail::Timer(other)),
                None => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Lane: a buffer in CCM RAM, which no DMA controller reaches
        // (RM0090), refuses; the channel and stream come back and play.
        // SAFETY: the F401's CCM RAM at 0x10000000 is real memory the CPU
        // can use, but no DMA controller reaches it, and the constructor
        // refuses the buffer before programming anything. Nothing else
        // touches it.
        let ccm: &'static mut [u32] =
            unsafe { core::slice::from_raw_parts_mut(0x1000_0000 as *mut u32, 16) };
        let (mut lane3, lane_refusal) = match pwm3
            .channel::<3, _>(run.ch3)
            .into_lane(run.lane3_stream, ccm)
        {
            Ok(_) => defmt::panic!("a CCM buffer was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (channel, stream, _ccm) = refused.into_parts();
                match channel.into_lane(stream, lane3_buf) {
                    Ok(lane) => (lane, Some(error)),
                    Err(refused) => defmt::panic!("the TIM3 lane: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM3 CH3",
            "a CCM buffer refuses; the channel and stream come back [API-001]",
            match lane_refusal {
                Some(UartConfigError::BufferUnreachable) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::Config(UartConfigError::BufferUnreachable)),
            },
        );

        // Sampler: a 0 rate refuses; the timer, stream and buffer come back.
        let (mut sampler, sampler_refusal) = match Sampler::new::<_, 'B'>(
            &mut run.rcc,
            run.tim1,
            run.sampler_stream,
            Hertz(0),
            samples,
        ) {
            Ok(_) => defmt::panic!("a 0-rate sampler was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (tim1, stream, buffer) = refused.into_parts();
                match Sampler::new::<_, 'B'>(&mut run.rcc, tim1, stream, Hertz::mhz(1), buffer) {
                    Ok(sampler) => (sampler, Some(error)),
                    Err(refused) => defmt::panic!("the TIM1 sampler: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM1 sampler",
            "a 0 rate refuses; the timer, stream and buffer come back [API-001]",
            match sampler_refusal {
                Some(UartConfigError::Unsupported) => Ok(()),
                Some(other) => Err(Fail::Config(other)),
                None => Err(Fail::Config(UartConfigError::Unsupported)),
            },
        );

        // The use: the handed-back lane plays, and the handed-back sampler
        // decodes it from PB0, as run R does.
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
                .arm(&pwm3, &pwm::duties(pattern, pwm3.period()))
                .map_err(|_| Fail::Count {
                    expected: 16,
                    got: 0,
                })?;
            sampler.start(count).map_err(|_| Fail::Count {
                expected: count as u32,
                got: 0,
            })?;
            pwm3.stage(0b0100);
            pwm3.start();
            let done = pwm::wait(budget, || lane3.poll());
            let sampled = pwm::wait(budget, || sampler.poll());
            pwm3.stop();
            pwm3.close_requests();
            if !matches!(done, Some(Ok(()))) || !matches!(sampled, Some(Ok(_))) {
                return Err(Fail::Count {
                    expected: 2,
                    got: 0,
                });
            }
            pwm::decode(sampler.samples(), 0, pattern, per_bit16, false)
        })();
        report.check(
            "TIM3 CH3 PB0",
            "the handed-back lane and sampler play and decode [API-001]",
            lane_use,
        );

        // AWG: two timers at different periods refuse; both PWMs and both
        // lanes come back, the second re-rated, and the pair plays PB6.
        let pwm4 = match Pwm::new(&mut run.rcc, run.tim4, Hertz::khz(600)) {
            Ok(pwm) => pwm,
            Err(refused) => defmt::panic!("TIM4 PWM: {}", refused.error()),
        };
        let lane4 = match pwm4
            .channel::<1, _>(run.pb6)
            .into_lane(run.lane4_stream, lane4_buf)
        {
            Ok(lane) => lane,
            Err(refused) => defmt::panic!("the TIM4 lane: {}", refused.error()),
        };
        let (mut awg, awg_refusal) = match Pwm::new(&mut run.rcc, run.tim2, Hertz::khz(100)) {
            Ok(pwm2) => {
                let lane2 = match pwm2
                    .channel::<3, _>(run.pb10)
                    .into_lane(run.lane2_stream, lane2_buf)
                {
                    Ok(lane) => lane,
                    Err(refused) => defmt::panic!("the TIM2 lane: {}", refused.error()),
                };
                match Awg::independent(pwm4, pwm2, (lane4,), (lane2,)) {
                    Ok(_) => defmt::panic!("mismatched periods were accepted"),
                    Err(refused) => {
                        let error = refused.error();
                        let (pwm4, mut pwm2, lane4, lane2) = refused.into_parts();
                        if pwm2.set_frequency(Hertz::khz(600)).is_err() {
                            defmt::panic!("re-rating TIM2");
                        }
                        match Awg::independent(pwm4, pwm2, lane4, lane2) {
                            Ok(awg) => (awg, Some(error)),
                            Err(refused) => defmt::panic!("the AWG: {}", refused.error()),
                        }
                    }
                }
            }
            Err(refused) => defmt::panic!("TIM2 PWM: {}", refused.error()),
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
                return Err(Fail::Count {
                    expected: 16,
                    got: 0,
                });
            }
            let awg_per_bit16 =
                (u64::from(sampler.rate().0) * 16 / u64::from(awg.sample_rate().0)) as u32;
            let count = (awg_per_bit16 as usize * (16 + 8)) / 16;
            sampler.start(count).map_err(|_| Fail::Count {
                expected: count as u32,
                got: 0,
            })?;
            if awg.play().is_err() {
                return Err(Fail::Count {
                    expected: 1,
                    got: 0,
                });
            }
            if !matches!(pwm::wait(budget, || awg.poll(0)), Some(Ok(()))) {
                return Err(Fail::Count {
                    expected: count as u32,
                    got: 0,
                });
            }
            let sampled = pwm::wait(budget, || sampler.poll());
            if !matches!(sampled, Some(Ok(_))) {
                return Err(Fail::Count {
                    expected: count as u32,
                    got: 0,
                });
            }
            pwm::decode(sampler.samples(), 6, awg_pattern, awg_per_bit16, false)
        })();
        report.check(
            "TIM4 CH1 PB6",
            "mismatched periods refuse; the parts play after re-rating [API-001]",
            awg_use,
        );
        // The same generator, TIM2 started in software beside TIM4: every
        // lane of both finishes.
        pwm::awg_pair_check(&mut report, "TIM4 + TIM2, software start", &mut awg, cpu);

        // Tick: a rate the clock cannot divide to refuses; the timer comes
        // back. Its vector is unbound, so it pauses right away.
        let (tick, tick_refusal) = match Tick::new(&mut run.rcc, run.tim9, Hertz::mhz(5)) {
            Ok(_) => defmt::panic!("a 5 MHz tick was accepted"),
            Err(refused) => {
                let error = refused.error();
                let tim9 = refused.into_parts();
                match Tick::new(&mut run.rcc, tim9, Hertz::khz(1)) {
                    Ok(mut tick) => {
                        tick.pause();
                        (tick, Some(error))
                    }
                    Err(refused) => defmt::panic!("TIM9 tick: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM9 tick",
            "an inexact rate refuses; the timer comes back and pauses [API-001]",
            match (tick_refusal, tick.rate()) {
                (Some(TimerConfigError::Inexact), Hertz(1_000)) => Ok(()),
                (Some(other), _) => Err(Fail::Timer(other)),
                (None, _) => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Timebase: same refusal; the use is the counter running.
        let (timebase, timebase_refusal) =
            match Timebase::new(&mut run.rcc, run.tim5, Hertz::mhz(5)) {
                Ok(_) => defmt::panic!("a 5 MHz timebase was accepted"),
                Err(refused) => {
                    let error = refused.error();
                    let tim5 = refused.into_parts();
                    match Timebase::new(&mut run.rcc, tim5, Hertz::mhz(1)) {
                        Ok(timebase) => (timebase, Some(error)),
                        Err(refused) => defmt::panic!("TIM5 timebase: {}", refused.error()),
                    }
                }
            };
        let first = timebase.now();
        cortex_m::asm::delay(84_000);
        let second = timebase.now();
        report.check(
            "TIM5 timebase",
            "an inexact rate refuses; the timer comes back and counts [API-001]",
            match (timebase_refusal, second > first) {
                (Some(TimerConfigError::Inexact), true) => Ok(()),
                (Some(other), _) => Err(Fail::Timer(other)),
                (None, _) => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Watchdog: a 1 us timeout refuses; the peripheral comes back and
        // starts. From there run J's shape takes over: feed for 300 ms,
        // then starve, and the second boot reports the bite. This way the
        // run's own reset clears its flags, and no live watchdog outlives
        // probe-rs to leave one behind for the next bench run.
        let (dog, watchdog_refusal) = match Watchdog::start(run.iwdg, 1) {
            Ok(_) => defmt::panic!("a 1 us watchdog was accepted"),
            Err(refused) => {
                let error = refused.error();
                let iwdg = refused.into_parts();
                match Watchdog::start_with_lsi(iwdg, watchdog::TIMEOUT_US, board::LSI_HZ) {
                    Ok(dog) => (dog, Some(error)),
                    Err(refused) => defmt::panic!("the watchdog: {}", refused.error()),
                }
            }
        };
        report.check(
            "IWDG",
            "a 1 us timeout refuses; the peripheral comes back and starts [API-001]",
            match watchdog_refusal {
                Some(ferrohal_stm32::iwdg::Error::Timeout) => Ok(()),
                Some(other) => Err(Fail::Count {
                    expected: ferrohal_stm32::iwdg::Error::Timeout as u32,
                    got: other as u32,
                }),
                None => Err(Fail::Count {
                    expected: 1,
                    got: 0,
                }),
            },
        );
        let frozen = Watchdog::is_frozen_on_halt();
        watchdog::feed_then_starve(
            &mut report,
            "IWDG 50 ms",
            cause,
            dog,
            board::REFUSAL_CLOCKS.cpu,
            frozen,
        );
    }
}
