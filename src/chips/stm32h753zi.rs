//! STM32H753ZI glue: the chip facts the IP modules leave out.
//!
//! The routing facts (RCC bits, vectors, pin alternate functions, DMAMUX
//! request IDs) are generated from stm32-metapac's metadata into
//! `gen/stm32h753zi.rs` by `tools/glue-gen`. Clock limits and the memory map, shared with the H743,
//! are in `shared/h7.rs`.
//!
//! Requirements: GLUE-001, GLUE-002

use stm32_metapac as pac;

/// The chip's interrupt vectors, as numbers: what a UART reports it needs
/// bound. Data only; FerroHAL never writes the NVIC with them.
pub use stm32_metapac::Interrupt;

use crate::{
    dma_v1::{Dma, DmaInstance, Stream},
    gpio_v2::{Alternate, Pin, Port, PortInstance},
    i2c::{SclPin, SdaPin},
    rcc_h7_rm0433::{Bus, RCC, RccBit},
    sdmmc::{CkPin, CmdPin, D0Pin, D1Pin, D2Pin, D3Pin},
    spi::{MisoPin, MosiPin, SckPin, SpiRxDma, SpiTxDma},
    uart::{Info, RxDma, RxPin, TxDma, TxPin},
    usart_v4::LineRegs,
};

#[path = "shared/macros.rs"]
#[macro_use]
mod macros;

#[path = "shared/h7_macros.rs"]
#[macro_use]
mod family;

/// Which ADC carries the temperature sensor and VREFINT channels.
macro_rules! internal_adc {
    (ADC3) => {
        true
    };
    ($other:ident) => {
        false
    };
}

/// The full-speed USB port's core (high-speed capable, on its internal full-speed PHY), its FIFO RAM in words and
/// its endpoints: facts metapac does not carry.
#[cfg(feature = "usb")]
pub(crate) const USB_FS_HS_CORE: bool = true;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_FIFO_WORDS: usize = 1024;
#[cfg(feature = "usb")]
pub(crate) const USB_FS_ENDPOINTS: usize = 9;

// The routing rows, generated from stm32-metapac's metadata.
#[path = "gen/stm32h753zi.rs"]
mod generated;
pub use generated::*;
#[path = "shared/h7.rs"]
mod h7;

pub use h7::check_dma_buffer;
pub(crate) use h7::{
    CLOCK_LIMITS, SYSTEM_MEMORY, TS_CAL1, TS_CAL2, UID_BASE, VREFIN_CAL, map_system_memory,
};

// Timers another timer's trigger output can start.

timer_trigger!(TIM8: TIM1, 0);
timer_trigger!(TIM3: TIM1, 0);

adc_internal!(ADC3: 18, 19);

/// The independent watchdog's token and registers.
pub type WatchdogPeripheral = IWDG1;

pub(crate) fn iwdg_regs() -> pac::iwdg::Iwdg {
    pac::IWDG1
}

/// Freezes the watchdog while a debugger holds the core halted:
/// `DBGMCU_APB4FZR1.IWDG1` (bit 18; IWDG1 sits on APB4, not APB1).
pub(crate) fn set_watchdog_freeze() {
    pac::DBGMCU.apb4fzr1().modify(|w| w.set_iwdg1(true));
}

/// Whether that freeze is set.
pub(crate) fn watchdog_frozen() -> bool {
    pac::DBGMCU.apb4fzr1().read().iwdg1()
}

/// SYSCFG's clock enable, which EXTI's port selection (EXTICR) needs.
pub(crate) const SYSCFG_ENABLE: RccBit = RccBit::new(Bus::Apb4, 1);

// Every DMA1 and DMA2 stream reaches the GPIO ports (RM0433 figure 1).
gpio_dma!(
    1: [0, 1, 2, 3, 4, 5, 6, 7];
    2: [0, 1, 2, 3, 4, 5, 6, 7];
);

/// The die revision code, `DBGMCU_IDC.REV_ID` (bits 31:16) on RM0433.
pub(crate) fn revision() -> u16 {
    pac::DBGMCU.idc().read().rev_id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uart::{Resource, Signal, Wiring};

    /// The TBS Lucid H7's receiver port, as the bench wires it.
    type Usart6Lucid = Wiring<
        USART6,
        Pin<'C', 6, Alternate<7>>,
        Pin<'C', 7, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 0>,
    >;

    #[test]
    fn wiring_describes_itself() {
        assert_eq!(
            Usart6Lucid::RESOURCES,
            [
                Resource::Peripheral("USART6"),
                Resource::Pin {
                    signal: Signal::Tx,
                    port: 'C',
                    pin: 6,
                    af: 7
                },
                Resource::Pin {
                    signal: Signal::Rx,
                    port: 'C',
                    pin: 7,
                    af: 7
                },
                // The Lucid's manifest names these DMA1_STREAM6_CH72 and
                // DMA1_STREAM0_CH71.
                Resource::Dma {
                    signal: Signal::Tx,
                    controller: 1,
                    stream: 6,
                    request: 72
                },
                Resource::Dma {
                    signal: Signal::Rx,
                    controller: 1,
                    stream: 0,
                    request: 71
                },
                Resource::Interrupt("USART6"),
                Resource::Interrupt("DMA1_STREAM0"),
                Resource::Interrupt("DMA1_STREAM6"),
            ]
        );
    }
}
