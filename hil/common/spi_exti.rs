//! The SPI and EXTI checks, shared by the bench firmwares. Every SPI link is
//! a jumper from MOSI to MISO, so the master reads back what it sends. Its
//! SCK pin is also routed to EXTI, which counts the clock's edges and reads
//! its idle level, so polarity and byte counts are checked without another
//! wire. The EXTI checks drive a GPIO output into a line over an existing
//! UART link.
//!
//! The firmware keeps the open `Spi` in a [`Bus`] shared with its handlers:
//! the receive stream's calls [`Bus::on_dma`], each EXTI vector's
//! [`Bus::on_exti`].

// Each run binary compiles this file and uses the checks its links need.
#![allow(dead_code)]

use cortex_m::peripheral::{DWT, NVIC};
use embedded_hal::spi::SpiBus;
use ferrohal_stm32::{
    chip::Interrupt,
    exti::{Ctrl, Edge, Exti, ExtiHandle},
    gpio::{Output, Pin, Port, PortInstance},
    spi::{self, Error, MODE_0, MODE_1, MODE_2, MODE_3, Polarity, Spi},
    time::Hertz,
};
use rtic::Mutex;

use crate::tests::{Fail, Report};

/// The SPI driver's `PASS_CYCLES`: the fewest cycles it counts one pass
/// of its completion wait as.
const PASS_FLOOR: u32 = 112;

/// What the handlers share with the checks task.
pub struct Bus<U: spi::SpiParts> {
    pub spi: Option<Spi<U>>,
    /// The last transfer the receive stream's handler ended.
    pub done: Option<Result<usize, Error>>,
    /// The line whose edges the EXTI handler counts.
    pub counted: Option<ExtiHandle>,
    pub edges: u32,
}

impl<U: spi::SpiParts> Bus<U> {
    pub const fn new() -> Self {
        Self {
            spi: None,
            done: None,
            counted: None,
            edges: 0,
        }
    }

    /// For the receive stream's handler.
    pub fn on_dma(&mut self) {
        if let Some(spi) = self.spi.as_mut()
            && let Some(result) = spi.on_interrupt()
        {
            self.done = Some(result);
        }
    }

    /// For an EXTI vector's handler: counts and clears the counted line.
    pub fn on_exti(&mut self) {
        if let Some(line) = self.counted
            && line.is_pending()
        {
            line.clear();
            self.edges += 1;
        }
    }
}

/// Byte `i` of a test pattern seeded by `seed`: a hash, so a dropped or
/// shifted byte shows at once.
fn byte(i: usize, seed: u8) -> u8 {
    ((i as u32 ^ u32::from(seed) << 8).wrapping_mul(2_654_435_761) >> 24) as u8
}

fn pattern<const N: usize>(seed: u8) -> [u8; N] {
    core::array::from_fn(|i| byte(i, seed))
}

fn compare(sent: &[u8], got: &[u8]) -> Result<(), Fail> {
    if got.len() != sent.len() {
        return Err(Fail::Silent {
            missing: sent.len().saturating_sub(got.len()),
        });
    }
    match sent.iter().zip(got).position(|(a, b)| a != b) {
        Some(at) => Err(Fail::Mismatch {
            at,
            sent: sent[at],
            got: got[at],
        }),
        None => Ok(()),
    }
}

fn spin(cycles: u32) {
    cortex_m::asm::delay(cycles);
}

/// Waits for the receive stream's handler to end the transfer: at most
/// `cycles` of the DWT counter. A transfer still running then is aborted,
/// so one failed check leaves the bus ready for the next.
fn wait_done<U: spi::SpiParts, B: Mutex<T = Bus<U>>>(
    bus: &mut B,
    cycles: u32,
) -> Result<usize, Fail> {
    let start = DWT::cycle_count();
    loop {
        if let Some(result) = bus.lock(|b| b.done.take()) {
            return result.map_err(Fail::Spi);
        }
        if DWT::cycle_count().wrapping_sub(start) > cycles {
            bus.lock(|b| b.spi.as_mut().map(Spi::abort));
            return Err(Fail::Spi(Error::Timeout));
        }
    }
}

/// CPU cycles `len` bytes take on the wire at the bus's SCK.
fn wire_cycles<U: spi::SpiParts, B: Mutex<T = Bus<U>>>(bus: &mut B, cpu: Hertz, len: usize) -> u32 {
    let sck = bus.lock(|b| b.spi.as_ref().map_or(1, |s| s.frequency().0.max(1)));
    (u64::from(cpu.0) * 8 * len as u64 / u64::from(sck)) as u32
}

/// One transfer by interrupt: start, wait for the handler, compare.
fn by_interrupt<U: spi::SpiParts, B: Mutex<T = Bus<U>>>(
    bus: &mut B,
    cpu: Hertz,
    sent: &[u8],
) -> Result<(), Fail> {
    bus.lock(|b| {
        b.done = None;
        b.spi.as_mut().map_or(Ok(()), |s| s.start(sent))
    })
    .map_err(Fail::Spi)?;
    let budget = wire_cycles(bus, cpu, sent.len())
        .saturating_mul(4)
        .saturating_add(cpu.0 / 100);
    let len = wait_done(bus, budget)?;
    if len != sent.len() {
        return Err(Fail::Count {
            expected: sent.len() as u32,
            got: len as u32,
        });
    }
    bus.lock(|b| compare(sent, b.spi.as_ref().map_or(&[][..], |s| s.received())))
}

fn set_config<U: spi::SpiParts, B: Mutex<T = Bus<U>>>(
    bus: &mut B,
    config: &spi::Config,
) -> Result<Hertz, Fail> {
    bus.lock(|b| b.spi.as_mut().map(|s| s.set_config(config)))
        .unwrap_or(Err(spi::ConfigError::Busy))
        .map_err(Fail::SpiConfig)
}

/// The slowest SCK this SPI offers: the kernel clock over 256.
fn slowest<U: spi::SpiParts, B: Mutex<T = Bus<U>>>(bus: &mut B) -> Result<spi::Config, Fail> {
    let fastest = set_config(bus, &spi::Config::new(Hertz::mhz(1_000)))?;
    Ok(spi::Config::new(Hertz(fastest.0 / 128)))
}

/// Every SPI check on one MOSI-to-MISO link. `config` is the link's normal
/// setting; `sck` is the SCK pin's EXTI line. The line stays masked except
/// while its edges are counted: lines 5 to 15 share vectors, and a line left
/// unmasked with nothing clearing it would fire for ever.
#[allow(clippy::too_many_arguments)]
pub fn spi_suite<U: spi::SpiParts, B: Mutex<T = Bus<U>>, const P: char, const N: u8>(
    report: &mut Report,
    name: &str,
    bus: &mut B,
    config: &spi::Config,
    cpu: Hertz,
    rx_vector: Interrupt,
    sck: &mut Exti<P, N>,
    ctrl: &mut Ctrl,
) where
    Port<P>: PortInstance,
{
    sck.unlisten(ctrl);
    let cap = bus.lock(|b| b.spi.as_ref().map_or(0, Spi::capacity));
    report.check(name, "receive-stream vector enabled [RESOURCE-001]", {
        bus.lock(|b| b.spi.as_ref().map(|s| s.interrupts().verify()))
            .unwrap_or(Ok(()))
            .map_err(|error| Fail::Irq(Some(error)))
    });

    report.check(
        name,
        "1, 15 and 256 bytes by interrupt [SPI-001 SPI-002 GPIO-001]",
        (|| {
            let sent: [u8; 256] = pattern(1);
            for len in [1, 15, cap.min(256)] {
                by_interrupt(bus, cpu, &sent[..len])?;
            }
            Ok(())
        })(),
    );

    report.check(
        name,
        "SpiBus: in place, read, write, uneven transfer [SPI-007 SPIPOLL-001]",
        (|| {
            // 600 bytes: three transfers through the 256-byte buffers.
            let sent: [u8; 600] = pattern(2);
            let mut buf = sent;
            bus.lock(|b| {
                b.spi
                    .as_mut()
                    .map_or(Ok(()), |s| SpiBus::transfer_in_place(s, &mut buf))
            })
            .map_err(Fail::Spi)?;
            compare(&sent, &buf)?;
            // Reading sends zeros, which come straight back.
            let mut zeros = [0xffu8; 40];
            bus.lock(|b| {
                b.spi
                    .as_mut()
                    .map_or(Ok(()), |s| SpiBus::read(s, &mut zeros))
            })
            .map_err(Fail::Spi)?;
            compare(&[0; 40], &zeros)?;
            bus.lock(|b| {
                b.spi
                    .as_mut()
                    .map_or(Ok(()), |s| SpiBus::write(s, &sent[..100]))
            })
            .map_err(Fail::Spi)?;
            let mut short = [0u8; 10];
            bus.lock(|b| {
                b.spi
                    .as_mut()
                    .map_or(Ok(()), |s| SpiBus::transfer(s, &mut short, &sent[..20]))
            })
            .map_err(Fail::Spi)?;
            compare(&sent[..10], &short)
        })(),
    );

    report.check(
        name,
        "busy, empty and oversized starts refused [SPI-003]",
        (|| {
            let sent: [u8; 256] = pattern(3);
            let busy = bus.lock(|b| {
                b.done = None;
                let s = b.spi.as_mut()?;
                s.start(&sent[..cap.min(256)]).ok()?;
                Some(s.start(&sent[..1]))
            });
            let len = wait_done(bus, cpu.0)?;
            if busy != Some(Err(Error::Busy)) {
                return Err(Fail::SpiNotRefused(busy.and_then(Result::err)));
            }
            if len != cap.min(256) {
                return Err(Fail::Count {
                    expected: cap.min(256) as u32,
                    got: len as u32,
                });
            }
            let empty = bus.lock(|b| b.spi.as_mut().map(|s| s.start(&[])));
            if empty != Some(Err(Error::Empty)) {
                return Err(Fail::SpiNotRefused(empty.and_then(Result::err)));
            }
            let big = [0u8; 257];
            let too_large = bus.lock(|b| b.spi.as_mut().map(|s| s.start(&big[..cap + 1])));
            if too_large != Some(Err(Error::TooLarge)) {
                return Err(Fail::SpiNotRefused(too_large.and_then(Result::err)));
            }
            Ok(())
        })(),
    );

    report.check(
        name,
        "every mode: SCK idles at CPOL, bytes loop [SPI-004 SPIPOLL-001 SPI-V2-001 SPI-V4-001]",
        (|| {
            let sent: [u8; 15] = pattern(4);
            for mode in [MODE_0, MODE_1, MODE_2, MODE_3] {
                set_config(bus, &config.with_mode(mode))?;
                spin(1_000);
                let idle_high = mode.polarity == Polarity::IdleHigh;
                if sck.is_high() != idle_high {
                    return Err(Fail::Level {
                        expected_high: idle_high,
                    });
                }
                by_interrupt(bus, cpu, &sent)?;
                if sck.is_high() != idle_high {
                    return Err(Fail::Level {
                        expected_high: idle_high,
                    });
                }
            }
            set_config(bus, &config.with_bit_order(spi::BitOrder::LsbFirst))?;
            by_interrupt(bus, cpu, &sent)?;
            set_config(bus, config).map(|_| ())
        })(),
    );

    report.check(
        name,
        "SCK: 8 rising edges per byte, modes 0 and 3 [SPI-004 EXTI-001 EXTI-004]",
        (|| {
            let slow = slowest(bus)?;
            for mode in [MODE_0, MODE_3] {
                set_config(bus, &slow.with_mode(mode))?;
                sck.set_edge(ctrl, Edge::Rising);
                sck.clear();
                bus.lock(|b| {
                    b.edges = 0;
                    b.counted = Some(sck.handle());
                });
                sck.listen(ctrl);
                let sent: [u8; 4] = pattern(5);
                let sent_ok = by_interrupt(bus, cpu, &sent);
                spin(10_000);
                sck.unlisten(ctrl);
                let edges = bus.lock(|b| {
                    b.counted = None;
                    b.edges
                });
                sent_ok?;
                if edges != 32 {
                    return Err(Fail::Count {
                        expected: 32,
                        got: edges,
                    });
                }
            }
            set_config(bus, config).map(|_| ())
        })(),
    );

    report.check(
        name,
        "SCK as reported: 64 bytes take 512 periods [SPI-005]",
        (|| {
            for setting in [slowest(bus)?, *config] {
                set_config(bus, &setting)?;
                let sent: [u8; 64] = pattern(6);
                let wire = wire_cycles(bus, cpu, sent.len());
                let start = DWT::cycle_count();
                by_interrupt(bus, cpu, &sent)?;
                let cycles = DWT::cycle_count().wrapping_sub(start);
                // Never faster than the wire; overhead (DMA gaps between bytes,
                // the start and the interrupt) at most a third more plus 20k.
                let (min, max) = (wire - wire / 50, wire + wire / 3 + 20_000);
                if !(min..=max).contains(&cycles) {
                    return Err(Fail::Time { cycles, min, max });
                }
            }
            Ok(())
        })(),
    );

    report.check(
        name,
        "abort mid-transfer, then a clean transfer [SPI-006]",
        (|| {
            let slow = slowest(bus)?;
            set_config(bus, &slow)?;
            let sent: [u8; 256] = pattern(7);
            let aborts = bus.lock(|b| b.spi.as_ref().map_or(0, |s| s.health().aborts));
            bus.lock(|b| {
                b.done = None;
                b.spi
                    .as_mut()
                    .map_or(Ok(()), |s| s.start(&sent[..cap.min(256)]))
            })
            .map_err(Fail::Spi)?;
            // A few bytes in, then pull the plug.
            spin(wire_cycles(bus, cpu, 4));
            bus.lock(|b| b.spi.as_mut().map(Spi::abort));
            let after = bus.lock(|b| b.spi.as_ref().map_or(0, |s| s.health().aborts));
            if after != aborts + 1 {
                return Err(Fail::Count {
                    expected: aborts + 1,
                    got: after,
                });
            }
            spin(wire_cycles(bus, cpu, 2));
            if let Some(stray) = bus.lock(|b| b.done.take()) {
                return Err(Fail::SpiNotRefused(stray.err()));
            }
            set_config(bus, config)?;
            by_interrupt(bus, cpu, &sent[..15])
        })(),
    );

    report.check(
        name,
        "polling with the vector masked [SPI-002]",
        (|| {
            let sent: [u8; 15] = pattern(8);
            NVIC::mask(rx_vector);
            let started = bus.lock(|b| b.spi.as_mut().map_or(Ok(()), |s| s.start(&sent)));
            let start = DWT::cycle_count();
            let polled = loop {
                if let Some(result) = bus.lock(|b| b.spi.as_mut().and_then(Spi::poll)) {
                    break result;
                }
                if DWT::cycle_count().wrapping_sub(start) > cpu.0 / 10 {
                    break Err(Error::Timeout);
                }
            };
            // SAFETY: RTIC bound and unmasked this vector; it is unmasked again.
            unsafe { NVIC::unmask(rx_vector) };
            started.map_err(Fail::Spi)?;
            polled.map_err(Fail::Spi)?;
            bus.lock(|b| compare(&sent, b.spi.as_ref().map_or(&[][..], |s| s.received())))
        })(),
    );

    report.check(
        name,
        "a polling pass takes no less than the driver's floor [SPI-008]",
        (|| {
            // Polls a slow transfer with the vector masked, as a blocking wait
            // does, and times the passes: none may beat `PASS_CYCLES`.
            let slow = slowest(bus)?;
            set_config(bus, &slow)?;
            let sent: [u8; 64] = pattern(9);
            NVIC::mask(rx_vector);
            let started = bus.lock(|b| b.spi.as_mut().map_or(Ok(()), |s| s.start(&sent)));
            let mut fastest = u32::MAX;
            let mut passes = 0;
            loop {
                let start = DWT::cycle_count();
                let done = bus.lock(|b| b.spi.as_mut().and_then(Spi::poll));
                let cycles = DWT::cycle_count().wrapping_sub(start);
                if done.is_some() || passes > 100_000 {
                    break;
                }
                fastest = fastest.min(cycles);
                passes += 1;
            }
            // SAFETY: RTIC bound and unmasked this vector; it is unmasked again.
            unsafe { NVIC::unmask(rx_vector) };
            set_config(bus, config)?;
            started.map_err(Fail::Spi)?;
            defmt::info!(
                "{=str}: fastest polling pass {=u32} cycles over {=u32} passes",
                name,
                fastest,
                passes
            );
            if fastest < PASS_FLOOR {
                return Err(Fail::Bound {
                    waited: fastest,
                    floor: PASS_FLOOR,
                });
            }
            Ok(())
        })(),
    );

    if let Some(health) = bus.lock(|b| b.spi.as_ref().map(Spi::health)) {
        defmt::info!(
            "{=str}: SCK {}, health {}",
            name,
            bus.lock(|b| b.spi.as_ref().map(|s| s.frequency())),
            health
        );
    }
}

/// The EXTI checks on a GPIO output wired to a line's pin.
pub fn exti_suite<
    U: spi::SpiParts,
    B: Mutex<T = Bus<U>>,
    const OP: char,
    const ON: u8,
    const P: char,
    const N: u8,
>(
    report: &mut Report,
    name: &str,
    bus: &mut B,
    out: &mut Pin<OP, ON, Output>,
    exti: &mut Exti<P, N>,
    ctrl: &mut Ctrl,
) where
    Port<OP>: PortInstance,
    Port<P>: PortInstance,
{
    const PULSES: u32 = 10;
    // Long enough for the handler to run between edges.
    const SETTLE: u32 = 20_000;
    exti.listen(ctrl);
    let mut count = |bus: &mut B, out: &mut Pin<OP, ON, Output>, exti: &mut Exti<P, N>, edge| {
        out.set_low();
        spin(SETTLE);
        exti.set_edge(ctrl, edge);
        exti.clear();
        bus.lock(|b| {
            b.edges = 0;
            b.counted = Some(exti.handle());
        });
        for _ in 0..PULSES {
            out.set_high();
            spin(SETTLE);
            out.set_low();
            spin(SETTLE);
        }
        bus.lock(|b| b.edges)
    };
    for (what, edge, expected) in [
        ("rising edges only", Edge::Rising, PULSES),
        ("falling edges only", Edge::Falling, PULSES),
        ("both edges", Edge::Both, 2 * PULSES),
    ] {
        let got = count(bus, out, exti, edge);
        report.check(
            name,
            what,
            if got == expected {
                Ok(())
            } else {
                Err(Fail::Count { expected, got })
            },
        );
    }

    report.check(
        name,
        "masked: edges ignored, then counted again [EXTI-002]",
        (|| {
            exti.set_edge(ctrl, Edge::Rising);
            exti.unlisten(ctrl);
            exti.clear();
            let before = bus.lock(|b| b.edges);
            out.set_high();
            spin(SETTLE);
            out.set_low();
            spin(SETTLE);
            let after = bus.lock(|b| b.edges);
            // A masked line doesn't latch its edges either.
            let pending = exti.is_pending();
            exti.listen(ctrl);
            out.set_high();
            spin(SETTLE);
            out.set_low();
            spin(SETTLE);
            let listening = bus.lock(|b| b.edges);
            if after != before {
                return Err(Fail::Count {
                    expected: before,
                    got: after,
                });
            }
            if pending {
                return Err(Fail::Level {
                    expected_high: false,
                });
            }
            if listening != before + 1 {
                return Err(Fail::Count {
                    expected: before + 1,
                    got: listening,
                });
            }
            Ok(())
        })(),
    );

    report.check(
        name,
        "software trigger raises the interrupt [EXTI-003]",
        (|| {
            let before = bus.lock(|b| b.edges);
            exti.trigger();
            spin(SETTLE);
            let after = bus.lock(|b| b.edges);
            if after == before + 1 {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: before + 1,
                    got: after,
                })
            }
        })(),
    );

    report.check(
        name,
        "level reads follow the pin [EXTI-004 GPIO-001]",
        (|| {
            out.set_high();
            spin(SETTLE);
            let high = exti.is_high();
            out.set_low();
            spin(SETTLE);
            let low = !exti.is_high();
            if high && low {
                Ok(())
            } else {
                Err(Fail::Level {
                    expected_high: !high,
                })
            }
        })(),
    );
    exti.unlisten(ctrl);
    bus.lock(|b| b.counted = None);
}

/// The polled SPI's checks on one MOSI-to-MISO link: no DMA, no
/// interrupts. `sck` is the SCK pin's EXTI line, only read for its level.
pub fn blocking_suite<U: ferrohal_stm32::spi::SpiParts, const P: char, const N: u8>(
    report: &mut Report,
    name: &str,
    spi: &mut ferrohal_stm32::spi::BlockingSpi<U>,
    config: &spi::Config,
    cpu: Hertz,
    sck: &mut Exti<P, N>,
    ctrl: &mut Ctrl,
) where
    Port<P>: PortInstance,
{
    sck.unlisten(ctrl);
    report.check(
        name,
        "SpiBus: in place, read, write, uneven transfer [SPI-007 SPIPOLL-001]",
        (|| {
            let sent: [u8; 600] = pattern(10);
            let mut buf = sent;
            SpiBus::transfer_in_place(spi, &mut buf).map_err(Fail::Spi)?;
            compare(&sent, &buf)?;
            let mut zeros = [0xffu8; 40];
            SpiBus::read(spi, &mut zeros).map_err(Fail::Spi)?;
            compare(&[0; 40], &zeros)?;
            SpiBus::write(spi, &sent[..100]).map_err(Fail::Spi)?;
            let mut short = [0u8; 10];
            SpiBus::transfer(spi, &mut short, &sent[..20]).map_err(Fail::Spi)?;
            compare(&sent[..10], &short)
        })(),
    );
    report.check(
        name,
        "every mode: SCK idles at CPOL, bytes loop [SPI-004 SPIPOLL-001 SPI-V2-001 SPI-V4-001]",
        (|| {
            let sent: [u8; 15] = pattern(11);
            for mode in [MODE_0, MODE_1, MODE_2, MODE_3] {
                spi.set_config(&config.with_mode(mode))
                    .map_err(Fail::SpiConfig)?;
                spin(1_000);
                let idle_high = mode.polarity == Polarity::IdleHigh;
                if sck.is_high() != idle_high {
                    return Err(Fail::Level {
                        expected_high: idle_high,
                    });
                }
                let mut buf = sent;
                SpiBus::transfer_in_place(spi, &mut buf).map_err(Fail::Spi)?;
                compare(&sent, &buf)?;
            }
            spi.set_config(config).map(|_| ()).map_err(Fail::SpiConfig)
        })(),
    );
    report.check(
        name,
        "64 bytes take at least their time on the wire [SPIPOLL-002]",
        (|| {
            let sent: [u8; 64] = pattern(12);
            let mut buf = sent;
            let wire = (u64::from(cpu.0) * 8 * 64 / u64::from(spi.frequency().0)) as u32;
            let start = DWT::cycle_count();
            SpiBus::transfer_in_place(spi, &mut buf).map_err(Fail::Spi)?;
            let cycles = DWT::cycle_count().wrapping_sub(start);
            compare(&sent, &buf)?;
            // One byte in flight: the gaps between bytes add, up to four times
            // the wire time at a fast SCK.
            let (min, max) = (wire - wire / 50, wire * 4 + 20_000);
            if !(min..=max).contains(&cycles) {
                return Err(Fail::Time { cycles, min, max });
            }
            defmt::info!(
                "{=str}: 64 bytes in {=u32} cycles, {=u32} on the wire",
                name,
                cycles,
                wire
            );
            Ok(())
        })(),
    );
}
