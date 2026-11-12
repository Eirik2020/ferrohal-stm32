//! Run S: the I2C master against the Pico 1 simulator, I2C3 on PA8 and
//! PC9, pulled up internally. The NACK-only suite runs first, unchanged:
//! nothing answers the vacant address it uses. Then the live suite
//! (`i2c_live.rs`) drives the ACK'd paths against the simulator's
//! register map — reads of every length, a write that lands, a repeated
//! start, a stretch within the bound and past it, the data-phase
//! refusal — with the simulator's own counters cross-checked against
//! the transactions this run makes. The wiring list is
//! `hil/WIRING.md`; the simulator's tag for this bus is 1.

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
const TAG: u8 = 1;

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
    let link = "I2C3 PA8/PC9 (Pico simulator)";

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
                    let cr2 = pac::I2C3.cr2().read();
                    let ccr = pac::I2C3.ccr().read();
                    let trise = pac::I2C3.trise().read();
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
            // The v1 recovery's BUSY spin: ten thousand SR2 reads while the
            // simulator still holds the line, past the deadline the
            // out-of-bound stretch check's ceiling must cover.
            i2c_live::suite(&mut report, link, &mut bus, cpu, TAG, 120_000);
        }
        Err(_) => report.check(link, "the handed-back wiring opens the bus", {
            Err(Fail::I2cConfig(ConfigError::Speed))
        }),
    }
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
