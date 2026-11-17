//! The glue probe (plan step 5, `docs/design/glue-generator.md`): the die
//! axis's evidence that the generated facts hold on the silicon, through the
//! debugger alone, with the core reset and halted and no firmware involved.
//!
//! For one part on its board:
//!
//! - `DEV_ID` is the die's.
//! - Every peripheral the glue offers, and every GPIO port its package
//!   bonds: with its RCC enable bit clear, a write to a scratch register is
//!   ignored; with it set, the write sticks; pulsing its reset bit brings the
//!   register back to the value it had before the write. That ties the
//!   enable bit, the reset bit and the peripheral's address together.
//! - Every vector the offered peripherals and DMA streams raise: its NVIC
//!   pending bit sets and clears, so the number exists on the silicon.
//!
//! Not here, by design: which signal a pin's alternate function carries.
//! AFR accepts every value on every pin, so a read-back proves nothing; the
//! bench runs prove routes on the wire.

use std::collections::BTreeSet;
use std::time::Duration;

use probe_rs::{MemoryInterface, Permissions, probe::list::Lister};

use crate::emit;
use crate::registry::Part;
use crate::silicon::{Die, DiePeripheral};

/// A register each driven IP version has, which a halted core can write
/// with the peripheral disabled, and a value its fields can hold.
fn scratch(kind: &str, version: &str) -> Option<(&'static str, u32)> {
    Some(match (kind, version) {
        ("usart", _) => ("brr", 0x1234),
        ("spi", "v2_i2s") => ("crcpr", 0x55),
        // The same CRC polynomial register, writable while CRC is off
        // (RM0431 28.9.5).
        ("spi", "v3_i2s") => ("crcpr", 0x55),
        ("spi", "v4_i2s") => ("crcpoly", 0x55),
        ("i2c", "v1") => ("oar2", 0x54),
        ("i2c", "v2") => ("timingr", 0x1034_5678),
        ("timer", _) => ("arr", 0x1234),
        ("adc", "v2") => ("htr", 0x123),
        ("adc", "v4") => ("htr1", 0x123),
        ("sdmmc", _) => ("dtimer", 0x1234_5678),
        ("otg", _) => ("grxfsiz", 0x80),
        ("gpio", _) => ("otyper", 0x5),
        _ => return None,
    })
}

const NVIC_ISPR: u64 = 0xE000_E200;
const NVIC_ICPR: u64 = 0xE000_E280;

struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    fn check(&mut self, what: &str, ok: bool, detail: String) {
        if ok {
            self.passed += 1;
            println!("PASS {what}: {detail}");
        } else {
            self.failed += 1;
            println!("FAIL {what}: {detail}");
        }
    }
}

pub fn probe(die: &Die, part: &Part, selector: &str) -> Result<(u32, u32), String> {
    let lister = Lister::new();
    let info = lister
        .list_all()
        .into_iter()
        .find(|p| {
            let id = format!(
                "{:04x}:{:04x}:{}",
                p.vendor_id,
                p.product_id,
                p.serial_number.as_deref().unwrap_or("")
            );
            id.eq_ignore_ascii_case(selector)
        })
        .ok_or_else(|| format!("no probe {selector}; probe-rs list prints them after `--`"))?;
    let probe = info
        .open()
        .map_err(|e| format!("opening {selector}: {e}"))?;
    let chip = format!("{}Tx", part.feature.to_uppercase());
    let mut session = probe
        .attach(chip.as_str(), Permissions::default())
        .map_err(|e| format!("attaching to {chip}: {e}"))?;
    let mut core = session.core(0).map_err(|e| e.to_string())?;
    core.reset_and_halt(Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    let read = |core: &mut probe_rs::Core, address: u64| {
        core.read_word_32(address)
            .map_err(|e| format!("read {address:#x}: {e}"))
    };

    let mut report = Report {
        passed: 0,
        failed: 0,
    };
    let rcc = die
        .peripherals
        .get("RCC")
        .ok_or("no RCC in the die table")?
        .address;

    // The die.
    let dbgmcu = die.peripherals.get("DBGMCU").ok_or("no DBGMCU")?;
    let idcode = ["idcode", "idc"]
        .iter()
        .find_map(|r| dbgmcu.registers.get(*r))
        .ok_or("no DBGMCU IDCODE")?;
    let dev_id = read(&mut core, dbgmcu.address + u64::from(*idcode))? & 0xFFF;
    report.check(
        &format!("{} die", part.feature),
        u32::from(part.die) == dev_id,
        format!("DEV_ID {dev_id:#05x}, the table's {:#05x}", part.die),
    );

    // The peripherals the glue offers, and the bonded ports.
    let offer = emit::offer(die, part)?;
    let bonded = die.packages.get(part.package).ok_or("no package")?;
    let ports: BTreeSet<String> = bonded
        .iter()
        .filter_map(|pin| pin.get(1..2))
        .map(|port| format!("GPIO{port}"))
        .collect();
    let mut names: Vec<String> = Vec::new();
    for list in [
        &offer.uarts,
        &offer.spis,
        &offer.i2cs,
        &offer.timers,
        &offer.adcs,
        &offer.usb,
        &offer.sdmmcs,
    ] {
        names.extend(list.iter().cloned());
    }
    names.extend(ports);
    let mut vectors: BTreeSet<String> = BTreeSet::new();
    for name in &names {
        let p = die
            .peripherals
            .get(name)
            .ok_or_else(|| format!("{name} not in the die table"))?;
        peripheral(&mut core, &mut report, rcc, name, p)?;
        vectors.extend(
            p.interrupts
                .iter()
                .filter(|((device, _), _)| device == part.device)
                .map(|(_, v)| v.clone()),
        );
    }
    for dma in &offer.dmas {
        if let Some(p) = die.peripherals.get(dma) {
            vectors.extend(
                p.interrupts
                    .iter()
                    .filter(|((device, _), _)| device == part.device)
                    .map(|(_, v)| v.clone()),
            );
        }
    }

    // The vectors.
    for vector in &vectors {
        let n = *die
            .interrupts
            .get(vector)
            .ok_or_else(|| format!("vector {vector} not numbered"))?;
        let (word, bit) = (u64::from(n / 32) * 4, 1u32 << (n % 32));
        core.write_word_32(NVIC_ISPR + word, bit)
            .map_err(|e| e.to_string())?;
        let pending = read(&mut core, NVIC_ISPR + word)? & bit != 0;
        core.write_word_32(NVIC_ICPR + word, bit)
            .map_err(|e| e.to_string())?;
        let cleared = read(&mut core, NVIC_ISPR + word)? & bit == 0;
        report.check(
            &format!("vector {vector}"),
            pending && cleared,
            format!("IRQ {n} pends and clears"),
        );
    }
    Ok((report.passed, report.failed))
}

/// One peripheral's enable bit, scratch register and reset bit.
fn peripheral(
    core: &mut probe_rs::Core,
    report: &mut Report,
    rcc: u64,
    name: &str,
    p: &DiePeripheral,
) -> Result<(), String> {
    let (kind, version) = (
        p.kind.as_deref().unwrap_or(""),
        p.version.as_deref().unwrap_or(""),
    );
    let Some((register, pattern)) = scratch(kind, version) else {
        return Err(format!("{name}: no scratch register for {kind} {version}"));
    };
    let offset = *p
        .registers
        .get(register)
        .ok_or_else(|| format!("{name}: no register {register}"))?;
    let scratch_at = p.address + u64::from(offset);
    let rcc_field = |field: Option<&glue_model::RccField>| -> Option<(u64, u32, String)> {
        let f = field?;
        Some((
            rcc + u64::from(f.offset?),
            1 << f.bit?,
            format!("{}.{}", f.register, f.field),
        ))
    };
    let Some((enable, enable_bit, enable_name)) =
        rcc_field(p.rcc.as_ref().and_then(|r| r.enable.as_ref()))
    else {
        return Err(format!("{name}: no resolved enable bit"));
    };
    let modify = |core: &mut probe_rs::Core, at: u64, bit: u32, on: bool| -> Result<(), String> {
        let v = core.read_word_32(at).map_err(|e| e.to_string())?;
        core.write_word_32(at, if on { v | bit } else { v & !bit })
            .map_err(|e| e.to_string())?;
        let _ = core.read_word_32(at);
        Ok(())
    };
    let what = format!("{name} ({kind} {version})");

    modify(core, enable, enable_bit, false)?;
    let _ = core.write_word_32(scratch_at, pattern);
    let off = core.read_word_32(scratch_at).unwrap_or(0);
    modify(core, enable, enable_bit, true)?;
    let before = core.read_word_32(scratch_at).map_err(|e| e.to_string())?;
    core.write_word_32(scratch_at, pattern)
        .map_err(|e| e.to_string())?;
    let on = core.read_word_32(scratch_at).map_err(|e| e.to_string())?;
    report.check(
        &what,
        off != pattern && on == pattern && before != pattern,
        format!("{enable_name} clocks it: {register} ignores {pattern:#x} with it clear ({off:#x}), keeps it set ({on:#x})"),
    );
    if let Some((reset, reset_bit, reset_name)) =
        rcc_field(p.rcc.as_ref().and_then(|r| r.reset.as_ref()))
    {
        modify(core, reset, reset_bit, true)?;
        modify(core, reset, reset_bit, false)?;
        let after = core.read_word_32(scratch_at).map_err(|e| e.to_string())?;
        report.check(
            &what,
            after == before,
            format!("{reset_name} resets it: {register} back to {before:#x} ({after:#x})"),
        );
    }
    modify(core, enable, enable_bit, false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every IP version the glue offers on some series has a scratch register,
    /// so the probe never stops at an offered peripheral (it stopped at the
    /// F767ZI's SPI1, spi v3_i2s, on 2026-11-17).
    #[test]
    fn every_driven_version_has_a_scratch_register() {
        let versions = [
            ("usart", "v2"),
            ("usart", "v4"),
            ("spi", "v2_i2s"),
            ("spi", "v3_i2s"),
            ("spi", "v4_i2s"),
            ("i2c", "v1"),
            ("i2c", "v2"),
            ("timer", "v1"),
            ("timer", "v3"),
            ("adc", "v2"),
            ("adc", "v4"),
            ("sdmmc", "v2"),
            ("otg", "v1"),
        ];
        for series in ["F4", "F7", "H7"] {
            for (kind, version) in versions {
                if crate::emit::drives(series, kind, version) {
                    assert!(
                        scratch(kind, version).is_some(),
                        "{series} {kind} {version}"
                    );
                }
            }
        }
    }
}
