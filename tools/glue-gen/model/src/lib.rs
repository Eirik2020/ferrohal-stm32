//! One part's metadata as the glue generator sees it: the subset of
//! stm32-metapac's `metadata::Metadata` the chip glue is made from, with every
//! RCC enable, reset and kernel-clock mux field resolved to its bit position
//! through metapac's own register definitions.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Part {
    /// metapac's name, `STM32H753ZI`.
    pub name: String,
    pub family: String,
    pub line: String,
    pub nvic_priority_bits: Option<u8>,
    pub memory: Vec<Region>,
    pub interrupts: Vec<Interrupt>,
    pub dma_channels: Vec<DmaChannel>,
    /// The bonded pins, `PA0` and so on: the package's.
    pub pins: Vec<String>,
    pub peripherals: Vec<Peripheral>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Region {
    pub name: String,
    /// `flash`, `ram` or `eeprom`.
    pub kind: String,
    pub address: u32,
    pub size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interrupt {
    pub name: String,
    pub number: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DmaChannel {
    pub name: String,
    pub dma: String,
    pub channel: u32,
    pub dmamux: Option<String>,
    pub dmamux_channel: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Peripheral {
    pub name: String,
    pub address: u64,
    /// The IP: kind and version (`usart`, `v4`), and the register block.
    pub kind: Option<String>,
    pub version: Option<String>,
    pub block: Option<String>,
    pub rcc: Option<Rcc>,
    pub pins: Vec<PeripheralPin>,
    pub dma: Vec<PeripheralDma>,
    pub interrupts: Vec<PeripheralInterrupt>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rcc {
    pub bus_clock: String,
    pub kernel_clock: KernelClock,
    pub enable: Option<RccField>,
    pub reset: Option<RccField>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum KernelClock {
    /// A fixed clock, by metapac's name for it.
    Clock(String),
    /// A selector field.
    Mux(RccField),
}

/// An RCC register field, and its bit position when the dumper could resolve
/// it through metapac's register definitions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RccField {
    pub register: String,
    pub field: String,
    pub bit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeripheralPin {
    pub pin: String,
    pub signal: String,
    pub af: Option<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeripheralDma {
    pub signal: String,
    pub channel: Option<String>,
    pub dmamux: Option<String>,
    pub dma: Option<String>,
    pub request: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeripheralInterrupt {
    pub signal: String,
    pub interrupt: String,
}
