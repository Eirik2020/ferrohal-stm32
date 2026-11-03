//! The ADC on `adc_v4` (H7): register operations for the `adc` module.
//!
//! Runs from per_ck (HSI, 64 MHz, as reset leaves CKPERSEL) over 16: 4 MHz, in
//! the asynchronous clock mode, independent of the core clock. Single-ended,
//! 16 bits, with linearity calibration at startup. ADC1 and ADC2 share their
//! clock and common registers; only ADC1 and ADC3 are offered.
//!
//! Requirements: ADC-001

use stm32_metapac::{
    adc::{
        Adc,
        regs::Isr,
        vals::{Adcaldif, Adstp, Dmngt, Ovrmod, Pcsel, Res, SampleTime},
    },
    adccommon::{
        AdcCommon,
        vals::{Ckmode, Presc},
    },
};

use crate::{
    adc::Error,
    rcc::{Bus, Clocks, Rcc},
};

pub const MAX_SAMPLE: u16 = 65535;
pub const SLOW_SOURCE_CYCLES: u16 = 65;
/// Polls of the status register for one conversion: 810.5 + 8.5 cycles at 4 MHz
/// is 205 us, or 82 000 CPU clocks at 400 MHz.
const CONVERSION_SPINS: u32 = 200_000;
/// Polls for ADRDY.
const READY_SPINS: u32 = 1_000_000;
const STOP_SPINS: u32 = 100_000;
/// Calibration's bound, in milliseconds.
const CALIBRATION_MS: u32 = 1000;

/// Selects per_ck for the ADC's kernel clock.
pub(crate) fn start_clock(rcc: &mut Rcc) -> Result<(), Error> {
    if rcc.adc_clock() {
        Ok(())
    } else {
        Err(Error::KernelClock)
    }
}

const fn sample_time(cycles: u16) -> SampleTime {
    match cycles {
        0..=1 => SampleTime::CYCLES1_5,
        2 => SampleTime::CYCLES2_5,
        3..=8 => SampleTime::CYCLES8_5,
        9..=16 => SampleTime::CYCLES16_5,
        17..=32 => SampleTime::CYCLES32_5,
        33..=65 => SampleTime::CYCLES64_5,
        66..=387 => SampleTime::CYCLES387_5,
        _ => SampleTime::CYCLES810_5,
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
        _bus: Bus,
        internal: bool,
        sample_cycles: u16,
    ) -> Result<(), Error> {
        let cycles_per_us = clocks.cpu.0 / 1_000_000;
        self.common.ccr().modify(|w| {
            w.set_ckmode(Ckmode::ASYNCHRONOUS);
            w.set_presc(Presc::DIV16);
            w.set_vrefen(internal);
            w.set_vsenseen(internal);
        });
        // Out of deep power-down, regulator on: tADCVREG_STUP is 10 us.
        self.regs.cr().modify(|w| w.set_deeppwd(false));
        self.regs.cr().modify(|w| w.set_advregen(true));
        cortex_m::asm::delay(cycles_per_us * 20);

        self.regs.cfgr().modify(|w| {
            w.set_res(Res::BITS16);
            w.set_ovrmod(Ovrmod::OVERWRITE);
            w.set_cont(false);
            w.set_dmngt(Dmngt::DR);
        });

        self.regs.cr().modify(|w| {
            w.set_adcaldif(Adcaldif::SINGLE_ENDED);
            w.set_adcallin(true);
        });
        self.regs.cr().modify(|w| w.set_adcal(true));
        let mut calibrated = false;
        for _ in 0..CALIBRATION_MS {
            if !self.regs.cr().read().adcal() {
                calibrated = true;
                break;
            }
            cortex_m::asm::delay(cycles_per_us * 1000);
        }
        if !calibrated {
            return Err(Error::Timeout);
        }

        self.regs.isr().write_value({
            let mut isr = Isr(0);
            isr.set_adrdy(true);
            isr
        });
        self.regs.cr().modify(|w| w.set_aden(true));
        let mut ready = false;
        for _ in 0..READY_SPINS {
            if self.regs.isr().read().adrdy() {
                ready = true;
                break;
            }
        }
        if !ready {
            return Err(Error::Timeout);
        }

        let smp = sample_time(sample_cycles);
        self.regs.smpr(0).modify(|w| {
            for channel in 0..10 {
                w.set_smp(channel, smp);
            }
        });
        self.regs.smpr(1).modify(|w| {
            for channel in 0..10 {
                w.set_smp(channel, smp);
            }
        });
        // The internal channels' tSTART (at most 15 us) has passed in the
        // ready wait or will have in calibration's; give them this too.
        cortex_m::asm::delay(cycles_per_us * 20);
        Ok(())
    }

    /// One conversion of `channel`, sequence length 1.
    pub(crate) fn read(&self, channel: u8) -> Result<u16, Error> {
        self.set_sequence(&[channel]);
        self.regs.cfgr().modify(|w| w.set_dmngt(Dmngt::DR));
        self.clear_flags();
        self.regs.cr().modify(|w| w.set_adstart(true));
        for _ in 0..CONVERSION_SPINS {
            if self.regs.isr().read().eoc() {
                return Ok(self.regs.dr().read().rdata());
            }
        }
        self.stop();
        Err(Error::Timeout)
    }

    /// Sets the sequence's channels, in order, and preselects them. At most
    /// 16: the caller checks.
    pub(crate) fn set_sequence(&self, channels: &[u8]) {
        let mut selected = stm32_metapac::adc::regs::Pcsel(0);
        for (position, &channel) in channels.iter().enumerate() {
            match position {
                0..=3 => self.regs.sqr1().modify(|w| w.set_sq(position, channel)),
                4..=8 => self.regs.sqr2().modify(|w| w.set_sq(position - 4, channel)),
                9..=13 => self.regs.sqr3().modify(|w| w.set_sq(position - 9, channel)),
                14..=15 => self.regs.sqr4().modify(|w| w.set_sq(position - 14, channel)),
                _ => {}
            }
            if channel < 20 {
                selected.set_pcsel(usize::from(channel), Pcsel::PRESELECTED);
            }
        }
        self.regs.pcsel().write_value(selected);
        let last = channels.len().saturating_sub(1) as u8;
        self.regs.sqr1().modify(|w| w.set_l(last));
    }

    pub(crate) fn data_address(&self) -> u32 {
        self.regs.dr().as_ptr() as u32
    }

    fn clear_flags(&self) {
        self.regs.isr().write_value(Isr(0x7FF));
    }

    fn stop(&self) {
        if self.regs.cr().read().adstart() {
            self.regs.cr().modify(|w| w.set_adstp(Adstp::STOP));
            for _ in 0..STOP_SPINS {
                if !self.regs.cr().read().adstart() {
                    break;
                }
            }
        }
    }

    /// Starts the sequence, with its DMA requests in one-shot mode.
    pub(crate) fn start_sequence(&self) {
        self.clear_flags();
        self.regs.cfgr().modify(|w| {
            w.set_dmngt(Dmngt::DMA_ONE_SHOT);
            w.set_ovrmod(Ovrmod::PRESERVE);
        });
        self.regs.cr().modify(|w| w.set_adstart(true));
    }

    /// Ends a sequence's DMA requests; returns whether a conversion overran.
    pub(crate) fn finish_sequence(&self) -> bool {
        let overrun = self.regs.isr().read().ovr();
        self.stop();
        self.regs.cfgr().modify(|w| {
            w.set_dmngt(Dmngt::DR);
            w.set_ovrmod(Ovrmod::OVERWRITE);
        });
        self.clear_flags();
        overrun
    }
}
