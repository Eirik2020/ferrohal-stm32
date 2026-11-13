//! Run S: the I2C master against the Pico 1 simulator, I2C4 on PD12 and
//! PD13, pulled up internally. The NACK-only suite runs first,
//! unchanged: nothing answers the vacant address it uses. Then the
//! live suite (`i2c_live.rs`) drives the ACK'd paths against the
//! simulator's register map. I2C4 is this chip's only benchable I2C
//! (`hil/WIRING.md` has the accounting), and the one hung off APB4:
//! its kernel clock must be exactly the tree's pclk4, which no other
//! bench checks. The simulator's tag for this bus is 2.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/i2c.rs"]
mod i2c;
#[path = "../../../common/i2c_live.rs"]
mod i2c_live;
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
/// The simulator's name for this bus, in its ID register.
const TAG: u8 = 2;

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_s(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
    defmt::info!("run S, clocks: {}", board::CLOCKS);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let cpu = board::CLOCKS.cpu;
    let mut report = Report::new();
    let link = "I2C4 PD12/PD13 (Pico simulator)";

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
            // The kernel clock this instance runs on is the APB4 one:
            // exactly the tree's pclk4, the path no other bench covers.
            report.check_for(
                link,
                "I2C-001",
                "the kernel clock is the APB4 prescaler's",
                {
                    let kernel = bus.kernel_clock();
                    if kernel != board::CLOCKS.pclk4 {
                        Err(Fail::Count {
                            expected: board::CLOCKS.pclk4.0,
                            got: kernel.0,
                        })
                    } else {
                        Ok(())
                    }
                },
            );
            // The timing register holds what the recipe of RM0433
            // §47.4.5 promises for that kernel clock: the low and high
            // times over standard mode's minimums, and the rate
            // reported at or below the one asked for.
            report.check_for(
                link,
                "I2C-001,I2C-005",
                "the timing register makes the reported rate",
                {
                    let kernel = bus.kernel_clock();
                    let speed = bus.frequency();
                    let timingr = pac::I2C4.timingr().read();
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
            let failed_before = report.failed;
            i2c::suite(&mut report, link, &mut bus, cpu);
            // The v2 recovery is a PE cycle and the ICR clear: a few
            // register writes past the deadline, and nothing more.
            i2c_live::suite(&mut report, link, &mut bus, cpu, TAG, 2_000);
            // Every transfer above walked this IP version's register
            // sequence; the version's own requirement stands on them all.
            report.check_for(
                link,
                "I2C-V2-001",
                "every transfer above walked this version's register sequence",
                if report.failed == failed_before {
                    Ok(())
                } else {
                    Err(Fail::Count {
                        expected: 0,
                        got: report.failed - failed_before,
                    })
                },
            );
        }
        Err(_) => report.check(link, "the handed-back wiring opens the bus", {
            Err(Fail::I2cConfig(ConfigError::Speed))
        }),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
