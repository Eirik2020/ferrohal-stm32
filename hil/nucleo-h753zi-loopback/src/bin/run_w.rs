//! Run W: faults injected on run A's USART6 loopback, one at a time, each
//! reported and each survived. This board's links are all loopbacks, so
//! the pair work runs on the F401; here the faults a UART can do to
//! itself.
//!
//! - Overrun: the DMA request is disconnected for two frames, so the
//!   second completes while the first still holds RDR and the USART's own
//!   overrun flag fires. v4 clears through ICR, which costs no byte, so
//!   A survives; whether B is discarded or lands once the request returns
//!   is the shift register's timing, and the check accepts both.
//! - DMA transfer error: the receive stream is stopped, re-pointed at
//!   DTCM — which DMA1 and DMA2 cannot reach, the same fact
//!   `ConfigError::BufferUnreachable` refuses at construction — and
//!   restarted. The next byte is a bus error; the stream stops, the
//!   driver reports DmaTransfer and restarts the ring on its own buffer.
//!   What the outage costs is timing, so the bytes right after the gap go
//!   to the log unnamed and an echo after it carries the check.
//! - Timeout: the DMA transmit request is disconnected before a frame is
//!   queued, so nothing drains. The queue's bound expires, the driver
//!   stops the stream, drops the queue and reports Timeout; the next
//!   frame goes out whole.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART6`, `DMA1_STREAM0`, `DMA1_STREAM6` | USART6's `UartIrq` | 2 |
//!
//! Injections reach the registers through `stm32_metapac`, as run R's
//! timer work does: the bench may sabotage what the library may not.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

use ferrohal_stm32::uart::Error;
use stm32_metapac::dma::St;

use tests::{Fail, Link, Loop, Report};

/// Where the receive stream must not write: DTCM, which DMA1 and DMA2
/// cannot reach on this chip.
const UNREACHABLE: u32 = 0x2000_0000;

/// Stops a DMA stream and waits for the enable to land, so a following
/// register write cannot race it.
fn stop_stream(st: St) {
    st.cr().modify(|w| w.set_en(false));
    while st.cr().read().en() {}
}

/// One fault's worth of reading: the bytes, the overrun and DMA errors,
/// and anything else. Stops at a silent line.
fn collect<U: ferrohal_stm32::uart::Instance>(
    loop_: &mut Loop<'_, U>,
    cap: usize,
) -> (usize, [u8; 64], u32, u32, Option<Error>) {
    let (mut len, mut overrun, mut dma, mut other) = (0, 0, 0, None);
    let mut got = [0; 64];
    let patience = loop_.patience();
    let mut buf = [0; 16];
    for _ in 0..64 {
        match loop_.rx().read_within(&mut buf, patience) {
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
            Err(Error::DmaTransfer) => dma += 1,
            Err(error) => other = Some(error),
        }
        if len >= cap {
            break;
        }
    }
    (len, got, overrun, dma, other)
}

/// Overrun: two frames with the DMA request disconnected.
fn inject_overrun<U: ferrohal_stm32::uart::Instance>(report: &mut Report, loop_: &mut Loop<'_, U>) {
    let name = loop_.name();
    tests::drain(loop_);
    let before = loop_.rx().health().overrun;
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmar(false));
    let first = loop_
        .tx()
        .write_all(b"AB")
        .and_then(|()| loop_.tx().flush());
    // The flag fires within an interrupt latency of the second frame's
    // completion; wait until the driver has counted it before the request
    // returns, so the restore cannot race the flag and swallow the fault.
    for _ in 0..100 {
        if loop_.rx().health().overrun != before {
            break;
        }
        tests::delay_ms(1);
    }
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmar(true));
    let second = loop_
        .tx()
        .write_all(b"CDE")
        .and_then(|()| loop_.tx().flush());
    let (len, got, overrun, dma, other) = collect(loop_, 8);
    let counted = loop_.rx().health().overrun.wrapping_sub(before);
    defmt::info!(
        "{=str}: overrun: reader {=u32}, driver counted {=u32}, {=usize} byte(s) after it, dma {=u32}, sends ok {} and {}",
        name,
        overrun,
        counted,
        len,
        dma,
        first.is_ok(),
        second.is_ok(),
    );
    report.check_for(
        name,
        "UART-015 UART-019",
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
        } else if dma != 0 {
            Err(Fail::Uart(Error::DmaTransfer))
        } else {
            Ok(())
        },
    );
    // v4 clears through ICR, which touches no byte, so A survives. Whether
    // B is discarded or lands once the request returns is the shift
    // register's timing, not policy, so both are accepted; the claim is
    // that A, C, D and E arrive in order and at most B is lost.
    report.check_for(
        name,
        "UART-015",
        "overrun: A, C, D and E arrive in order, at most B lost",
        match len {
            4 => got[0] == b'A' && got[1] == b'C' && got[2] == b'D' && got[3] == b'E',
            5 => {
                got[0] == b'A'
                    && got[1] == b'B'
                    && got[2] == b'C'
                    && got[3] == b'D'
                    && got[4] == b'E'
            }
            _ => false,
        }
        .then_some(())
        .ok_or(Fail::Count {
            expected: 4,
            got: len as u32,
        }),
    );
    report.check(
        name,
        "echo after the overrun",
        tests::echo(loop_, b"after the overrun\\n"),
    );
}

/// DMA transfer error: the receive stream re-pointed at DTCM.
fn inject_dma_transfer<U: ferrohal_stm32::uart::Instance>(
    report: &mut Report,
    loop_: &mut Loop<'_, U>,
) {
    let name = loop_.name();
    tests::drain(loop_);
    let health = loop_.rx().health();
    let (transfers, restarts) = (health.dma_transfer, health.restarts);
    let rx = stm32_metapac::DMA1.st(0);
    stop_stream(rx);
    rx.m0ar().write(|w| *w = UNREACHABLE);
    rx.cr().modify(|w| w.set_en(true));
    let sent = loop_.tx().write_all(b"X").and_then(|()| loop_.tx().flush());
    let (len, got, overrun, dma, other) = collect(loop_, 8);
    let after = loop_.rx().health();
    defmt::info!(
        "{=str}: dma transfer: reader {=u32}, driver counted {=u32}, restarts {=u32} -> {=u32}, {=usize} byte(s) after it, overrun {=u32}, sent {}, bytes {=[u8]:02x}",
        name,
        dma,
        after.dma_transfer.wrapping_sub(transfers),
        restarts,
        after.restarts,
        len,
        overrun,
        sent.is_ok(),
        &got[..len],
    );
    report.check_for(
        name,
        "UARTDMA-011",
        "dma transfer: reported once and the ring restarted",
        if let Some(error) = other {
            Err(Fail::Uart(error))
        } else if let Err(error) = sent {
            Err(Fail::Uart(error))
        } else if dma != 1 || after.dma_transfer.wrapping_sub(transfers) != 1 {
            Err(Fail::Count {
                expected: 1,
                got: if dma != 1 { dma } else { 0 },
            })
        } else if after.restarts <= restarts {
            Err(Fail::NotReported(Error::DmaTransfer))
        } else if overrun != 0 {
            Err(Fail::Uart(Error::Overrun))
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
        tests::echo(loop_, b"after the dma error\\n"),
    );
}

/// Timeout: the DMA transmit request disconnected before a frame is queued.
fn inject_timeout<U: ferrohal_stm32::uart::Instance>(report: &mut Report, loop_: &mut Loop<'_, U>) {
    let name = loop_.name();
    tests::drain(loop_);
    let before = loop_.tx().health().timeouts;
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmat(false));
    let frame = [0x55; 128];
    let queued = loop_.tx().write_all(&frame);
    let flushed = queued.and_then(|()| loop_.tx().flush());
    stm32_metapac::USART6.cr3().modify(|w| w.set_dmat(true));
    let after = loop_.tx().health();
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
        tests::echo(loop_, b"after the timeout\\n"),
    );
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{
        chip::USART6,
        uart::{Buffers, Config, Uart, UartInterrupts, UartIrq},
    };

    use crate::{
        board,
        tests::{self, Loop, Report},
    };

    #[shared]
    struct Shared {
        #[lock_free]
        irq6: UartIrq,
    }

    #[local]
    struct Local {
        usart6: Uart<USART6>,
        vectors6: UartInterrupts,
        led: board::Led,
    }

    #[init(local = [
        tx6: [u8; board::TX_BUFFER] = [0; board::TX_BUFFER],
        rx6: [u8; board::RX_RING] = [0; board::RX_RING],
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

        let vectors6 = irq6.interrupts();

        checks::spawn().ok();
        (
            Shared { irq6 },
            Local {
                usart6,
                vectors6,
                led: run.led,
            },
        )
    }

    /// The driver's own three steps, in their order.
    fn serve(irq: &mut UartIrq) {
        irq.on_rx_dma();
        irq.on_uart();
        irq.on_tx_dma();
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        serve(cx.shared.irq6);
    }

    #[task(binds = DMA1_STREAM0, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        serve(cx.shared.irq6);
    }

    #[task(binds = DMA1_STREAM6, priority = 2, shared = [irq6])]
    fn usart6_tx(cx: usart6_tx::Context) {
        serve(cx.shared.irq6);
    }

    #[task(priority = 1, local = [usart6, vectors6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART6",
            "UART-003 RESOURCE-001",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors6),
        );
        let mut loop_ = Loop {
            name: "USART6 PC6->PC7 loopback",
            uart: cx.local.usart6,
        };

        crate::inject_overrun(&mut report, &mut loop_);
        crate::inject_dma_transfer(&mut report, &mut loop_);
        crate::inject_timeout(&mut report, &mut loop_);

        tests::finish(&report, || {});
    }
}
