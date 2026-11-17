//! I2C: scans I2C1 for devices once a second, logging every address that
//! answers its read with an ACK.
//!
//! - SCL PB8, CN5-10 (D15)
//! - SDA PB9, CN5-9 (D14)
//!
//! Wiring: an I2C device on D15 and D14, with GND and 3V3 (UM1724, Arduino
//! connectors). A breakout board usually carries its own pull-ups; the
//! pins are also pulled up here, which is enough for a short bus at 100 kHz.
//! With nothing fitted, every address is answered by a NACK and the scan
//! reports none.
//!
//! `I2c` is an `embedded-hal` `I2c` bus, so device driver crates take it
//! as it is. Every wait is bounded: a stuck bus is an error, never a hang.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use embedded_hal::i2c::{Error as _, ErrorKind, I2c as _};
use ferrohal_stm32::{
    chip::Peripherals,
    gpio::{Alternate, OpenDrain, Pin, Pull},
    i2c::{Config, I2c, Wiring},
    rcc,
    time::Hertz,
};
use panic_probe as _;

const CPU_HZ: u32 = 16_000_000;

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    let mut rcc = match p.RCC.freeze(&rcc::Config::hsi()) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };
    let mut gpiob = p.GPIOB.split(&mut rcc);

    // Alternate function 4, open drain: I2C1 on these pins (DS10086).
    let mut scl: Pin<'B', 8, Alternate<4, OpenDrain>> =
        gpiob.p8.into_alternate_open_drain(&mut gpiob.ctrl);
    scl.set_pull(&mut gpiob.ctrl, Pull::Up);
    let mut sda: Pin<'B', 9, Alternate<4, OpenDrain>> =
        gpiob.p9.into_alternate_open_drain(&mut gpiob.ctrl);
    sda.set_pull(&mut gpiob.ctrl, Pull::Up);
    let wiring = Wiring {
        i2c: p.I2C1,
        scl,
        sda,
    };

    let mut bus = match I2c::new(&mut rcc, wiring, &Config::new(Hertz::khz(100))) {
        Ok(bus) => bus,
        Err(refused) => defmt::panic!("I2C1: {}", refused.error()),
    };
    defmt::info!("i2c-scan: SCL at {} Hz", bus.frequency().0);

    loop {
        let mut found = 0;
        // 0x08 to 0x77: the addresses outside the reserved ones.
        for address in 0x08..=0x77u8 {
            let mut byte = [0u8; 1];
            match bus.read(address, &mut byte) {
                Ok(()) => {
                    defmt::info!("device at {=u8:#04x}", address);
                    found += 1;
                }
                // Nobody home: the bus is put back, and the scan goes on.
                Err(error) if matches!(error.kind(), ErrorKind::NoAcknowledge(_)) => {}
                Err(error) => defmt::warn!("{=u8:#04x}: {}", address, error),
            }
        }
        defmt::info!("scan done: {} device(s)", found);
        cortex_m::asm::delay(CPU_HZ);
    }
}
