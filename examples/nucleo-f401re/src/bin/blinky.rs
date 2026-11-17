//! GPIO: blinks LD2, the green user LED, and blinks it faster while B1, the
//! blue user button, is held.
//!
//! - LD2 is PA5, active high (UM1724, LEDs).
//! - B1 is PC13, low while pressed (UM1724, push-buttons). The pin is
//!   pulled up here, so it reads high when the button is released.
//!
//! No wiring: both are on the board.
//!
//! FerroHAL has no delay or time source of its own: this counts CPU cycles
//! with `cortex_m::asm::delay`, which is fine for a blink. A firmware that
//! keeps time uses a timer (`timer::Timebase`) or its scheduler's.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use embedded_hal::digital::InputPin;
use ferrohal_stm32::{chip::Peripherals, gpio::Pull, rcc::Config};
use panic_probe as _;

/// The reset clock: 16 MHz from HSI.
const CPU_HZ: u32 = 16_000_000;

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    // The reset state, made explicit: the RCC owner the drivers ask for
    // their clocks.
    let mut rcc = match p.RCC.freeze(&Config::hsi()) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };

    // Splitting a port turns its clock on and hands out one token per pin;
    // `ctrl` owns the port's shared mode registers.
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let mut gpioc = p.GPIOC.split(&mut rcc);
    let mut led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
    let mut button = gpioc.p13.into_input(&mut gpioc.ctrl);
    button.set_pull(&mut gpioc.ctrl, Pull::Up);

    defmt::info!("blinky: hold B1 to blink faster");
    loop {
        let pressed = button.is_low().unwrap_or(false);
        let half_period_ms = if pressed { 100 } else { 500 };
        if led.is_set_high() {
            led.set_low();
        } else {
            led.set_high();
        }
        cortex_m::asm::delay(CPU_HZ / 1000 * half_period_ms);
    }
}
