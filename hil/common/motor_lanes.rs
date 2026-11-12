//! A flight controller's motor DMA routes, in the flight code's arrangement,
//! through the waveform generator with every output disabled: the timers count
//! and the streams feed their compare registers, and nothing reaches a motor pad. Whether a real frame is
//! right on the wire is the Nucleo benches' check, whose samplers read it.

use ferrohal_stm32::timer::{Awg, ChannelTimer, Lanes};

use crate::usb_bench::{Link, Report};

/// A 1 and a 0 bit's pulse, as a fraction of the period, in sixteenths.
const ONE: u32 = 12;
const ZERO: u32 = 6;
const FRAMES: u32 = 100;

fn frame(pattern: u16, period: u32) -> [u32; 16] {
    core::array::from_fn(|i| {
        period
            * if pattern & (0x8000 >> i) != 0 {
                ONE
            } else {
                ZERO
            }
            / 16
    })
}

/// The waveform generator over four lanes on two timers, however the board
/// starts them, with its outputs as the board set them.
pub fn suite<T1, T2, L1, L2>(report: &mut Report, link: &mut Link, awg: &mut Awg<T1, T2, L1, L2>)
where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    let mut done = 0u32;
    let mut slowest_us = 0u32;
    for round in 0..FRAMES {
        link.service();
        let first = frame(0x8421u16.rotate_left(round), awg.period());
        let second = frame(0x5AA5u16.rotate_left(round), awg.period());
        let loaded = (0..awg.lanes()).all(|lane| {
            awg.load(lane, if lane < 2 { &first } else { &second })
                .is_ok()
        });
        if !loaded || awg.play().is_err() {
            awg.abort();
            continue;
        }
        let began = link.now();
        let mut finished = [false; 4];
        let mut failed = false;
        while finished.iter().any(|seen| !seen) && link.now().wrapping_sub(began) < 2_000 {
            for (lane, seen) in finished.iter_mut().enumerate() {
                if !*seen {
                    if let Some(result) = awg.poll(lane) {
                        *seen = true;
                        failed |= result.is_err();
                    }
                }
            }
        }
        slowest_us = slowest_us.max(link.now().wrapping_sub(began));
        if finished.iter().any(|seen| !seen) {
            awg.abort();
        } else if !failed {
            done += 1;
        }
    }
    check!(
        report,
        link,
        done == FRAMES,
        "motors: {} of {} frames on all four streams through the AWG, slowest {} us [AWG-005]",
        done,
        FRAMES,
        slowest_us
    );
}

/// Every lane reading its pad's port, on the flight routes: all four finish. Their
/// outputs are released, so nothing is sent, and they end released again.
pub fn listen_suite<T1, T2, L1, L2>(
    report: &mut Report,
    link: &mut Link,
    awg: &mut Awg<T1, T2, L1, L2>,
) where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    const CAPTURES: usize = 64;
    link.service();
    let started = awg
        .listen(ferrohal_stm32::time::Hertz::mhz(1), CAPTURES)
        .is_ok();
    let began = link.now();
    let mut finished = [false; 4];
    let mut failed = !started;
    while started && finished.iter().any(|seen| !seen) && link.now().wrapping_sub(began) < 2_000 {
        for (lane, seen) in finished.iter_mut().enumerate() {
            if !*seen {
                if let Some(result) = awg.poll(lane) {
                    *seen = true;
                    failed |= result.is_err();
                }
            }
        }
    }
    let all = finished.iter().all(|seen| *seen);
    if !all {
        awg.abort();
    }
    let read = (0..4)
        .filter(|lane| awg.samples(*lane).len() == CAPTURES)
        .count();
    check!(
        report,
        link,
        all && !failed && read == 4,
        "motors: all four lanes read their pads over DMA, {} of 4 [AWG-012]",
        read
    );
    let _ = awg.speak();
    awg.set_outputs(false);
}
