//! A flight controller's motor DMA routes, in the flight code's arrangement,
//! with every output disabled: the timers count and the streams feed their
//! compare registers, and nothing reaches a motor pad. Whether a real frame is
//! right on the wire is the Nucleo benches' check, whose samplers read it.

use ferrohal_stm32::timer::{ChannelTimer, CompareLane, Pwm};

use crate::usb_bench::{Link, Report};

/// A 1 and a 0 bit's pulse, as a fraction of the period, in sixteenths.
const ONE: u32 = 12;
const ZERO: u32 = 6;
const FRAMES: u32 = 100;

fn frame(pattern: u16, period: u32) -> [u32; 16] {
    core::array::from_fn(|i| {
        period * if pattern & (0x8000 >> i) != 0 { ONE } else { ZERO } / 16
    })
}

/// Four lanes on two timers. `start` sets the timers going however the board
/// does; `masks` are the channels of each, for `stage`.
#[allow(clippy::too_many_arguments)]
pub fn suite<T1, T2, const A: u8, const B: u8, const C: u8, const D: u8>(
    report: &mut Report,
    link: &mut Link,
    first: &mut Pwm<T1>,
    second: &mut Pwm<T2>,
    lanes: (
        &mut CompareLane<T1, A>,
        &mut CompareLane<T1, B>,
        &mut CompareLane<T2, C>,
        &mut CompareLane<T2, D>,
    ),
    masks: (u8, u8),
    mut start: impl FnMut(&mut Pwm<T1>, &mut Pwm<T2>),
) where
    T1: ChannelTimer,
    T2: ChannelTimer,
{
    let (a, b, c, d) = lanes;
    let mut done = 0u32;
    let mut slowest_us = 0u32;
    for round in 0..FRAMES {
        link.service();
        let duties = frame(0x8421u16.rotate_left(round), first.period());
        let others = frame(0x5AA5u16.rotate_left(round), second.period());
        let armed = a.arm(&duties).is_ok()
            && b.arm(&duties).is_ok()
            && c.arm(&others).is_ok()
            && d.arm(&others).is_ok();
        if !armed {
            continue;
        }
        first.stage(masks.0);
        second.stage(masks.1);
        let began = link.now();
        start(first, second);
        let mut left = 4u32;
        let mut failed = false;
        let mut finished = [false; 4];
        while left > 0 && link.now().wrapping_sub(began) < 2_000 {
            let results = [
                if finished[0] { None } else { a.poll() },
                if finished[1] { None } else { b.poll() },
                if finished[2] { None } else { c.poll() },
                if finished[3] { None } else { d.poll() },
            ];
            for (seen, result) in finished.iter_mut().zip(results) {
                if let Some(result) = result {
                    *seen = true;
                    left -= 1;
                    failed |= result.is_err();
                }
            }
        }
        slowest_us = slowest_us.max(link.now().wrapping_sub(began));
        first.stop();
        second.stop();
        first.close_requests();
        second.close_requests();
        if left > 0 {
            a.abort();
            b.abort();
            c.abort();
            d.abort();
        } else if !failed {
            done += 1;
        }
    }
    check!(
        report,
        link,
        done == FRAMES,
        "motors: {} of {} frames on all four streams, outputs off, slowest {} us",
        done,
        FRAMES,
        slowest_us
    );
}
