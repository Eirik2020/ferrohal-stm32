//! Run W: faults injected on run B's cross-wired pair, one at a time, each
//! reported and each survived.
//!
//! - Overrun: USART6's DMA request is disconnected for two frames, so the
//!   second completes while the first still holds DR and the USART's own
//!   overrun flag fires. The driver reports it, the clear may or may not
//!   cost the first byte (RM0368's SR-then-DR clear), and the frames after
//!   it arrive whole.
//! - Noise: USART1 sends 8N2 at 120500, 4.6 % fast for the receiver's
//!   115200. Late in each frame a sample straddles the next bit's edge, so
//!   the majority vote still reads the bit but not all three samples
//!   agree: the noise flag fires, every byte intact, no framing error.
//! - DMA transfer error: USART6's receive stream is stopped, re-pointed at
//!   unmapped memory and restarted. The next byte is a bus error; the
//!   stream stops, the driver reports DmaTransfer and restarts the ring on
//!   its own buffer, and an echo after the gap succeeds. What the outage
//!   costs is timing — a frame completed into a dead stream raises
//!   overrun, whose mark the restart wipes — so the bytes right after the
//!   gap go to the log unnamed.
//! - Timeout: USART1's DMA transmit request is disconnected before a frame
//!   is queued, so nothing drains. The queue's bound expires, the driver
//!   stops the stream, drops the queue and reports Timeout; the next
//!   frame goes out whole.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM5`, `DMA2_STREAM7` | USART1's `UartIrq` | 2 |
//! | `USART6`, `DMA2_STREAM2`, `DMA2_STREAM6` | USART6's `UartIrq` | 2 |
//!
//! Injections reach the registers through `stm32_metapac`, as run B's
//! parity watch does: the bench may sabotage what the library may not.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

use ferrohal_stm32::uart::{Config, Error, StopBits};
use stm32_metapac::dma::St;

use tests::{Configurable, Cross, Fail, Link, Report};

/// The sender running 4.6 % fast: inside the window where the last of the
/// three samples of the last data bit straddles the stop, so the majority
/// still reads the bit but not all three agree. Below 4.35 % no sample
/// straddles; above 5.1 % the middle one crosses too and the byte
/// corrupts.
const FAST: u32 = 120_500;
/// The receiver's rate.
const SLOW: u32 = 115_200;
/// Bytes whose last data bit is 0 against a stop of 1, so the straddled
/// sample disagrees: 0x55's bit 7 is 0.
const NOISY: [u8; 8] = [0x55; 8];

/// Where the receiver's stream must not write: past the 96 KiB of SRAM,
/// unmapped, so the first access is a bus error.
const UNMAPPED: u32 = 0x2020_0000;

/// Stops a DMA stream and waits for the enable to land, so a following
/// register write cannot race it.
fn stop_stream(st: St) {
    st.cr().modify(|w| w.set_en(false));
    while st.cr().read().en() {}
}

/// One fault's worth of reading: the bytes, the overrun, noise and DMA
/// errors, and anything else. Stops at a silent line.
fn collect<U: ferrohal_stm32::uart::Instance, V: ferrohal_stm32::uart::Instance>(
    cross: &mut Cross<'_, U, V>,
    cap: usize,
) -> (usize, [u8; 64], u32, u32, u32, Option<Error>) {
    let (mut len, mut overrun, mut noise, mut dma, mut other) = (0, 0, 0, 0, None);
    let mut got = [0; 64];
    let patience = cross.patience();
    let mut buf = [0; 16];
    for _ in 0..64 {
        match cross.rx().read_within(&mut buf, patience) {
            Ok(n) => {
                for &byte in &buf[..n] {
                    if let Some(slot) = got.get_mut(len) {
                        *slot = byte;
                    }
                    len += 1;
                }
            }
            Err(Error::Timeout) => break,
            Err(Error::Overrun) => overrun += 1,
            Err(Error::Noise) => noise += 1,
            Err(Error::DmaTransfer) => dma += 1,
            Err(error) => other = Some(error),
        }
        if len >= cap {
            break;
        }
    }
    (len, got, overrun, noise, dma, other)
}

/// The frames after a fault must be exactly `want`, optionally preceded by
/// `before`: the overrun's clear may cost its byte, a noise fault costs
/// none, a DMA transfer error costs the byte that tripped it.
fn expect_tail(len: usize, got: &[u8; 64], before: Option<u8>, want: &[u8]) -> Result<(), Fail> {
    if len != want.len() && !(before.is_some() && len == want.len() + 1) {
        return Err(Fail::Count {
            expected: want.len() as u32,
            got: len as u32,
        });
    }
    let tail = &got[len - want.len()..len];
    if tail != want {
        let at = tail.iter().zip(want).position(|(a, b)| a != b).unwrap_or(0);
        return Err(Fail::Mismatch {
            at,
            sent: want[at],
            got: tail[at],
        });
    }
    if let Some(first) = before {
        if len == want.len() + 1 && got[0] != first {
            return Err(Fail::Mismatch {
                at: 0,
                sent: first,
                got: got[0],
            });
        }
    }
    Ok(())
}

/// Overrun: two frames with the receiver's DMA request disconnected.
fn inject_overrun<U: ferrohal_stm32::uart::Instance, V: ferrohal_stm32::uart::Instance>(
    report: &mut Report,
    cross: &mut Cross<'_, U, V>,
) {
    let name = cross.name;
    tests::drain(cross);
    let before = cross.to.rx().health().overrun;
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmar(false));
    let first = cross
        .tx()
        .write_all(b"AB")
        .and_then(|()| cross.tx().flush());
    // The flag fires within an interrupt latency of the second frame's
    // completion; wait until the driver has counted it before the request
    // returns, so the restore cannot race the flag and swallow the fault.
    for _ in 0..100 {
        if cross.to.rx().health().overrun != before {
            break;
        }
        tests::delay_ms(1);
    }
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmar(true));
    let second = cross
        .tx()
        .write_all(b"CDE")
        .and_then(|()| cross.tx().flush());
    let (len, got, overrun, noise, dma, other) = collect(cross, 8);
    let counted = cross.to.rx().health().overrun.wrapping_sub(before);
    defmt::info!(
        "{=str}: overrun: reader {=u32}, driver counted {=u32}, {=usize} byte(s) after it, noise {=u32}, dma {=u32}, sends ok {} and {}",
        name,
        overrun,
        counted,
        len,
        noise,
        dma,
        first.is_ok(),
        second.is_ok(),
    );
    report.check_for(
        name,
        "UART-015 USART-004",
        "overrun: reported once, by the reader and in Health",
        if let Some(error) = other {
            Err(Fail::Uart(error))
        } else if let Err(error) = first.or(second) {
            Err(Fail::Uart(error))
        } else if overrun != 1 || counted != 1 {
            Err(Fail::Count {
                expected: 1,
                got: if overrun != 1 { overrun } else { counted },
            })
        } else if noise != 0 || dma != 0 {
            Err(Fail::Uart(if noise != 0 {
                Error::Noise
            } else {
                Error::DmaTransfer
            }))
        } else {
            Ok(())
        },
    );
    report.check_for(
        name,
        "UART-015",
        "overrun: reception carries on, at most the first byte lost",
        expect_tail(len, &got, Some(b'A'), b"CDE"),
    );
    report.check(
        name,
        "echo after the overrun",
        tests::echo(cross, b"after the overrun\n"),
    );
}

/// Noise: the sender 5.5 % fast, 8N2 into 8N1.
fn inject_noise<U: ferrohal_stm32::uart::Instance, V: ferrohal_stm32::uart::Instance>(
    report: &mut Report,
    cross: &mut Cross<'_, U, V>,
) {
    let name = cross.name;
    let set = cross
        .from
        .set_config(&Config::new(FAST).with_stop_bits(StopBits::Two))
        .and_then(|()| cross.to.set_config(&Config::new(SLOW)));
    if let Err(error) = set {
        report.check_for(name, "UART-015", "noise: 8N2 at 121500", Err(error.into()));
        return;
    }
    tests::drain(cross);
    let before = cross.to.rx().health().noise;
    let sent = cross
        .tx()
        .write_all(&NOISY)
        .and_then(|()| cross.tx().flush());
    let (len, got, overrun, noise, dma, other) = collect(cross, 32);
    let counted = cross.to.rx().health().noise.wrapping_sub(before);
    defmt::info!(
        "{=str}: noise: reader {=u32}, driver counted {=u32}, {=usize} byte(s), overrun {=u32}, dma {=u32}, sent {}",
        name,
        noise,
        counted,
        len,
        overrun,
        dma,
        sent.is_ok(),
    );
    report.check_for(
        name,
        "UART-015 USART-004",
        "noise: reported, and every byte intact",
        if let Some(error) = other {
            Err(Fail::Uart(error))
        } else if let Err(error) = sent {
            Err(Fail::Uart(error))
        } else if noise == 0 || counted == 0 {
            Err(Fail::NotReported(Error::Noise))
        } else if overrun != 0 || dma != 0 {
            Err(Fail::Uart(if overrun != 0 {
                Error::Overrun
            } else {
                Error::DmaTransfer
            }))
        } else {
            expect_tail(len, &got, None, &NOISY)
        },
    );
    report.check(
        name,
        "back to 115200 8N1 after the noise check",
        cross
            .set_config(&Config::new(SLOW))
            .map_err(Fail::from)
            .and_then(|()| {
                tests::drain(cross);
                tests::echo(cross, b"restored")
            }),
    );
}

/// DMA transfer error: the receive stream re-pointed at unmapped memory.
fn inject_dma_transfer<U: ferrohal_stm32::uart::Instance, V: ferrohal_stm32::uart::Instance>(
    report: &mut Report,
    cross: &mut Cross<'_, U, V>,
) {
    let name = cross.name;
    tests::drain(cross);
    let health = cross.to.rx().health();
    let (transfers, restarts) = (health.dma_transfer, health.restarts);
    let rx = stm32_metapac::DMA2.st(2);
    stop_stream(rx);
    rx.m0ar().write(|w| *w = UNMAPPED);
    rx.cr().modify(|w| w.set_en(true));
    let first = cross.tx().write_all(b"X").and_then(|()| cross.tx().flush());
    let second = cross
        .tx()
        .write_all(b"YZ")
        .and_then(|()| cross.tx().flush());
    let (len, got, overrun, noise, dma, other) = collect(cross, 8);
    let after = cross.to.rx().health();
    defmt::info!(
        "{=str}: dma transfer: reader {=u32}, driver counted {=u32}, restarts {=u32} -> {=u32}, {=usize} byte(s) after it, overrun {=u32}, noise {=u32}, sends ok {} and {}, bytes {=[u8]:02x}",
        name,
        dma,
        after.dma_transfer.wrapping_sub(transfers),
        restarts,
        after.restarts,
        len,
        overrun,
        noise,
        first.is_ok(),
        second.is_ok(),
        &got[..len],
    );
    report.check_for(
        name,
        "UARTDMA-011",
        "dma transfer: reported once and the ring restarted",
        if let Some(error) = other {
            Err(Fail::Uart(error))
        } else if let Err(error) = first.or(second) {
            Err(Fail::Uart(error))
        } else if dma != 1 || after.dma_transfer.wrapping_sub(transfers) != 1 {
            Err(Fail::Count {
                expected: 1,
                got: if dma != 1 { dma } else { 0 },
            })
        } else if after.restarts <= restarts {
            Err(Fail::NotReported(Error::DmaTransfer))
        } else if overrun != 0 || noise != 0 {
            Err(Fail::Uart(if overrun != 0 {
                Error::Overrun
            } else {
                Error::Noise
            }))
        } else {
            Ok(())
        },
    );
    // What the outage costs is timing: a frame completed into a dead
    // stream raises overrun, and the restart wipes its mark (marks carry
    // the ring's run), so the bytes right after the gap are not named
    // here — the log has them. That reception carries on is the claim.
    report.check(
        name,
        "echo after the dma transfer error",
        tests::echo(cross, b"after the dma error\n"),
    );
}

/// Timeout: the sender's DMA request disconnected before a frame is queued.
fn inject_timeout<U: ferrohal_stm32::uart::Instance, V: ferrohal_stm32::uart::Instance>(
    report: &mut Report,
    cross: &mut Cross<'_, U, V>,
) {
    let name = cross.name;
    tests::drain(cross);
    let before = cross.from.tx().health().timeouts;
    stm32_metapac::USART1.cr3().modify(|w| w.set_dmat(false));
    let frame = [0x55; 128];
    let queued = cross.tx().write_all(&frame);
    let flushed = queued.and_then(|()| cross.tx().flush());
    stm32_metapac::USART1.cr3().modify(|w| w.set_dmat(true));
    let after = cross.from.tx().health();
    defmt::info!(
        "{=str}: timeout: queue {}, flush {}, timeouts {=u32} -> {=u32}",
        name,
        queued.is_ok(),
        if flushed.is_ok() { "ok" } else { "err" },
        before,
        after.timeouts,
    );
    report.check_for(
        name,
        "UARTDMA-012",
        "timeout: the bound expired, the queue dropped and counted",
        match (&flushed, after.timeouts.wrapping_sub(before)) {
            (Err(Error::Timeout), 1) => Ok(()),
            (Err(error), _) => Err(Fail::Uart(*error)),
            (Ok(()), _) => Err(Fail::NotReported(Error::Timeout)),
        },
    );
    report.check(
        name,
        "the next frame goes out whole",
        tests::echo(cross, b"after the timeout\n"),
    );
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{
        chip::{USART1, USART6},
        uart::{Buffers, Config, Uart, UartInterrupts, UartIrq},
    };

    use crate::{
        board,
        tests::{self, Cross, Report},
    };

    const RX_RING: usize = 256;
    const TX_BUFFER: usize = 256;

    #[shared]
    struct Shared {
        #[lock_free]
        irq1: UartIrq,
        #[lock_free]
        irq6: UartIrq,
    }

    #[local]
    struct Local {
        usart1: Uart<USART1>,
        usart6: Uart<USART6>,
        vectors1: UartInterrupts,
        vectors6: UartInterrupts,
        led: board::Led,
    }

    #[init(local = [
        tx1: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx1: [u8; RX_RING] = [0; RX_RING],
        tx6: [u8; TX_BUFFER] = [0; TX_BUFFER],
        rx6: [u8; RX_RING] = [0; RX_RING],
    ])]
    fn init(mut cx: init::Context) -> (Shared, Local) {
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_w(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run W, clocks: {}", board::CLOCKS);

        let config = Config::new(115_200);
        let local = cx.local;
        let (usart1, irq1) = Uart::new(
            &mut run.rcc,
            run.usart1,
            &config,
            Buffers {
                tx: local.tx1,
                rx: local.rx1,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART1: {}", error));
        let (usart6, irq6) = Uart::new(
            &mut run.rcc,
            run.usart6,
            &config,
            Buffers {
                tx: local.tx6,
                rx: local.rx6,
            },
        )
        .unwrap_or_else(|error| defmt::panic!("USART6: {}", error));

        let vectors1 = irq1.interrupts();
        let vectors6 = irq6.interrupts();

        checks::spawn().ok();
        (
            Shared { irq1, irq6 },
            Local {
                usart1,
                usart6,
                vectors1,
                vectors6,
                led: run.led,
            },
        )
    }

    /// The driver's own three steps, in its order.
    fn serve(irq: &mut UartIrq) {
        irq.on_rx_dma();
        irq.on_uart();
        irq.on_tx_dma();
    }

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_line(cx: usart1_line::Context) {
        serve(cx.shared.irq1);
    }

    #[task(binds = DMA2_STREAM5, priority = 2, shared = [irq1])]
    fn usart1_rx(cx: usart1_rx::Context) {
        serve(cx.shared.irq1);
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq1])]
    fn usart1_tx(cx: usart1_tx::Context) {
        serve(cx.shared.irq1);
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        serve(cx.shared.irq6);
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        serve(cx.shared.irq6);
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [irq6])]
    fn usart6_tx(cx: usart6_tx::Context) {
        serve(cx.shared.irq6);
    }

    #[task(priority = 1, local = [usart1, usart6, vectors1, vectors6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART1",
            "UART-003 RES-001",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors1),
        );
        report.check_for(
            "USART6",
            "UART-003 RES-001",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors6),
        );
        let mut cross = Cross {
            name: "USART1 PB6->USART6 PC7",
            from: cx.local.usart1,
            to: cx.local.usart6,
        };

        crate::inject_overrun(&mut report, &mut cross);
        crate::inject_dma_transfer(&mut report, &mut cross);
        crate::inject_timeout(&mut report, &mut cross);
        crate::inject_noise(&mut report, &mut cross);

        tests::finish(&report, || {});
    }
}
