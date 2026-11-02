//! A flight controller's battery inputs, read as the flight code reads them:
//! a sequence over DMA. With no battery on USB power they read whatever the
//! board's dividers settle at, so this checks the path, not the volts.

use ferrohal_stm32::adc::{self, AdcScan};

use crate::usb_bench::{Link, Report};

/// Runs the sequence a hundred times, polled, and reports the last samples
/// under `names`, in millivolts of `vdda_mv`.
pub fn suite<A: adc::Instance>(
    report: &mut Report,
    link: &mut Link,
    scan: &mut AdcScan<A>,
    names: &[&str],
    vdda_mv: u32,
) {
    let mut failures = 0u32;
    let mut slowest_us = 0u32;
    for _ in 0..100 {
        let began = link.now();
        if scan.start().is_err() {
            failures += 1;
            continue;
        }
        let finished = loop {
            link.service();
            if let Some(done) = scan.poll() {
                break done.is_ok();
            }
            if link.now().wrapping_sub(began) > 20_000 {
                scan.abort();
                break false;
            }
        };
        slowest_us = slowest_us.max(link.now().wrapping_sub(began));
        failures += u32::from(!finished);
    }
    check!(
        report,
        link,
        failures == 0,
        "adc: 100 sequences of {} channels over DMA, {} failed, slowest {} us",
        names.len(),
        failures,
        slowest_us
    );
    let samples = scan.samples();
    let in_scale = samples.len() == names.len() && samples.iter().all(|&s| s <= scan.max_sample());
    check!(report, link, in_scale, "adc: {} samples in scale", samples.len());
    let values: [u16; 4] = core::array::from_fn(|i| samples.get(i).copied().unwrap_or(0));
    for (name, &sample) in names.iter().zip(&values) {
        link.line(format_args!(
            "INFO adc: {} is {} mV ({})",
            name,
            scan.to_millivolts(sample, vdda_mv),
            sample
        ));
    }
}
