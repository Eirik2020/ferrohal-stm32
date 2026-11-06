//! Run Q: the I2C master, I2C1 on PB8 and PB9 (the Arduino connector's
//! SCL and SDA), pulled up internally, no slave fitted. The bus itself
//! answers a vacant address with a NACK, which is what the run proves the
//! driver's error paths with. No links; nothing else on the bench uses
//! these pins.

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
use ferrohal_stm32::{chip::Peripherals, i2c::{Config, ConfigError, I2c}, time::Hertz};
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
    let mut run = match board::run_q(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
    defmt::info!("run Q, clocks: {}", board::CLOCKS);
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let cpu = board::CLOCKS.cpu;
    let mut report = Report::new();
    let link = "I2C1 PB8/PB9 (Arduino SCL/SDA, no slave)";

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
            // The timing registers hold what the driver computed for the
            // kernel clock it reports: FREQ in whole MHz, CCR the divider
            // RM0368 §27.6.11 gives for standard mode, TRISE the rise
            // count, and the rate reported the one those make.
            report.check_for(
                link,
                "I2C-001,I2C-005",
                "the timing registers make the reported rate",
                {
                    let kernel = bus.kernel_clock();
                    let speed = bus.frequency();
                    let freq = (kernel.0 / 1_000_000) as u8;
                    let cr2 = pac::I2C1.cr2().read();
                    let ccr = pac::I2C1.ccr().read();
                    let trise = pac::I2C1.trise().read();
                    let expected = kernel.0 / (2 * speed.0);
                    if cr2.freq() != freq {
                        Err(Fail::Count {
                            expected: u32::from(freq),
                            got: u32::from(cr2.freq()),
                        })
                    } else if u32::from(ccr.ccr()) != expected {
                        Err(Fail::Count {
                            expected,
                            got: u32::from(ccr.ccr()),
                        })
                    } else if u32::from(trise.trise()) != u32::from(freq) + 1 {
                        Err(Fail::Count {
                            expected: u32::from(freq) + 1,
                            got: u32::from(trise.trise()),
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
