//! Timer PWM: fades LD2 up and down with TIM2's channel 1.
//!
//! LD2 is PA5, which also carries TIM2_CH1 on alternate function 1
//! (DS10086, alternate function mapping). The pin type proves the channel
//! reaches it: `Pwm::channel::<1, _>` takes only a pin with TIM2_CH1, so a
//! wrong pin or function does not build.
//!
//! No wiring: LD2 is on the board.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    chip::Peripherals,
    gpio::{Alternate, Pin},
    rcc::Config,
    time::Hertz,
    timer::Pwm,
};
use panic_probe as _;

const CPU_HZ: u32 = 16_000_000;
/// Steps from dark to full and back, and the time each step holds.
const STEPS: u32 = 100;
const STEP_MS: u32 = 10;

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    let mut rcc = match p.RCC.freeze(&Config::hsi()) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let led: Pin<'A', 5, Alternate<1>> = gpioa.p5.into_alternate(&mut gpioa.ctrl);

    // 1 kHz, as near as the timer's clock allows; a rate it cannot make is
    // refused with the TIM2 token handed back.
    let pwm = match Pwm::new(&mut rcc, p.TIM2, Hertz::khz(1)) {
        Ok(pwm) => pwm,
        Err(refused) => defmt::panic!("TIM2: {}", refused.error()),
    };
    // The channel borrows the timer, so the timer cannot be freed under it.
    let mut channel = pwm.channel::<1, _>(led);
    pwm.start();
    // A duty of `period` is fully on; 0 is off.
    let period = pwm.period();
    defmt::info!("pwm-fade: {} Hz, period {} ticks", pwm.rate().0, period);

    loop {
        for step in (0..=STEPS).chain((0..STEPS).rev()) {
            channel.set_duty(period * step / STEPS);
            cortex_m::asm::delay(CPU_HZ / 1000 * STEP_MS);
        }
    }
}
