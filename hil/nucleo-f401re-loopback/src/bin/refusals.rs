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
//! - SPI: a 0 Hz DMA config and a 0 Hz polled config refuse; the wirings
//!   transfer bytes over S1 and S3.
//! - I2C: an out-of-range speed refuses; the wiring opens the bus, which
//!   NACKs a vacant address.
//! - ADC: an empty scan sequence refuses; the converter, DMA channel and buffer
//!   come back and scan the internal channels.
//! - PWM: a 0 rate refuses; the timer comes back and makes the lanes.
//! - Lane: a buffer in CCM RAM, which no DMA controller reaches (RM0090),
//!   refuses; the pin and DMA channel come back and play on PB0.
//! - Sampler: a 0 rate refuses; the timer, DMA channel and buffer come back and
//!   decode the lane playing on PB0.
//! - lane group: two timers at different periods refuse; both PWMs and both lanes
//!   come back, the second re-rated, and the pair plays PB6.
//! - Tick and Timebase: rates the clock cannot divide to refuse; the timers
//!   come back and run. The tick pauses right away, its vector unbound.
//! - Watchdog: a 1 us timeout refuses; the peripheral comes back and
//!   starts, and is fed before the run ends.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM2`, `DMA2_STREAM7` | USART1's `on_interrupt` | 2 |
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
/// channels stopped. Counts what is still on.
fn freed(
    handed_back: bool,
    off: bool,
    channels: &[(stm32_metapac::dma::Dma, usize)],
) -> Result<(), tests::Fail> {
    let running = channels
        .iter()
        .filter(|(dma, channel)| dma.st(*channel).cr().read().en())
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
        },
        usb::{self, UsbFs},
    };

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
        watchdog,
    };

    #[shared]
    struct Shared {
        // None until the checks task constructs the port from the parts a
        // refusal hands back, and again once it frees it.
        usart1: Option<Uart<board::Usart1LoopCh2>>,
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
            Shared { usart1: None },
            Local {
                run: Some(run),
                rcc_refusal: Some(rcc_refusal),
            },
        )
    }

    #[task(binds = USART1, priority = 2, shared = [usart1])]
    fn usart1_irq(mut cx: usart1_irq::Context) {
        cx.shared.usart1.lock(|slot| {
            if let Some(uart) = slot.as_mut() {
                tests::serve(uart);
            }
        });
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [usart1])]
    fn usart1_rx(mut cx: usart1_rx::Context) {
        cx.shared.usart1.lock(|slot| {
            if let Some(uart) = slot.as_mut() {
                tests::serve(uart);
            }
        });
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [usart1])]
    fn usart1_tx(mut cx: usart1_tx::Context) {
        cx.shared.usart1.lock(|slot| {
            if let Some(uart) = slot.as_mut() {
                tests::serve(uart);
            }
        });
    }

    #[task(priority = 1, local = [run, rcc_refusal], shared = [usart1])]
    async fn checks(mut cx: checks::Context) {
        use embedded_hal::digital::InputPin as _;
        use embedded_hal::i2c::I2c as _;
        use embedded_hal::spi::SpiBus as _;
        use ferrohal_stm32::{
            adc::{Adc, Config as AdcConfig, ConfigError as AdcConfigError},
            i2c::{Config as I2cConfig, ConfigError as I2cConfigError, Error as I2cError, I2c},
            iwdg::Watchdog,
            spi::{
                self, Buffers as SpiBuffers, Config as SpiConfig, ConfigError as SpiConfigError,
            },
            time::Hertz,
            timer::{LaneGroup, ConfigError as TimerConfigError, Pwm, Sampler, Tick, Timebase},
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
        let (usb, dm, dp) = run.usb;
        report.check(
            "USB OTG_FS PA11/PA12",
            "no 48 MHz source refuses; the pins come back over J2 [API-001]",
            match UsbFs::new(&mut run.rcc, usb::Wiring { usb, dm, dp }) {
                Ok(_) => Err(Fail::Count {
                    expected: 1,
                    got: 0,
                }),
                Err(refused) => {
                    let (error, usb::Wiring { dm, dp, .. }) = refused.into_inner();
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
            &UartConfig::new(Hertz(1)),
            UartBuffers { tx, rx },
        ) {
            Ok(_) => defmt::panic!("a 1-baud UART config was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (wiring, buffers) = refused.into_parts();
                match Uart::new(
                    &mut run.rcc,
                    wiring,
                    &UartConfig::new(Hertz(115_200)),
                    buffers,
                ) {
                    Ok(uart) => (uart, Some(error)),
                    Err(refused) => defmt::panic!("USART1: {}", refused.error()),
                }
            }
        };
        cx.shared.usart1.lock(|slot| *slot = Some(usart1));
        let mut usart1 = tests::Locked(tests::Slot::new(&mut cx.shared.usart1));
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
        // `free` stops the line and both DMA channels, and hands the token back.
        let handed_back = cx
            .shared
            .usart1
            // Freed inside the lock: once out of the slot no handler can
            // serve it, so an event landing before free masks the line
            // would raise the vector with nothing to clear it.
            .lock(|slot| slot.take().is_some_and(|uart| uart.free().is_ok()));
        report.check(
            "USART1 PA9->PA10",
            "free leaves the USART off, TCIE off and both DMA channels stopped [API-002]",
            crate::freed(
                handed_back,
                {
                    let cr1 = stm32_metapac::USART1.cr1().read();
                    // TCIE off too: a drain left armed would raise the vector
                    // for ever once nothing serves it.
                    !cr1.ue() && !cr1.tcie()
                },
                &[(stm32_metapac::DMA2, 7), (stm32_metapac::DMA2, 2)],
            ),
        );

        // SPI: a 0 Hz config refuses for the DMA driver and the polled driver,
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
                                spi_freed = spi.free().is_ok();
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
            "free leaves the SPI off and both DMA channels stopped [API-002]",
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
                Ok(_) => Err(Fail::I2cConfig(I2cConfigError::Frequency)),
                Err(refused) => {
                    if refused.error() != I2cConfigError::Frequency {
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

        // ADC: an empty sequence refuses; the converter, DMA channel and buffer
        // come back and scan the internal channels.
        let adc = match Adc::new(&mut run.rcc, run.adc1, &AdcConfig::slow_source()) {
            Ok(adc) => adc,
            Err(refused) => defmt::panic!("ADC1: {}", refused.error()),
        };
        let channels = [adc.vrefint_channel(), adc.temperature_channel()];
        report.check(
            "ADC1 scan",
            "an empty sequence refuses; the parts scan the internals [API-001]",
            match adc.into_scan(run.scan_dma, &[], scan) {
                Ok(_) => Err(Fail::AdcConfig(AdcConfigError::Sequence)),
                Err(refused) => {
                    if refused.error() != AdcConfigError::Sequence {
                        Err(Fail::AdcConfig(refused.error()))
                    } else {
                        let (adc, dma, buffer) = refused.into_parts();
                        let Ok(mut scan) = adc.into_scan(dma, &channels, buffer) else {
                            defmt::panic!("the scan")
                        };
                        let mut result = Err(Fail::Count {
                            expected: 1,
                            got: 0,
                        });
                        if scan.start().is_ok() {
                            let mut spins = cpu.0 / 8;
                            loop {
                                if let Some(done) = scan.on_interrupt() {
                                    if done.is_ok() {
                                        let [reference, temperature] = scan.samples() else {
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
                Some(TimerConfigError::Inexact) if pwm3.rate() == Hertz::khz(600) => Ok(()),
                Some(TimerConfigError::Inexact) => Err(Fail::Count {
                    expected: 600_000,
                    got: pwm3.rate().0,
                }),
                Some(other) => Err(Fail::Timer(other)),
                None => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // Lane: assembly takes no memory, so there is no buffer to refuse
        // here. A frame no DMA reaches is refused at `arm`, but the F401 has
        // no such RAM: it has no CCM, and DMA2 reaches all its SRAM
        // (RM0368). The H753's run U checks that refusal on DTCM.
        let mut lane3 =
            match ferrohal_stm32::timer::LaneWiring::<_, 3, _, _>::new(run.ch3, run.lane3_dma)
                .assemble(&pwm3)
            {
                Ok(lane) => lane,
                Err(refused) => defmt::panic!("the TIM3 lane: {}", refused.error()),
            };
        pwm::lend_frames(lane3_buf);
        pwm::lend_frames(lane4_buf);
        pwm::lend_frames(lane2_buf);

        // Sampler: a 0 rate refuses; the timer, DMA channel and buffer come back.
        let (mut sampler, sampler_refusal) = match Sampler::new::<'B'>(
            &mut run.rcc,
            run.tim1,
            run.sampler_dma,
            Hertz(0),
            samples,
        ) {
            Ok(_) => defmt::panic!("a 0-rate sampler was accepted"),
            Err(refused) => {
                let error = refused.error();
                let (tim1, dma, buffer) = refused.into_parts();
                match Sampler::new::<'B'>(&mut run.rcc, tim1, dma, Hertz::mhz(1), buffer) {
                    Ok(sampler) => (sampler, Some(error)),
                    Err(refused) => defmt::panic!("the TIM1 sampler: {}", refused.error()),
                }
            }
        };
        report.check(
            "TIM1 sampler",
            "a 0 rate refuses; the timer, DMA channel and buffer come back [API-001]",
            match sampler_refusal {
                Some(TimerConfigError::Inexact) => Ok(()),
                Some(other) => Err(Fail::Timer(other)),
                None => Err(Fail::Timer(TimerConfigError::Inexact)),
            },
        );

        // The use: the handed-back lane plays, and the handed-back sampler
        // decodes it from PB0, as run R does.
        let pattern: u16 = 0x3c96;
        let budget = cpu.0 / 100;
        let lane_use = (|| {
            sampler
                .set_rate(Hertz(pwm3.rate().0 * 12))
                .map_err(Fail::Timer)?;
            let per_bit16 = (u64::from(sampler.rate().0) * 16 / u64::from(pwm3.rate().0)) as u32;
            let count = (per_bit16 as usize * (16 + 8)) / 16;
            pwm::arm(&mut lane3, &pwm3, &pwm::duties(pattern, pwm3.period()))
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
            let done = pwm::wait(budget, || lane3.on_interrupt());
            let sampled = pwm::wait(budget, || sampler.on_interrupt());
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

        // lane group: two timers at different periods refuse; both PWMs and both
        // lanes come back, the second re-rated, and the pair plays PB6.
        let pwm4 = match Pwm::new(&mut run.rcc, run.tim4, Hertz::khz(600)) {
            Ok(pwm) => pwm,
            Err(refused) => defmt::panic!("TIM4 PWM: {}", refused.error()),
        };
        let lane4 =
            match ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(run.pb6, run.lane4_dma)
                .assemble(&pwm4)
            {
                Ok(lane) => lane,
                Err(refused) => defmt::panic!("the TIM4 lane: {}", refused.error()),
            };
        let (mut lane_group, lane_group_refusal) = match Pwm::new(&mut run.rcc, run.tim2, Hertz::khz(100)) {
            Ok(pwm2) => {
                let lane2 = match ferrohal_stm32::timer::LaneWiring::<_, 3, _, _>::new(
                    run.pb10,
                    run.lane2_dma,
                )
                .assemble(&pwm2)
                {
                    Ok(lane) => lane,
                    Err(refused) => defmt::panic!("the TIM2 lane: {}", refused.error()),
                };
                match LaneGroup::independent(pwm4, pwm2, (lane4,), (lane2,)) {
                    Ok(_) => defmt::panic!("mismatched periods were accepted"),
                    Err(refused) => {
                        let error = refused.error();
                        let (pwm4, mut pwm2, lane4, lane2) = refused.into_parts();
                        if pwm2.set_rate(Hertz::khz(600)).is_err() {
                            defmt::panic!("re-rating TIM2");
                        }
                        match LaneGroup::independent(pwm4, pwm2, lane4, lane2) {
                            Ok(lane_group) => (lane_group, Some(error)),
                            Err(refused) => defmt::panic!("the lane group: {}", refused.error()),
                        }
                    }
                }
            }
            Err(refused) => defmt::panic!("TIM2 PWM: {}", refused.error()),
        };
        let lane_group_pattern: u16 = 0xa5c3;
        let lane_group_use = (|| {
            if lane_group_refusal != Some(TimerConfigError::PeriodMismatch) {
                return Err(Fail::Timer(TimerConfigError::PeriodMismatch));
            }
            sampler
                .set_rate(Hertz(lane_group.sample_rate().0 * 12))
                .map_err(Fail::Timer)?;
            let period = lane_group.period();
            if pwm::load(&mut lane_group, 0, &pwm::duties(lane_group_pattern, period)).is_err() {
                return Err(Fail::Count {
                    expected: 16,
                    got: 0,
                });
            }
            let lane_group_per_bit16 =
                (u64::from(sampler.rate().0) * 16 / u64::from(lane_group.sample_rate().0)) as u32;
            let count = (lane_group_per_bit16 as usize * (16 + 8)) / 16;
            sampler.start(count).map_err(|_| Fail::Count {
                expected: count as u32,
                got: 0,
            })?;
            if lane_group.play().is_err() {
                return Err(Fail::Count {
                    expected: 1,
                    got: 0,
                });
            }
            if !matches!(
                pwm::wait(budget, || lane_group.on_interrupt().map(|r| r.map(|_| ()))),
                Some(Ok(()))
            ) {
                return Err(Fail::Count {
                    expected: count as u32,
                    got: 0,
                });
            }
            let sampled = pwm::wait(budget, || sampler.on_interrupt());
            if !matches!(sampled, Some(Ok(_))) {
                return Err(Fail::Count {
                    expected: count as u32,
                    got: 0,
                });
            }
            pwm::decode(sampler.samples(), 6, lane_group_pattern, lane_group_per_bit16, false)
        })();
        report.check(
            "TIM4 CH1 PB6",
            "mismatched periods refuse; the parts play after re-rating [API-001]",
            lane_group_use,
        );
        // The same lane group, TIM2 started in software beside TIM4: every
        // lane of both finishes.
        pwm::lane_group_pair_check(&mut report, "TIM4 + TIM2, software start", &mut lane_group, cpu);

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
                Some(ferrohal_stm32::iwdg::ConfigError::Timeout) => Ok(()),
                Some(other) => Err(Fail::Count {
                    expected: ferrohal_stm32::iwdg::ConfigError::Timeout as u32,
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
