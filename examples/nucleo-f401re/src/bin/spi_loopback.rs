//! SPI: a polled SPI2 master, its MOSI wired to its own MISO, so every byte
//! sent comes back. The same `embedded-hal` `SpiBus` a sensor driver takes.
//!
//! - SCK  PB13, CN10-30
//! - MISO PB14, CN10-28
//! - MOSI PB15, CN10-26
//!
//! Wiring: one jumper from CN10-26 to CN10-28 (UM1724, morpho connectors).
//! Without it MISO reads the pull-down, and the check reports zeros.
//!
//! `BlockingSpi` needs no DMA channel and no interrupt. `spi::Spi` is the
//! DMA driver, for a bus whose transfers should not hold the CPU.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use embedded_hal::spi::SpiBus;
use ferrohal_stm32::{
    chip::Peripherals,
    gpio::{Alternate, Pin, Pull, Speed},
    rcc::Config,
    spi::{self, BlockingSpi, BlockingWiring},
    time::Hertz,
};
use panic_probe as _;

const CPU_HZ: u32 = 16_000_000;

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    let mut rcc = match p.RCC.freeze(&Config::hsi()) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };
    let mut gpiob = p.GPIOB.split(&mut rcc);

    // Alternate function 5 is SPI2 on these pins (DS10086). The driver's
    // bounds check each pin against SPI2: a pin or function that is not
    // SPI2's does not build.
    let mut sck: Pin<'B', 13, Alternate<5>> = gpiob.p13.into_alternate(&mut gpiob.ctrl);
    sck.set_speed(&mut gpiob.ctrl, Speed::High);
    let mut miso: Pin<'B', 14, Alternate<5>> = gpiob.p14.into_alternate(&mut gpiob.ctrl);
    miso.set_pull(&mut gpiob.ctrl, Pull::Down);
    let mut mosi: Pin<'B', 15, Alternate<5>> = gpiob.p15.into_alternate(&mut gpiob.ctrl);
    mosi.set_speed(&mut gpiob.ctrl, Speed::High);
    let wiring = BlockingWiring {
        spi: p.SPI2,
        sck,
        miso,
        mosi,
    };

    // 1 MHz at most, mode 0, MSB first; the driver picks the nearest
    // divider at or below it.
    let mut bus = match BlockingSpi::new(&mut rcc, wiring, &spi::Config::new(Hertz::mhz(1))) {
        Ok(bus) => bus,
        Err(refused) => defmt::panic!("SPI2: {}", refused.error()),
    };
    defmt::info!("spi-loopback: SCK at {} Hz", bus.frequency().0);

    let mut round: u8 = 0;
    loop {
        let sent = [round, round.wrapping_add(1), 0xA5, 0x5A];
        let mut received = [0u8; 4];
        match bus.transfer(&mut received, &sent) {
            Ok(()) if received == sent => defmt::info!("sent {=[u8]:02x}, got it back", sent),
            Ok(()) => defmt::warn!(
                "sent {=[u8]:02x}, got {=[u8]:02x}: is MOSI jumpered to MISO?",
                sent,
                received
            ),
            Err(error) => defmt::warn!("SPI2: {}", error),
        }
        round = round.wrapping_add(1);
        cortex_m::asm::delay(CPU_HZ);
    }
}
