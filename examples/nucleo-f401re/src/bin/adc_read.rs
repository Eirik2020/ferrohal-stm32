//! ADC: reads the voltage on A0 (PA0, ADC1 channel 0) twice a second, with
//! the supply measured from the internal reference and the die temperature
//! from the internal sensor.
//!
//! Wiring: anything between 0 V and 3.3 V on CN8-1 (A0), such as a
//! potentiometer's wiper between GND and 3V3 (UM1724, Arduino connectors).
//! Left open, A0 floats and reads whatever it picks up.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    adc::{Adc, Config},
    chip::Peripherals,
    rcc,
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
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let a0 = gpioa.p0.into_analog(&mut gpioa.ctrl);

    // A long sample time: kind to a high-impedance source, and what the
    // internal channels need.
    let mut adc = match Adc::new(&mut rcc, p.ADC1, &Config::slow_source()) {
        Ok(adc) => adc,
        Err(refused) => defmt::panic!("ADC1: {}", refused.error()),
    };
    // The pin proves the channel: only a pin ADC1 can read gives one.
    let channel = adc.channel(&a0);

    loop {
        // Each read blocks for one conversion, bounded; none of them panics.
        let reading = adc.vdda_millivolts().and_then(|vdda| {
            let sample = adc.read(channel)?;
            let temperature = adc.temperature_decidegrees()?;
            Ok((vdda, sample, temperature))
        });
        match reading {
            Ok((vdda, sample, temperature)) => defmt::info!(
                "A0: {} of {} = {} mV (VDDA {} mV), die at {} tenths of a degree C",
                sample,
                adc.max_sample(),
                adc.to_millivolts(sample, vdda),
                vdda,
                temperature
            ),
            Err(error) => defmt::warn!("ADC: {}", error),
        }
        cortex_m::asm::delay(CPU_HZ / 2);
    }
}
