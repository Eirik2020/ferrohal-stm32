//! Run R: the I2C master, I2C3 on PA8 and PC9, pulled up internally, no
//! slave fitted. The bus itself answers a vacant address with a NACK,
//! which is what the run proves the driver's error paths with. No links
//! touch these pins, so the whole bench runs as one script. No vectors:
//! the I2C needs none.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/i2c.rs"]
mod i2c;
#[path = "../../../common/tests.rs"]
mod tests;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    chip::Peripherals,
    i2c::{Config, ConfigError, I2c},
    time::Hertz,
};
use panic_probe as _;
use stm32_metapac as pac;

use tests::{Fail, Report};

/// The bench speed: standard mode, slow enough that the internal ~40 k
/// pull-ups give the lines their rise.
const SPEED: Hertz = Hertz::khz(50);

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_r(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
    defmt::info!("run R, clocks: {}", board::CLOCKS);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let cpu = board::CLOCKS.cpu;
    let mut report = Report::new();
    let link = "I2C3 PA8/PC9 (no slave)";

    // A speed the IP version cannot carry is refused, the wiring handed
    // back, and the handed-back wiring opens the bus.
    let bus = match I2c::new(&mut run.rcc, run.i2c, &Config::new(Hertz::mhz(5))) {
        Err(refused) => {
            report.check_for(
                link,
                "I2C-002",
                "an out-of-range speed is refused",
                if refused.error() == ConfigError::Speed {
                    Ok(())
                } else {
                    Err(Fail::I2cConfig(refused.error()))
                },
            );
            let wiring = refused.into_parts();
            I2c::new(&mut run.rcc, wiring, &Config::new(SPEED))
        }
        Ok(bus) => {
            report.check_for(
                link,
                "I2C-002",
                "an out-of-range speed is refused",
                Err(Fail::I2cNotNack(None)),
            );
            Ok(bus)
        }
    };
    match bus {
        Ok(mut bus) => {
            // The timing register holds what the recipe of RM0433 §47.4.5
            // promises for the kernel clock the driver reports: the low
            // and high times over standard mode's minimums, and the
            // rate reported at or below the one asked for.
            report.check_for(
                link,
                "I2C-001,I2C-005",
                "the timing register makes the reported rate",
                {
                    let kernel = bus.kernel_clock();
                    let speed = bus.frequency();
                    let timingr = pac::I2C3.timingr().read();
                    let tick = (1_000_000_000 + kernel.0 - 1) / kernel.0;
                    let tp = tick * (u32::from(timingr.presc()) + 1);
                    let low = tp * (u32::from(timingr.scll()) + 1);
                    let high = tp * (u32::from(timingr.sclh()) + 1);
                    if low < 4_700 {
                        Err(Fail::Time {
                            cycles: low,
                            min: 4_700,
                            max: 20_000,
                        })
                    } else if high < 4_000 {
                        Err(Fail::Time {
                            cycles: high,
                            min: 4_000,
                            max: 20_000,
                        })
                    } else if low + high < 1_000_000_000 / speed.0.max(1) {
                        Err(Fail::Time {
                            cycles: low + high,
                            min: 1_000_000_000 / speed.0.max(1),
                            max: 2_000_000_000,
                        })
                    } else if speed.0 > SPEED.0 {
                        Err(Fail::I2cConfig(ConfigError::Speed))
                    } else {
                        Ok(())
                    }
                },
            );
            bus.set_clock(Some(&tests::DWT_CLOCK));
            i2c::suite(&mut report, link, &mut bus, cpu);
        }
        Err(_) => report.check(link, "the handed-back wiring opens the bus", {
            Err(Fail::I2cConfig(ConfigError::Speed))
        }),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
