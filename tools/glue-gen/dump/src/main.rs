//! Prints one part's metadata, the part the build's feature selects, as JSON
//! on stdout. Run by the generator; see `../src/main.rs`.

use glue_model::{
    DmaChannel, Interrupt, KernelClock, Part, Peripheral, PeripheralDma, PeripheralInterrupt,
    PeripheralPin, Rcc, RccField, Region,
};
use stm32_metapac::metadata::{
    METADATA, MemoryRegionKind, PeripheralRccKernelClock, PeripheralRccRegister,
    ir::{BitOffset, BlockItemInner, IR},
};

fn main() {
    let m = &METADATA;
    // RCC fields resolve through the RCC block's own register definitions.
    let rcc = m
        .peripherals
        .iter()
        .find(|p| p.name == "RCC")
        .and_then(|p| p.registers.as_ref());
    let field = |reg: &PeripheralRccRegister| RccField {
        register: reg.register.to_string(),
        field: reg.field.to_string(),
        bit: rcc.and_then(|r| bit_of(r.ir, r.block, reg.register, reg.field)),
    };

    let part = Part {
        name: m.name.to_string(),
        family: m.family.to_string(),
        line: m.line.to_string(),
        nvic_priority_bits: m.nvic_priority_bits,
        memory: m
            .memory
            .iter()
            .flat_map(|bank| bank.iter())
            .map(|r| Region {
                name: r.name.to_string(),
                kind: match r.kind {
                    MemoryRegionKind::Flash => "flash",
                    MemoryRegionKind::Ram => "ram",
                    MemoryRegionKind::Eeprom => "eeprom",
                }
                .to_string(),
                address: r.address,
                size: r.size,
            })
            .collect(),
        interrupts: m
            .interrupts
            .iter()
            .map(|i| Interrupt {
                name: i.name.to_string(),
                number: i.number,
            })
            .collect(),
        dma_channels: m
            .dma_channels
            .iter()
            .map(|c| DmaChannel {
                name: c.name.to_string(),
                dma: c.dma.to_string(),
                channel: c.channel,
                dmamux: c.dmamux.map(str::to_string),
                dmamux_channel: c.dmamux_channel,
            })
            .collect(),
        pins: m.pins.iter().map(|p| p.name.to_string()).collect(),
        peripherals: m
            .peripherals
            .iter()
            .map(|p| Peripheral {
                name: p.name.to_string(),
                address: p.address,
                kind: p.registers.as_ref().map(|r| r.kind.to_string()),
                version: p.registers.as_ref().map(|r| r.version.to_string()),
                block: p.registers.as_ref().map(|r| r.block.to_string()),
                rcc: p.rcc.as_ref().map(|r| Rcc {
                    bus_clock: r.bus_clock.to_string(),
                    kernel_clock: match &r.kernel_clock {
                        PeripheralRccKernelClock::Clock(name) => {
                            KernelClock::Clock(name.to_string())
                        }
                        PeripheralRccKernelClock::Mux(reg) => KernelClock::Mux(field(reg)),
                    },
                    enable: r.enable.as_ref().map(field),
                    reset: r.reset.as_ref().map(field),
                }),
                pins: p
                    .pins
                    .iter()
                    .map(|pin| PeripheralPin {
                        pin: pin.pin.to_string(),
                        signal: pin.signal.to_string(),
                        af: pin.af,
                    })
                    .collect(),
                dma: p
                    .dma_channels
                    .iter()
                    .map(|d| PeripheralDma {
                        signal: d.signal.to_string(),
                        channel: d.channel.map(str::to_string),
                        dmamux: d.dmamux.map(str::to_string),
                        dma: d.dma.map(str::to_string),
                        request: d.request,
                    })
                    .collect(),
                interrupts: p
                    .interrupts
                    .iter()
                    .map(|i| PeripheralInterrupt {
                        signal: i.signal.to_string(),
                        interrupt: i.interrupt.to_string(),
                    })
                    .collect(),
            })
            .collect(),
    };
    match serde_json::to_string(&part) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("glue-dump: {error}");
            std::process::exit(1);
        }
    }
}

/// The bit position of `field` in `register` of `block`, through the
/// block's register and field-set definitions (case-insensitive, as
/// metapac's metadata and IR spell names differently).
fn bit_of(ir: &IR, block: &str, register: &str, field: &str) -> Option<u32> {
    let block = ir
        .blocks
        .iter()
        .find(|b| b.name.eq_ignore_ascii_case(block))?;
    let item = block
        .items
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(register))?;
    let BlockItemInner::Register(reg) = &item.inner else {
        return None;
    };
    let fieldset = ir.fieldsets.iter().find(|f| Some(f.name) == reg.fieldset)?;
    let found = fieldset
        .fields
        .iter()
        .find(|f| f.name.eq_ignore_ascii_case(field))?;
    match &found.bit_offset {
        BitOffset::Regular(offset) => Some(offset.offset),
        BitOffset::Cursed(_) => None,
    }
}
