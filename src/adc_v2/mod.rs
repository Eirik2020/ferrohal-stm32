//! The ADC on `adc_v2` (F4): register operations for the `adc` module.
//!
//! The ADC clock is PCLK2 over 2, 4, 6 or 8, at most 36 MHz (DS8626, DS10086).
//! Single conversions and sequences share the sequence registers, so a `read`
//! between sequences isn't offered once an `AdcScan` holds the ADC.

use stm32_metapac::{
    adc::{
        Adc,
        regs::Sr,
        vals::{Align, Dds, Eocs, Res, SampleTime},
    },
    adccommon::{AdcCommon, vals::Adcpre},
};

use crate::{
    adc::Error,
    rcc::{Bus, Clocks, Rcc},
};

pub const MAX_SAMPLE: u16 = 4095;
pub const SLOW_SOURCE_CYCLES: u16 = 480;
const MAX_ADC_CLOCK_HZ: u32 = 36_000_000;
/// Polls of the status register for one conversion. The slowest clock tree
/// gives 492 ADC clocks of 128 CPU clocks each, and a poll takes at least one.
const CONVERSION_SPINS: u32 = 100_000;

/// Nothing to start: the ADC runs from its bus.
pub(crate) fn start_clock(_rcc: &mut Rcc) -> Result<(), Error> {
    Ok(())
}

const fn sample_time(cycles: u16) -> SampleTime {
    match cycles {
        0..=3 => SampleTime::CYCLES3,
        4..=15 => SampleTime::CYCLES15,
        16..=28 => SampleTime::CYCLES28,
        29..=56 => SampleTime::CYCLES56,
        57..=84 => SampleTime::CYCLES84,
        85..=112 => SampleTime::CYCLES112,
        113..=144 => SampleTime::CYCLES144,
        _ => SampleTime::CYCLES480,
    }
}

/// One ADC and the registers its instances share.
#[derive(Clone, Copy)]
pub struct Unit {
    regs: Adc,
    common: AdcCommon,
}

impl Unit {
    pub const fn new(regs: Adc, common: AdcCommon) -> Self {
        Self { regs, common }
    }

    pub(crate) fn power_up(
        &self,
        clocks: &Clocks,
        bus: Bus,
        internal: bool,
        sample_cycles: u16,
    ) -> Result<(), Error> {
        let pclk = clocks.bus(bus).0;
        let divider = if pclk / 2 <= MAX_ADC_CLOCK_HZ {
            Adcpre::DIV2
        } else if pclk / 4 <= MAX_ADC_CLOCK_HZ {
            Adcpre::DIV4
        } else if pclk / 6 <= MAX_ADC_CLOCK_HZ {
            Adcpre::DIV6
        } else {
            Adcpre::DIV8
        };
        self.common.ccr().modify(|w| {
            w.set_adcpre(divider);
            w.set_tsvrefe(internal);
        });
        let smp = sample_time(sample_cycles);
        self.regs.smpr1().modify(|w| {
            for channel in 0..9 {
                w.set_smp(channel, smp);
            }
        });
        self.regs.smpr2().modify(|w| {
            for channel in 0..10 {
                w.set_smp(channel, smp);
            }
        });
        self.regs.cr1().modify(|w| {
            w.set_res(Res::BITS12);
            w.set_scan(true);
        });
        self.regs.cr2().modify(|w| {
            w.set_align(Align::RIGHT);
            w.set_eocs(Eocs::EACH_CONVERSION);
            w.set_adon(true);
        });
        // tSTAB and the internal channels' tSTART: at most 3 and 10 us.
        cortex_m::asm::delay(clocks.hclk.0 / 1_000_000 * 15);
        Ok(())
    }

    /// One conversion of `channel`, sequence length 1.
    pub(crate) fn read(&self, channel: u8) -> Result<u16, Error> {
        self.set_sequence(&[channel]);
        self.regs.sr().write_value(Sr(0));
        self.regs.cr2().modify(|w| w.set_swstart(true));
        for _ in 0..CONVERSION_SPINS {
            let sr = self.regs.sr().read();
            if sr.ovr() {
                self.regs.sr().write_value(Sr(0));
                return Err(Error::Overrun);
            }
            if sr.eoc() {
                return Ok(self.regs.dr().read().data());
            }
        }
        Err(Error::Timeout)
    }

    /// Sets the sequence's channels, in order. At most 16: the caller checks.
    pub(crate) fn set_sequence(&self, channels: &[u8]) {
        for (position, &channel) in channels.iter().enumerate() {
            match position {
                0..=5 => self.regs.sqr3().modify(|w| w.set_sq(position, channel)),
                6..=11 => self.regs.sqr2().modify(|w| w.set_sq(position - 6, channel)),
                12..=15 => self.regs.sqr1().modify(|w| w.set_sq(position - 12, channel)),
                _ => {}
            }
        }
        let last = channels.len().saturating_sub(1) as u8;
        self.regs.sqr1().modify(|w| w.set_l(last));
    }

    pub(crate) fn data_address(&self) -> u32 {
        self.regs.dr().as_ptr() as u32
    }

    /// Starts the sequence, with its DMA requests. DMA requests stop once the
    /// stream's count ends (DDS = 0), so each start turns them on afresh
    /// (RM0090 13.8.1).
    pub(crate) fn start_sequence(&self) {
        self.regs.sr().write_value(Sr(0));
        self.regs.cr2().modify(|w| {
            w.set_dma(false);
            w.set_dds(Dds::SINGLE);
        });
        self.regs.cr2().modify(|w| w.set_dma(true));
        self.regs.cr2().modify(|w| w.set_swstart(true));
    }

    /// Ends a sequence's DMA requests; returns whether a conversion overran.
    pub(crate) fn finish_sequence(&self) -> bool {
        let overrun = self.regs.sr().read().ovr();
        self.regs.cr2().modify(|w| w.set_dma(false));
        self.regs.sr().write_value(Sr(0));
        overrun
    }
}
