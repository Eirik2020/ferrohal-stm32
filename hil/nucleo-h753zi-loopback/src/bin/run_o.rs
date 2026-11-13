//! Run O: the ADCs. PD7 drives PA6 over S1, read on ADC1 channel 3, on DMA1
//! stream 0 for its sequence. PF7 drives PF6 over Z10, read on ADC3 channel 8
//! with the internal channels, on DMA2 stream 0.

#![no_std]
#![no_main]

#[path = "../../../common/adc.rs"]
mod adc;
#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use core::ptr::addr_of_mut;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    adc::{Adc, Config},
    chip::Peripherals,
    signature,
};
use panic_probe as _;

use tests::{Fail, Report};

static mut SAMPLES1: [u16; 2] = [0; 2];
static mut SAMPLES3: [u16; 3] = [0; 3];
const SETTLE: u32 = 400_000;
const BOUND: u32 = 40_000_000;

#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_o(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    defmt::info!("run O, clocks: {}", board::CLOCKS);
    let mut report = Report::new();

    let uid = signature::device_uid();
    defmt::info!("unique ID: {=[u8]:02x}", uid);
    report.check(
        "unique ID",
        "isn't blank [UID-001]",
        if uid != [0; 12] && uid != [0xFF; 12] {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 1,
                got: 0,
            })
        },
    );

    // The die revision, through this family's glue, against DBGMCU read here
    // directly; and a code the reference manual lists for this family.
    let revision = signature::revision();
    let direct = stm32_metapac::DBGMCU.idc().read().rev_id();
    defmt::info!("die revision: {=u16:#06x}", revision);
    report.check(
        "die revision",
        "the glue reads DBGMCU's REV_ID, a known code [REVISION-001]",
        if revision == direct && matches!(revision, 0x1003 | 0x2001 | 0x2003) {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: u32::from(direct),
                got: u32::from(revision),
            })
        },
    );

    let mut adc1 = match Adc::new(&mut run.rcc, run.adc1, &Config::slow_source()) {
        Ok(adc) => adc,
        Err(error) => defmt::panic!("ADC1: {}", error),
    };
    let pin = adc1.channel(&run.analog1);
    let link = &mut run.drive1;
    adc::rails(&mut report, "ADC1 PA6", &mut adc1, pin, SETTLE, |high| {
        if high {
            link.set_high()
        } else {
            link.set_low()
        }
    });
    // SAFETY: `main` runs once, and this is the only reference.
    let samples: &'static mut [u16] = unsafe { &mut *addr_of_mut!(SAMPLES1) };
    let mut scan = match adc1.into_scan(run.stream1, &[pin, pin], samples) {
        Ok(scan) => scan,
        Err(error) => defmt::panic!("ADC1 scan: {}", error),
    };
    adc::scan(
        &mut report,
        "ADC1 scan",
        &mut scan,
        SETTLE,
        BOUND,
        |high| {
            if high {
                link.set_high()
            } else {
                link.set_low()
            }
        },
        |samples, high, max| {
            samples.len() == 2
                && samples.iter().all(|&sample| {
                    if high {
                        u32::from(sample) * 100 >= u32::from(max) * 97
                    } else {
                        u32::from(sample) * 100 <= u32::from(max) * 3
                    }
                })
        },
    );

    let mut adc3 = match Adc::new(&mut run.rcc, run.adc3, &Config::slow_source()) {
        Ok(adc) => adc,
        Err(error) => defmt::panic!("ADC3: {}", error),
    };
    let pin = adc3.channel(&run.analog3);
    let link = &mut run.drive3;
    adc::rails(&mut report, "ADC3 PF6", &mut adc3, pin, SETTLE, |high| {
        if high {
            link.set_high()
        } else {
            link.set_low()
        }
    });
    adc::internal(&mut report, "ADC3 internal", &mut adc3);
    let channels = [pin, adc3.vrefint_channel(), adc3.temperature_channel()];
    // SAFETY: `main` runs once, and this is the only reference.
    let samples: &'static mut [u16] = unsafe { &mut *addr_of_mut!(SAMPLES3) };
    let mut scan = match adc3.into_scan(run.stream3, &channels, samples) {
        Ok(scan) => scan,
        Err(error) => defmt::panic!("ADC3 scan: {}", error),
    };
    adc::scan(
        &mut report,
        "ADC3 scan",
        &mut scan,
        SETTLE,
        BOUND,
        |high| {
            if high {
                link.set_high()
            } else {
                link.set_low()
            }
        },
        |samples, high, max| {
            let [pin, reference, _] = samples else {
                return false;
            };
            let pin_ok = if high {
                u32::from(*pin) * 100 >= u32::from(max) * 97
            } else {
                u32::from(*pin) * 100 <= u32::from(max) * 3
            };
            // 1.216 V of 3.3 V, 16 bits.
            pin_ok && (20_000..=28_000).contains(reference)
        },
    );
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
