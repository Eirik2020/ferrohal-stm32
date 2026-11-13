//! Run P: TIM1 starting TIM8 from its trigger output, as the Foxeer's motor
//! timers start. No links: no output is enabled. Then the waveform generator
//! over the same pair, linked: a lane on TIM1 channel 1 (PE9) and one on TIM8
//! channel 1 (PC6), DMA1 streams 0 and 1, outputs off, and every lane of both
//! must finish [AWG-005].

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/pwm.rs"]
mod pwm;
#[path = "../../../common/tests.rs"]
mod tests;

use core::ptr::addr_of_mut;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    chip::Peripherals,
    time::Hertz,
    timer::{Awg, Pwm},
};
use panic_probe as _;

/// The two lanes' sample buffers, in AXI SRAM, which DMA1 reaches; the data
/// cache stays off in this run.
static mut LANES: [[u32; 20]; 2] = [[0; 20]; 2];

use tests::Report;

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let (tim_m, tim_s) = (p.TIM1, p.TIM8);
    let (gpioc, gpioe, dma1) = (p.GPIOC, p.GPIOE, p.DMA1);
    let mut rcc = match p.RCC.freeze(&board::CLOCK_CONFIG) {
        Ok(rcc) => rcc,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    let (Ok(mut master), Ok(mut slave)) = (
        Pwm::new(&mut rcc, tim_m, Hertz::khz(2)),
        Pwm::new(&mut rcc, tim_s, Hertz::khz(2)),
    ) else {
        defmt::panic!("timers")
    };
    let mut report = Report::new();
    pwm::trigger_suite(&mut report, "TIM1 starts TIM8", &mut master, &mut slave, 64);

    // The generator over the same pair, linked.
    let name = "TIM1 starts TIM8, linked";
    let mut c = gpioc.split(&mut rcc);
    let mut e = gpioe.split(&mut rcc);
    let dma1 = dma1.split(&mut rcc);
    let pe9 = e.p9.into_alternate::<1>(&mut e.ctrl);
    let pc6 = c.p6.into_alternate::<3>(&mut c.ctrl);
    // SAFETY: `main` runs once, and these are the only references.
    let [one, two] = unsafe { &mut *addr_of_mut!(LANES) };
    let rated = master
        .set_frequency(Hertz::khz(150))
        .and_then(|_| slave.set_frequency(Hertz::khz(150)));
    let lane1 = ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(pe9, dma1.s0).assemble(&master, &mut one[..]);
    let lane8 = ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(pc6, dma1.s1).assemble(&slave, &mut two[..]);
    match (rated, lane1, lane8) {
        (Ok(_), Ok(lane1), Ok(lane8)) => match Awg::linked(master, slave, (lane1,), (lane8,)) {
            Ok(mut awg) => {
                awg.set_outputs(false);
                pwm::awg_pair_check(&mut report, name, &mut awg, board::CLOCKS.cpu);
            }
            Err(refused) => report.check(
                name,
                "AWG: every lane on both timers finishes [AWG-005]",
                Err(tests::Fail::Timer(refused.error())),
            ),
        },
        _ => report.check(
            name,
            "AWG: every lane on both timers finishes [AWG-005]",
            Err(tests::Fail::Count {
                expected: 2,
                got: 0,
            }),
        ),
    }
    tests::finish(&report, || {});
}
