//! Prints one part's metadata, the part the build's feature selects, as JSON
//! on stdout. Run by the generator; see `../src/main.rs`.

use glue_model::{
    DmaChannel, Interrupt, KernelClock, Part, Peripheral, PeripheralDma, PeripheralInterrupt,
    PeripheralPin, Rcc, RccField, Region, Register,
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
        offset: rcc.and_then(|r| offset_of(r.ir, r.block, reg.register)),
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
                registers: p
                    .registers
                    .as_ref()
                    .map(|r| registers_of(r.ir, r.block))
                    .unwrap_or_default(),
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
    let block = ir.blocks.iter().find(|b| same_block(b.name, block))?;
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

fn offset_of(ir: &IR, block: &str, register: &str) -> Option<u32> {
    let block = ir.blocks.iter().find(|b| same_block(b.name, block))?;
    let item = block
        .items
        .iter()
        .find(|i| i.name.eq_ignore_ascii_case(register))?;
    matches!(item.inner, BlockItemInner::Register(_)).then_some(item.byte_offset)
}

/// A block's plain registers, by name and byte offset, with those of the
/// blocks it extends (metapac builds the timers that way); arrays are left out.
fn registers_of(ir: &IR, block: &str) -> Vec<Register> {
    let mut out: Vec<Register> = Vec::new();
    let mut next = Some(block);
    while let Some(name) = next {
        let Some(found) = ir.blocks.iter().find(|b| same_block(b.name, name)) else {
            break;
        };
        for item in found.items {
            if item.array.is_none()
                && matches!(item.inner, BlockItemInner::Register(_))
                && !out.iter().any(|r| r.name == item.name)
            {
                out.push(Register {
                    name: item.name.to_string(),
                    offset: item.byte_offset,
                });
            }
        }
        next = found.extends;
    }
    out
}

/// A block's name as a peripheral gives it (`TIM_GP16`) and as the register
/// set does (`TimGp16`).
fn same_block(a: &str, b: &str) -> bool {
    let plain = |s: &str| {
        s.chars()
            .filter(|c| *c != '_')
            .map(|c| c.to_ascii_lowercase())
            .collect::<String>()
    };
    plain(a) == plain(b)
}
