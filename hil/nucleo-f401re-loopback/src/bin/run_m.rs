//! Run M: the ADC. PA7 drives PA6 over S1, read on ADC1 channel 6, one at a
//! time and then as a sequence with the internal channels, over DMA2 stream 0.

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

static mut SAMPLES: [u16; 3] = [0; 3];
const SETTLE: u32 = 84_000;
const BOUND: u32 = 840_000;

#[entry]
fn main() -> ! {
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_m(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.sysclk.0);
    defmt::info!("run M, clocks: {}", board::CLOCKS);
    let mut report = Report::new();

    let uid = signature::device_uid();
    defmt::info!("unique ID: {=[u8]:02x}", uid);
    report.check(
        "unique ID",
        "isn't blank [SIG-001]",
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
    let direct = stm32_metapac::DBGMCU.idcode().read().rev_id();
    defmt::info!("die revision: {=u16:#06x}", revision);
    report.check(
        "die revision",
        "the glue reads DBGMCU's REV_ID, a known code [SIG-002]",
        if revision == direct && matches!(revision, 0x1000 | 0x1001) {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: u32::from(direct),
                got: u32::from(revision),
            })
        },
    );

    let name = "ADC1 PA6";
    let mut adc = match Adc::new(&mut run.rcc, run.adc, &Config::slow_source()) {
        Ok(adc) => adc,
        Err(error) => defmt::panic!("ADC: {}", error),
    };
    let pin = adc.channel(&run.analog);
    let link = &mut run.drive;
    adc::rails(&mut report, name, &mut adc, pin, SETTLE, |high| {
        if high {
            link.set_high()
        } else {
            link.set_low()
        }
    });
    adc::internal(&mut report, "ADC1 internal", &mut adc);

    let channels = [pin, adc.vrefint_channel(), adc.temperature_channel()];
    // SAFETY: `main` runs once, and this is the only reference.
    let samples: &'static mut [u16] = unsafe { &mut *addr_of_mut!(SAMPLES) };
    let mut scan = match adc.into_scan(run.stream, &channels, samples) {
        Ok(scan) => scan,
        Err(error) => defmt::panic!("scan: {}", error),
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
            let [pin, reference, temperature] = samples else {
                return false;
            };
            let pin_ok = if high {
                u32::from(*pin) * 100 >= u32::from(max) * 97
            } else {
                u32::from(*pin) * 100 <= u32::from(max) * 3
            };
            // 1.21 V of 3.3 V, and the sensor's 0.76 V at 25 C, near enough.
            pin_ok && (1300..=1700).contains(reference) && (800..=1200).contains(temperature)
        },
    );
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
