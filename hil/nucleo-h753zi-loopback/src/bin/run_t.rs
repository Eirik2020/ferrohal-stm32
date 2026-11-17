//! Run T: DMA streams that will not stop. Built with the library's
//! `fault-injection` feature: `dma::fault::stick` makes a stream ignore stop
//! requests, so a running one keeps EN set and every bounded stop on it
//! times out, as a stream that will not stop would. Run U's parts.
//!
//! - USART6 over Z2: the transmit stream sticks with DMAT off, so it waits
//!   for requests; the flush times out, cannot stop it and reports Halted
//!   [UARTDMA-009]. DMAT back on, the stuck stream drains its frame into
//!   the receiver, which carries on; once it has stopped, the transmit side
//!   still refuses, until a free and a reopen. Then the receive ring sticks:
//!   `free` refuses and hands the UART back, its reads Halted, the buffers
//!   kept [API-003]; released, `free` succeeds. Last, a receive stream an
//!   earlier owner left running (re-armed by hand at a scratch buffer)
//!   sticks: `Uart::new` refuses with DmaBusy; released, it opens.
//! - SPI1 over S1, at 390 kHz: `free` mid-transfer with the receive stream
//!   stuck refuses [API-003]; released, it succeeds.
//! - TIM3 CH3's lane, armed with the timer stopped (its stream waits for
//!   requests): `free` refuses with the stream stuck.
//! - TIM8's sampler at 10 kHz, mid-capture: as the lane.
//! - ADC1's scan, its stream re-armed by hand after a sequence (it waits
//!   for requests no sequence makes): as the lane.
//!
//! Each refusal is checked with the stream still running (its EN read
//! directly), and each retried free with every stream of the handle
//! stopped.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA1_STREAM6`, `DMA1_STREAM1` | USART6's `UartIrq` | 2 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

use stm32_metapac::dma::Dma;

use tests::Fail;

/// `Ok` when every condition of a check held.
fn held(held: bool) -> Result<(), Fail> {
    if held {
        Ok(())
    } else {
        Err(Fail::Count {
            expected: 1,
            got: 0,
        })
    }
}

/// The stream's EN, read directly.
fn running(dma: Dma, stream: usize) -> bool {
    dma.st(stream).cr().read().en()
}

/// Re-enables a stream the driver has stopped, as an earlier owner might
/// have left it: its configuration stays, `len` more transfers, waiting for
/// requests that do not come. `memory`, if given, replaces its target.
fn rearm(dma: Dma, stream: usize, memory: Option<u32>, len: u16) {
    let st = dma.st(stream);
    if let Some(address) = memory {
        st.m0ar().write(|w| *w = address);
    }
    st.ndtr().write(|w| w.set_ndt(len));
    st.cr().modify(|w| w.set_en(true));
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        dma::{StopTimeout, fault},
        uart::{Buffers as UartBuffers, Config as UartConfig, ConfigError, Error, Uart, UartIrq},
    };
    use stm32_metapac::{DMA1, DMA2};

    use crate::{
        board, held, rearm, running,
        tests::{self, Fail, Link, Loop, Report},
    };

    #[shared]
    struct Shared {
        irq6: Option<UartIrq>,
    }

    #[local]
    struct Local {
        run: Option<board::RunU>,
    }

    #[init]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let (run, _) = match board::run_u(cx.device) {
            Ok(both) => both,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run T, clocks: {}", board::CLOCKS);
        checks::spawn().ok();
        (Shared { irq6: None }, Local { run: Some(run) })
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

    #[task(priority = 1, local = [run], shared = [irq6])]
    async fn checks(mut cx: checks::Context) {
        use ferrohal_stm32::{
            adc::{Adc, Config as AdcConfig},
            spi::{self, Buffers as SpiBuffers, Config as SpiConfig},
            time::Hertz,
            timer::{LaneWiring, Pwm, Sampler},
        };

        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        static mut TX: [u8; 256] = [0; 256];
        static mut RX: [u8; 256] = [0; 256];
        static mut SCRATCH: [u8; 16] = [0; 16];
        static mut SPI_TX: [u8; 256] = [0; 256];
        static mut SPI_RX: [u8; 256] = [0; 256];
        static mut LANE: [u32; 20] = [0; 20];
        static mut SAMPLES: [u16; 1024] = [0; 1024];
        static mut SCAN: [u16; 2] = [0; 2];
        // SAFETY: `checks` runs once, and these are the only references.
        // SCRATCH is only ever an address handed to a stream.
        let (tx, rx, scratch, spi_tx, spi_rx, lane_buf, samples, scan_buf) = unsafe {
            (
                &mut *core::ptr::addr_of_mut!(TX),
                &mut *core::ptr::addr_of_mut!(RX),
                core::ptr::addr_of_mut!(SCRATCH) as u32,
                &mut *core::ptr::addr_of_mut!(SPI_TX),
                &mut *core::ptr::addr_of_mut!(SPI_RX),
                &mut *core::ptr::addr_of_mut!(LANE),
                &mut *core::ptr::addr_of_mut!(SAMPLES),
                &mut *core::ptr::addr_of_mut!(SCAN),
            )
        };

        // USART6: DMA1 stream 6 transmits, stream 1 receives.
        const UART: &str = "USART6 PC6->PC7";
        let config = UartConfig::new(Hertz(115_200));
        let open = |rcc: &mut ferrohal_stm32::rcc::Rcc, wiring, buffers| {
            Uart::new(rcc, wiring, &config, buffers)
        };
        let (mut usart6, irq) = match open(&mut run.rcc, run.usart6, UartBuffers { tx, rx }) {
            Ok(opened) => opened,
            Err(refused) => defmt::panic!("USART6: {}", refused.error()),
        };
        cx.shared.irq6.lock(|handle| *handle = Some(irq));
        {
            let mut link = Loop {
                name: UART,
                uart: &mut usart6,
            };
            report.check(UART, "echo before any fault", tests::echo(&mut link, b"before\n"));

            fault::stick(1, 6);
            stm32_metapac::USART6.cr3().modify(|w| w.set_dmat(false));
            let frame = *b"stuck frame 0123";
            let flushed = link
                .tx()
                .write_all(&frame)
                .and_then(|()| link.tx().flush());
            report.check_for(
                UART,
                "UARTDMA-009",
                "a transmit stream that will not stop: the flush reports Halted",
                match flushed {
                    Err(Error::Halted) if running(DMA1, 6) => Ok(()),
                    Err(Error::Halted) => Err(Fail::Count {
                        expected: 1,
                        got: 0,
                    }),
                    Err(error) => Err(Fail::Uart(error)),
                    Ok(()) => Err(Fail::NotReported(Error::Halted)),
                },
            );
            // Requests back: the stream that would not stop drains the
            // frame into the loop, and the receiver takes it.
            stm32_metapac::USART6.cr3().modify(|w| w.set_dmat(true));
            let mut got = [0u8; 32];
            let mut len = 0;
            let patience = link.patience();
            while len < frame.len() {
                match link.rx().read_within(&mut got[len..], patience) {
                    Ok(n) => len += n,
                    Err(_) => break,
                }
            }
            defmt::info!("{=str}: after the stuck flush, {=usize} byte(s): {=[u8]:02x}", UART, len, &got[..len]);
            report.check_for(
                UART,
                "UARTDMA-009",
                "reception carries on: the stuck stream's frame arrives whole",
                held(got[..len] == frame),
            );
            fault::release(1, 6);
            let stopped = !running(DMA1, 6);
            let again = link.tx().write_all(b"x").and_then(|()| link.tx().flush());
            report.check_for(
                UART,
                "UARTDMA-009",
                "once the stream has stopped, the transmit side still refuses",
                match again {
                    Err(Error::Halted) if stopped && !running(DMA1, 6) => Ok(()),
                    Err(Error::Halted) => Err(Fail::Count {
                        expected: 0,
                        got: 1,
                    }),
                    Err(error) => Err(Fail::Uart(error)),
                    Ok(()) => Err(Fail::NotReported(Error::Halted)),
                },
            );
        }
        let Ok((wiring, buffers)) = usart6.free() else {
            report.check(UART, "free once both streams stopped", Err(Fail::Count { expected: 1, got: 0 }));
            tests::finish(&report, || {})
        };
        let (mut usart6, irq) = match open(&mut run.rcc, wiring, buffers) {
            Ok(opened) => opened,
            Err(refused) => defmt::panic!("USART6 reopen: {}", refused.error()),
        };
        cx.shared.irq6.lock(|handle| *handle = Some(irq));
        let mut link = Loop {
            name: UART,
            uart: &mut usart6,
        };
        report.check_for(
            UART,
            "UARTDMA-009",
            "a free and a reopen bring transmission back",
            tests::echo(&mut link, b"reopened\n"),
        );

        // The receive ring runs all the time; stuck, `free` must refuse.
        fault::stick(1, 1);
        let Err(refused) = usart6.free() else {
            fault::release(1, 1);
            report.check_for(UART, "API-003", "free refuses while the receive stream will not stop", Err(Fail::Count { expected: 1, got: 0 }));
            tests::finish(&report, || {})
        };
        let still = running(DMA1, 1);
        let error = refused.error();
        let mut usart6 = refused.into_parts();
        let mut byte = [0u8; 1];
        let read = usart6.rx().try_read(&mut byte);
        report.check_for(
            UART,
            "API-003 UARTDMA-009",
            "free refuses while the receive stream will not stop; the UART comes back, reads Halted",
            held(error == StopTimeout && still && read == Err(Error::Halted)),
        );
        fault::release(1, 1);
        let Ok((wiring, buffers)) = usart6.free() else {
            report.check_for(UART, "API-003", "a retried free succeeds once the stream stops", Err(Fail::Count { expected: 1, got: 0 }));
            tests::finish(&report, || {})
        };
        report.check_for(
            UART,
            "API-003",
            "a retried free succeeds once the stream stops; both streams read stopped",
            held(!running(DMA1, 1) && !running(DMA1, 6)),
        );

        // A receive stream an earlier owner left running, at a scratch
        // buffer: stuck, the open refuses it.
        rearm(DMA1, 1, Some(scratch), 16);
        fault::stick(1, 1);
        let refusal = open(&mut run.rcc, wiring, buffers);
        fault::release(1, 1);
        let (wiring, buffers) = match refusal {
            Ok(_) => {
                report.check_for(UART, "UARTDMA-009", "an open refuses a stream that will not stop", Err(Fail::NotRefused(ConfigError::DmaBusy)));
                tests::finish(&report, || {})
            }
            Err(refused) => {
                report.check_for(
                    UART,
                    "UARTDMA-009",
                    "an open refuses a stream that will not stop (DmaBusy)",
                    if refused.error() == ConfigError::DmaBusy {
                        Ok(())
                    } else {
                        Err(Fail::Config(refused.error()))
                    },
                );
                refused.into_parts()
            }
        };
        match open(&mut run.rcc, wiring, buffers) {
            Ok((mut usart6, irq)) => {
                cx.shared.irq6.lock(|handle| *handle = Some(irq));
                let mut link = Loop {
                    name: UART,
                    uart: &mut usart6,
                };
                report.check_for(
                    UART,
                    "UARTDMA-009",
                    "released, the same parts open and echo",
                    tests::echo(&mut link, b"opened\n"),
                );
            }
            Err(refused) => report.check(UART, "released, the same parts open and echo", Err(Fail::Config(refused.error()))),
        }

        // SPI1, DMA1 streams 5 (transmit) and 4 (receive), at 390 kHz: 256
        // bytes take 5 ms, far longer than a stop's bound.
        const SPI: &str = "SPI1 PA5/PA6/PD7";
        match spi::Spi::new(
            &mut run.rcc,
            run.spi1,
            &SpiConfig::new(Hertz::khz(400)),
            SpiBuffers { tx: spi_tx, rx: spi_rx },
        ) {
            Ok(mut spi) => {
                let started = spi.start(&[0x5a; 256]).is_ok();
                fault::stick(1, 4);
                match spi.free() {
                    Ok(_) => {
                        fault::release(1, 4);
                        report.check_for(SPI, "API-003", "free refuses mid-transfer while the receive stream will not stop", Err(Fail::Count { expected: 1, got: 0 }));
                    }
                    Err(refused) => {
                        let still = running(DMA1, 4);
                        report.check_for(
                            SPI,
                            "API-003",
                            "free refuses mid-transfer while the receive stream will not stop",
                            held(started && refused.error() == StopTimeout && still),
                        );
                        fault::release(1, 4);
                        let freed = refused.into_parts().free().is_ok();
                        report.check_for(
                            SPI,
                            "API-003",
                            "a retried free succeeds once the streams stop",
                            held(freed && !running(DMA1, 4) && !running(DMA1, 5)),
                        );
                    }
                }
            }
            Err(refused) => report.check(SPI, "open at 390 kHz", Err(Fail::SpiConfig(refused.error()))),
        }

        // TIM3 CH3 on PB0, DMA2 stream 0: armed, the timer never started.
        const LANE_NAME: &str = "TIM3 CH3 lane";
        let Ok(pwm3) = Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(500)) else {
            defmt::panic!("TIM3 PWM")
        };
        match LaneWiring::<_, 3, _, _>::new(run.pb0, run.lane3_stream).assemble(&pwm3, lane_buf) {
            Ok(mut lane) => {
                let armed = lane.arm(&pwm3, &[1, 2, 3, 4]).is_ok();
                fault::stick(2, 0);
                match lane.free(&pwm3) {
                    Ok(_) => {
                        fault::release(2, 0);
                        report.check_for(LANE_NAME, "API-003", "free refuses while the stream will not stop", Err(Fail::Count { expected: 1, got: 0 }));
                    }
                    Err(refused) => {
                        let still = running(DMA2, 0);
                        report.check_for(
                            LANE_NAME,
                            "API-003",
                            "free refuses while the stream will not stop",
                            held(armed && refused.error() == StopTimeout && still),
                        );
                        fault::release(2, 0);
                        let freed = refused.into_parts().free(&pwm3).is_ok();
                        report.check_for(
                            LANE_NAME,
                            "API-003",
                            "a retried free succeeds once the stream stops",
                            held(freed && !running(DMA2, 0)),
                        );
                    }
                }
            }
            Err(refused) => report.check(LANE_NAME, "assemble", Err(Fail::Timer(refused.error()))),
        }

        // TIM8's port-B sampler, DMA1 stream 0, at 10 kHz: 1000 samples
        // take 100 ms.
        const SAMPLER: &str = "TIM8 sampler";
        match Sampler::new::<'B'>(&mut run.rcc, run.tim8, run.sampler_stream, Hertz::khz(10), samples) {
            Ok(mut sampler) => {
                let started = sampler.start(1000).is_ok();
                fault::stick(1, 0);
                match sampler.free() {
                    Ok(_) => {
                        fault::release(1, 0);
                        report.check_for(SAMPLER, "API-003", "free refuses mid-capture while the stream will not stop", Err(Fail::Count { expected: 1, got: 0 }));
                    }
                    Err(refused) => {
                        let still = running(DMA1, 0);
                        report.check_for(
                            SAMPLER,
                            "API-003",
                            "free refuses mid-capture while the stream will not stop",
                            held(started && refused.error() == StopTimeout && still),
                        );
                        fault::release(1, 0);
                        let freed = refused.into_parts().free().is_ok();
                        report.check_for(
                            SAMPLER,
                            "API-003",
                            "a retried free succeeds once the stream stops",
                            held(freed && !running(DMA1, 0)),
                        );
                    }
                }
            }
            Err(refused) => report.check(SAMPLER, "open at 10 kHz", Err(Fail::Timer(refused.error()))),
        }

        // ADC1's scan of PA1 twice, DMA1 stream 7: one sequence, then the
        // stream re-armed by hand, waiting for requests no sequence makes.
        const SCAN_NAME: &str = "ADC1 scan";
        let adc = match Adc::new(&mut run.rcc, run.adc1, &AdcConfig::slow_source()) {
            Ok(adc) => adc,
            Err(refused) => defmt::panic!("ADC1: {}", refused.error()),
        };
        let pin = adc.channel(&run.analog1);
        match adc.into_scan(run.scan_stream, &[pin, pin], scan_buf) {
            Ok(mut scan) => {
                let mut sequenced = false;
                if scan.start().is_ok() {
                    for _ in 0..board::CLOCKS.cpu.0 / 8 {
                        if let Some(done) = scan.poll() {
                            sequenced = done.is_ok();
                            break;
                        }
                    }
                }
                rearm(DMA1, 7, None, 2);
                fault::stick(1, 7);
                match scan.free() {
                    Ok(_) => {
                        fault::release(1, 7);
                        report.check_for(SCAN_NAME, "API-003", "free refuses while the stream will not stop", Err(Fail::Count { expected: 1, got: 0 }));
                    }
                    Err(refused) => {
                        let still = running(DMA1, 7);
                        report.check_for(
                            SCAN_NAME,
                            "API-003",
                            "free refuses while the stream will not stop",
                            held(sequenced && refused.error() == StopTimeout && still),
                        );
                        fault::release(1, 7);
                        let freed = refused.into_parts().free().is_ok();
                        report.check_for(
                            SCAN_NAME,
                            "API-003",
                            "a retried free succeeds once the stream stops",
                            held(freed && !running(DMA1, 7)),
                        );
                    }
                }
            }
            Err(refused) => report.check(SCAN_NAME, "the scan", Err(Fail::Adc(refused.error()))),
        }

        tests::finish(&report, || {});
    }
}
