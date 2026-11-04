//! Run B: USART1 and USART6 wired to each other, each on its other pins and
//! other receive stream.
//!
//! - USART1 TX (PB6) over J3 to USART6 RX (PC7), and USART6 TX (PC6) over
//!   J4 to USART1 RX (PB7): echo, stream and break, each way.
//! - Both directions streaming at once.
//! - Deliberate mismatches, each way: half the baud rate must give a
//!   framing error, even parity into odd a parity error. The receiving
//!   USART is not transmitting, so ES0299 2.10.2 cannot hide the parity
//!   error.
//! - The parity error's clear, each way. RM0368 says to wait for RXNE
//!   before clearing PE; the driver clears it at once, an SR read then a
//!   DR read, from the USART vector. 8N2 into 8O1 makes the sender's first
//!   stop bit the receiver's parity bit, so a byte with an odd count of
//!   ones is a parity error and one with an even count is good, with no
//!   change of frame between them: one bad byte, then a good frame straight
//!   after it. The line vector reads SR first, through stm32-metapac, and
//!   notes RXNE whenever PE is set; the good frame must arrive whole and in
//!   order; PE must be seen and reported once.
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
//! | `USART1`, `DMA2_STREAM5`, `DMA2_STREAM7` | USART1's `UartIrq` | 2 |
//! | `USART6`, `DMA2_STREAM2`, `DMA2_STREAM6` | USART6's `UartIrq` | 2 |
//! | `SPI1` | RTIC, dispatching the priority 1 `checks` task | 1 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embedded_io::Write;
use ferrohal_stm32::uart::{Config, Error, Parity, StopBits};
use stm32_metapac::usart::Usart;

use tests::{Configurable, Cross, Fail, Link, Report};

/// On while the PE clear check runs: only then do the line vectors read SR
/// ahead of the driver.
static WATCH: AtomicBool = AtomicBool::new(false);
/// While watching, how often a line vector found PE set, and in how many of
/// those RXNE was set too.
static PE_SEEN: AtomicU32 = AtomicU32::new(0);
static PE_WITH_RXNE: AtomicU32 = AtomicU32::new(0);

/// The byte the receiver refuses: an odd count of ones.
const BAD: u8 = 0x01;
/// The frame that follows it: every byte an even count of ones.
const GOOD: [u8; 16] = [
    0x00, 0x03, 0x05, 0x06, 0x09, 0x0a, 0x0c, 0x0f, 0x11, 0x12, 0x14, 0x17, 0x18, 0x1b, 0x1d, 0x1e,
];
const _: () = {
    assert!(BAD.count_ones() % 2 == 1);
    let mut i = 0;
    while i < GOOD.len() {
        assert!(GOOD[i].count_ones() % 2 == 0);
        i += 1;
    }
};

/// Called by a line vector before the driver's `on_interrupt`: notes PE,
/// and RXNE with it, as SR holds them before the driver's SR and DR reads
/// clear PE.
///
/// This extra read cannot change the clear being measured. An SR read
/// clears nothing by itself; a flag clears only when a DR access follows
/// an SR read made while it was set. The driver makes its own SR read
/// straight after this one and its DR read straight after that, so the
/// same SR-then-DR sequence clears PE either way. The one access that
/// could slip between this read and the driver's is the receive DMA
/// taking a new byte from DR; the next byte is a whole frame away (about
/// 95 us at 115200), while the two reads are a few cycles apart. And this
/// runs only while `WATCH` is on, so no other check sees it.
fn watch(regs: Usart) {
    if !WATCH.load(Ordering::Relaxed) {
        return;
    }
    let sr = regs.sr().read();
    if sr.pe() {
        PE_SEEN.fetch_add(1, Ordering::Relaxed);
        if sr.rxne() {
            PE_WITH_RXNE.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One bad byte, then `GOOD` straight after it, 8N2 into 8O1; then both
/// ends back to 8N1. Three checks: the line vector saw PE (and what RXNE
/// was then), the good frame arrived whole and in order, and PE was seen
/// and reported once.
fn pe_clear(report: &mut Report, cross: &mut Cross) {
    let name = cross.name;
    let frames = cross
        .from
        .set_config(&Config::new(115_200).with_stop_bits(StopBits::Two))
        .and_then(|()| {
            cross
                .to
                .set_config(&Config::new(115_200).with_parity(Parity::Odd))
        });
    if let Err(error) = frames {
        report.check_for(
            name,
            "UART-014",
            "PE clear: 8N2 into 8O1",
            Err(error.into()),
        );
        return;
    }
    tests::drain(cross);
    let counted_before = cross.to.rx().health().parity;
    PE_SEEN.store(0, Ordering::Relaxed);
    PE_WITH_RXNE.store(0, Ordering::Relaxed);
    WATCH.store(true, Ordering::Relaxed);

    let mut frame = [0; 1 + GOOD.len()];
    frame[0] = BAD;
    frame[1..].copy_from_slice(&GOOD);
    let sent = cross
        .tx()
        .write_all(&frame)
        .and_then(|()| cross.tx().flush());

    // Everything the reader gets until the line is silent: the bytes, the
    // parity errors, where the first one fell, and any other error.
    let mut got = [0; 2 * GOOD.len()];
    let (mut len, mut parity, mut parity_at, mut other) = (0, 0, None, None);
    let patience = cross.patience();
    let mut buf = [0; 8];
    // Bounded: far more passes than the frame's bytes and errors can take.
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
            Err(Error::Parity) => {
                parity += 1;
                parity_at.get_or_insert(len);
            }
            Err(error) => other = Some(error),
        }
    }
    WATCH.store(false, Ordering::Relaxed);
    let seen = PE_SEEN.load(Ordering::Relaxed);
    let with_rxne = PE_WITH_RXNE.load(Ordering::Relaxed);
    let counted = cross.to.rx().health().parity.wrapping_sub(counted_before);
    defmt::info!(
        "{=str}: PE seen by the line vector {=u32} time(s), RXNE set in {=u32}; reader: {=usize} bytes, {=u32} parity error(s), first after byte {}; driver counted {=u32}",
        name,
        seen,
        with_rxne,
        len,
        parity,
        parity_at,
        counted
    );

    report.check_for(
        name,
        "UART-014",
        "PE clear: line vector saw PE, RXNE state above",
        match sent {
            Err(error) => Err(error.into()),
            Ok(()) if seen == 0 => Err(Fail::NotReported(Error::Parity)),
            Ok(()) => Ok(()),
        },
    );

    // The bad byte itself may be there or not: DMA takes it, unless the
    // clear's DR read got to it first.
    let received = &got[..len.min(got.len())];
    let good = match received {
        [BAD, rest @ ..] => rest,
        rest => rest,
    };
    report.check_for(
        name,
        "UART-014",
        "PE clear: next good frame after a parity error intact and in order",
        if let Some(at) = GOOD.iter().zip(good).position(|(sent, got)| sent != got) {
            Err(Fail::Mismatch {
                at,
                sent: GOOD[at],
                got: good[at],
            })
        } else if good.len() < GOOD.len() {
            Err(Fail::Silent {
                missing: GOOD.len() - good.len(),
            })
        } else if len > GOOD.len() + 1 {
            Err(Fail::Count {
                expected: GOOD.len() as u32 + 1,
                got: len as u32,
            })
        } else {
            Ok(())
        },
    );

    let restored = cross
        .set_config(&Config::new(115_200))
        .map_err(Fail::from)
        .and_then(|()| {
            tests::drain(cross);
            tests::echo(cross, b"restored")
        });
    report.check_for(
        name,
        "UART-014",
        "PE clear: PE seen and reported once",
        if let Some(error) = other {
            Err(Fail::Uart(error))
        } else if let Some(got) = [parity, counted, seen].into_iter().find(|&n| n != 1) {
            Err(Fail::Count { expected: 1, got })
        } else {
            restored
        },
    );
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::uart::{Buffers, Config, Uart, UartInterrupts, UartIrq};

    use crate::{
        board,
        tests::{self, Cross, Pair, Report},
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
        usart1: Uart,
        usart6: Uart,
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
        // The cycle counter, for the wait-bound check.
        cx.core.DCB.enable_trace();
        cx.core.DWT.enable_cycle_counter();
        let mut run = match board::run_b(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run B, clocks: {}", board::CLOCKS);

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

    #[task(binds = USART1, priority = 2, shared = [irq1])]
    fn usart1_line(cx: usart1_line::Context) {
        crate::watch(stm32_metapac::USART1);
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM5, priority = 2, shared = [irq1])]
    fn usart1_rx(cx: usart1_rx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = DMA2_STREAM7, priority = 2, shared = [irq1])]
    fn usart1_tx(cx: usart1_tx::Context) {
        cx.shared.irq1.on_interrupt();
    }

    #[task(binds = USART6, priority = 2, shared = [irq6])]
    fn usart6_line(cx: usart6_line::Context) {
        crate::watch(stm32_metapac::USART6);
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM2, priority = 2, shared = [irq6])]
    fn usart6_rx(cx: usart6_rx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(binds = DMA2_STREAM6, priority = 2, shared = [irq6])]
    fn usart6_tx(cx: usart6_tx::Context) {
        cx.shared.irq6.on_interrupt();
    }

    #[task(priority = 1, local = [usart1, usart6, vectors1, vectors6, led])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        report.check_for(
            "USART1",
            "UART-003",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors1),
        );
        report.check_for(
            "USART6",
            "UART-003",
            "vectors enabled at one priority",
            tests::vectors_verified(cx.local.vectors6),
        );
        let (usart1, usart6) = (cx.local.usart1, cx.local.usart6);

        for forward in [true, false] {
            let mut cross = if forward {
                Cross {
                    name: "USART1 PB6->USART6 PC7",
                    from: &mut *usart1,
                    to: &mut *usart6,
                }
            } else {
                Cross {
                    name: "USART6 PC6->USART1 PB7",
                    from: &mut *usart6,
                    to: &mut *usart1,
                }
            };
            let name = cross.name;
            tests::drain(&mut cross);
            report.check_for(
                name,
                "UART-001 UART-002 USART-001",
                "echo",
                tests::echo(&mut cross, b"hello across\n"),
            );
            report.check_for(
                name,
                "UART-002",
                "stream 8 KiB",
                tests::stream(&mut cross, 8 * 1024),
            );
            report.check_for(
                name,
                "UART-005 UART-013 USART-002 USART-003",
                "break reported",
                tests::line_break(&mut cross),
            );
            report.check_for(
                name,
                "UART-005 USART-002",
                "half baud rate reported as framing error",
                tests::framing_mismatch(&mut cross),
            );
            report.check_for(
                name,
                "UART-014",
                "even into odd reported as parity error",
                tests::parity_mismatch(&mut cross),
            );
            crate::pe_clear(&mut report, &mut cross);
        }

        let (tx1, rx1) = usart1.halves();
        let (tx6, rx6) = usart6.halves();
        report.check_for(
            "USART1 <-> USART6",
            "UART-002",
            "8 KiB each way, both at once",
            tests::streams(
                &mut [
                    &mut Pair {
                        name: "USART1->USART6",
                        tx: tx1,
                        rx: rx6,
                    },
                    &mut Pair {
                        name: "USART6->USART1",
                        tx: tx6,
                        rx: rx1,
                    },
                ],
                8 * 1024,
            ),
        );

        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }
}
