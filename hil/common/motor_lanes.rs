//! A flight controller's motor DMA routes, in the flight code's arrangement,
//! through the lane group with every output disabled: the timers count
//! and the streams feed their compare registers, and nothing reaches a motor pad. Whether a real frame is
//! right on the wire is the Nucleo benches' check, whose samplers read it.

use ferrohal_stm32::timer::{LaneGroup, ChannelTimer, Frame, Lanes};

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

/// Frame memory, one block per lane: a lane keeps none of its own (FerroHAL
/// `docs/design/lanes.md`), so the suite writes each frame here and lends it.
pub type Memory = [Option<&'static mut [u32]>; 4];

/// Loads `duties` on `lane` from its block, the lane's last frame back first.
fn load<T1, T2, L1, L2>(
    lane_group: &mut LaneGroup<T1, T2, L1, L2>,
    memory: &mut Memory,
    lane: usize,
    duties: &[u32],
) -> bool
where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    let Some(slot) = memory.get_mut(lane) else {
        return false;
    };
    if let Some(frame) = lane_group.take_frame(lane) {
        *slot = Some(frame.into_words());
    }
    let (Some(words), Some(limits)) = (slot.take(), lane_group.frame_limits(lane)) else {
        return false;
    };
    if let Some(head) = words.get_mut(..duties.len()) {
        head.copy_from_slice(duties);
    }
    match Frame::new(words, duties.len(), limits) {
        Ok(frame) => match lane_group.load(lane, frame) {
            Ok(()) => true,
            Err(refused) => {
                *slot = Some(refused.into_parts().into_words());
                false
            }
        },
        Err(refused) => {
            *slot = Some(refused.into_parts());
            false
        }
    }
}

/// One service of every lane (`LaneGroup::on_interrupt`): marks the lanes that
/// ended, and says whether the lane group failed, which stops every lane, so
/// all are marked.
fn serve<T1, T2, L1, L2>(lane_group: &mut LaneGroup<T1, T2, L1, L2>, finished: &mut [bool; 4]) -> bool
where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    match lane_group.on_interrupt() {
        Some(Ok(ended)) => {
            for (lane, seen) in finished.iter_mut().enumerate() {
                *seen |= ended & (1 << lane) != 0;
            }
            false
        }
        Some(Err(_)) => {
            *finished = [true; 4];
            true
        }
        None => false,
    }
}

/// The lane group over four lanes on two timers, however the board
/// starts them, with its outputs as the board set them.
pub fn suite<T1, T2, L1, L2>(
    report: &mut Report,
    link: &mut Link,
    lane_group: &mut LaneGroup<T1, T2, L1, L2>,
    memory: &mut Memory,
) where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    let mut done = 0u32;
    let mut slowest_us = 0u32;
    for round in 0..FRAMES {
        link.service();
        let first = frame(0x8421u16.rotate_left(round), lane_group.period());
        let second = frame(0x5AA5u16.rotate_left(round), lane_group.period());
        let loaded = (0..lane_group.lanes())
            .all(|lane| load(lane_group, memory, lane, if lane < 2 { &first } else { &second }));
        if !loaded || lane_group.play().is_err() {
            lane_group.abort();
            continue;
        }
        let began = link.now();
        let mut finished = [false; 4];
        let mut failed = false;
        while finished.iter().any(|seen| !seen) && link.now().wrapping_sub(began) < 2_000 {
            failed |= serve(lane_group, &mut finished);
        }
        slowest_us = slowest_us.max(link.now().wrapping_sub(began));
        if finished.iter().any(|seen| !seen) {
            lane_group.abort();
        } else if !failed {
            done += 1;
        }
    }
    check!(
        report,
        link,
        done == FRAMES,
        "motors: {} of {} frames on all four streams through the lane group, slowest {} us",
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
    lane_group: &mut LaneGroup<T1, T2, L1, L2>,
) where
    T1: ChannelTimer,
    T2: ChannelTimer,
    L1: Lanes<T1>,
    L2: Lanes<T2>,
{
    const CAPTURES: usize = 64;
    link.service();
    let started = lane_group.prepare_listen(ferrohal_stm32::time::Hertz::mhz(1), CAPTURES).and_then(|rate| lane_group.listen().map(|()| rate))
        .is_ok();
    let began = link.now();
    let mut finished = [false; 4];
    let mut failed = !started;
    while started && finished.iter().any(|seen| !seen) && link.now().wrapping_sub(began) < 2_000 {
        failed |= serve(lane_group, &mut finished);
    }
    let all = finished.iter().all(|seen| *seen);
    if !all {
        lane_group.abort();
    }
    let read = (0..4)
        .filter(|lane| lane_group.samples(*lane).len() == CAPTURES)
        .count();
    check!(
        report,
        link,
        all && !failed && read == 4,
        "motors: all four lanes read their pads over DMA, {} of 4",
        read
    );
    let _ = lane_group.speak();
    lane_group.set_outputs(false);
}
