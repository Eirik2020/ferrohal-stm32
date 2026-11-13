//! The silicon model (`docs/design/glue-generator.md`): a die's table is the
//! union of every registered part cut from it, and the parts must agree on
//! everything the die fixes. A package is the set of pins a part bonds; a
//! device (product line) is the set of the die's peripherals a part enables.
//!
//! What the die fixes, and so what two parts may not disagree on: each
//! peripheral's address, IP (kind, version, register block), RCC fields, its
//! interrupt per signal, a pin's alternate function for a signal, a DMA
//! route's request; and the chip's vector numbers and DMA channels. What a part
//! may add or leave out: whole peripherals (its device), pins (its package),
//! and the rows that name pins its package does not bond.

use std::collections::{BTreeMap, BTreeSet};

use glue_model::{DmaChannel, Part as Metadata, PeripheralDma, Rcc};

use crate::registry::Part;

/// One peripheral of a die, merged over the die's parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiePeripheral {
    pub address: u64,
    pub kind: Option<String>,
    pub version: Option<String>,
    pub block: Option<String>,
    pub rcc: Option<Rcc>,
    /// (pin, signal) -> alternate function.
    pub pins: BTreeMap<(String, String), Option<u8>>,
    /// Every DMA route. On a fixed-request DMA (the F4's) one stream can
    /// serve a signal through two channel selections, so the request is part
    /// of a route, not a value parts must agree on; on a DMAMUX it is fixed
    /// per signal, and `merge` checks that.
    pub dma: BTreeSet<PeripheralDma>,
    /// (device, signal) -> interrupt. A vector's name can be the device's:
    /// on the H753, the crypto line, RNG's vector is `HASH_RNG`, shared with
    /// HASH; on the H743 it is `RNG`. Parts agree within one device.
    pub interrupts: BTreeMap<(String, String), String>,
    /// The parts that have it.
    pub parts: BTreeSet<String>,
}

/// A die's table: the union of its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Die {
    pub id: u16,
    pub parts: Vec<String>,
    pub peripherals: BTreeMap<String, DiePeripheral>,
    /// vector name -> number.
    pub interrupts: BTreeMap<String, u32>,
    pub dma_channels: BTreeMap<String, DmaChannel>,
    /// package -> bonded pins.
    pub packages: BTreeMap<String, BTreeSet<String>>,
    /// device -> the peripherals its parts enable.
    pub devices: BTreeMap<String, BTreeSet<String>>,
}

/// Merges the parts of one die. Every disagreement is reported, not only the
/// first, so a metapac bump shows its whole effect at once.
pub fn merge(id: u16, parts: &[(Part, Metadata)]) -> Result<Die, Vec<String>> {
    let mut die = Die {
        id,
        ..Die::default()
    };
    let mut conflicts = Vec::new();
    // (peripheral, signal, DMAMUX) -> the requests: fixed by the die. A set,
    // since metapac gives several requests one signal name (HRTIM1's five
    // timers are all `TIMER`); compared whole between the parts that have it.
    let mut mux_requests: BTreeMap<(String, String, String), BTreeSet<Option<u32>>> =
        BTreeMap::new();
    for (part, metadata) in parts {
        if part.die != id {
            conflicts.push(format!(
                "{} is cut from die {:#x}, not {id:#x}",
                part.feature, part.die
            ));
            continue;
        }
        let name = part.feature.to_string();
        die.parts.push(name.clone());

        let pins: BTreeSet<String> = metadata.pins.iter().cloned().collect();
        match die.packages.get(part.package) {
            Some(known) if *known != pins => conflicts.push(format!(
                "{name}: package {} bonds other pins than another part of it",
                part.package
            )),
            Some(_) => {}
            None => {
                die.packages.insert(part.package.to_string(), pins);
            }
        }
        let enabled: BTreeSet<String> = metadata
            .peripherals
            .iter()
            .map(|p| p.name.clone())
            .collect();
        die.devices
            .entry(part.device.to_string())
            .or_default()
            .extend(enabled);

        for vector in &metadata.interrupts {
            agree(
                &mut conflicts,
                &name,
                &format!("vector {}", vector.name),
                die.interrupts
                    .entry(vector.name.clone())
                    .or_insert(vector.number),
                &vector.number,
            );
        }
        for channel in &metadata.dma_channels {
            agree(
                &mut conflicts,
                &name,
                &format!("DMA channel {}", channel.name),
                die.dma_channels
                    .entry(channel.name.clone())
                    .or_insert_with(|| channel.clone()),
                channel,
            );
        }

        for peripheral in &metadata.peripherals {
            let what = |field: &str| format!("{} {field}", peripheral.name);
            let merged = die
                .peripherals
                .entry(peripheral.name.clone())
                .or_insert_with(|| DiePeripheral {
                    address: peripheral.address,
                    kind: peripheral.kind.clone(),
                    version: peripheral.version.clone(),
                    block: peripheral.block.clone(),
                    rcc: peripheral.rcc.clone(),
                    pins: BTreeMap::new(),
                    dma: BTreeSet::new(),
                    interrupts: BTreeMap::new(),
                    parts: BTreeSet::new(),
                });
            merged.parts.insert(name.clone());
            agree(
                &mut conflicts,
                &name,
                &what("address"),
                &merged.address,
                &peripheral.address,
            );
            agree(
                &mut conflicts,
                &name,
                &what("IP kind"),
                &merged.kind,
                &peripheral.kind,
            );
            agree(
                &mut conflicts,
                &name,
                &what("IP version"),
                &merged.version,
                &peripheral.version,
            );
            agree(
                &mut conflicts,
                &name,
                &what("register block"),
                &merged.block,
                &peripheral.block,
            );
            agree(
                &mut conflicts,
                &name,
                &what("RCC fields"),
                &merged.rcc,
                &peripheral.rcc,
            );
            for pin in &peripheral.pins {
                let entry = merged
                    .pins
                    .entry((pin.pin.clone(), pin.signal.clone()))
                    .or_insert(pin.af);
                agree(
                    &mut conflicts,
                    &name,
                    &what(&format!("{} on {} alternate function", pin.signal, pin.pin)),
                    entry,
                    &pin.af,
                );
            }
            let mut requests: BTreeMap<(String, String, String), BTreeSet<Option<u32>>> =
                BTreeMap::new();
            for route in &peripheral.dma {
                if let Some(mux) = &route.dmamux {
                    requests
                        .entry((peripheral.name.clone(), route.signal.clone(), mux.clone()))
                        .or_default()
                        .insert(route.request);
                }
                merged.dma.insert(route.clone());
            }
            for (key, set) in requests {
                let what = what(&format!("{} requests on {}", key.1, key.2));
                let entry = mux_requests.entry(key).or_insert_with(|| set.clone());
                agree(&mut conflicts, &name, &what, entry, &set);
            }
            for vector in &peripheral.interrupts {
                let entry = merged
                    .interrupts
                    .entry((part.device.to_string(), vector.signal.clone()))
                    .or_insert_with(|| vector.interrupt.clone());
                agree(
                    &mut conflicts,
                    &name,
                    &what(&format!("{} interrupt", vector.signal)),
                    entry,
                    &vector.interrupt,
                );
            }
            // A row may only name pins this part's package bonds. The H7's
            // `_C` analog pads (PC2_C, PC3_C) are not GPIO and metapac does
            // not list them as pins; FerroHAL does not offer them.
            for pin in &peripheral.pins {
                if !pin.pin.ends_with("_C") && !metadata.pins.contains(&pin.pin) {
                    conflicts.push(format!(
                        "{name}: {} {} names {}, which its package {} does not bond",
                        peripheral.name, pin.signal, pin.pin, part.package
                    ));
                }
            }
        }
    }
    if conflicts.is_empty() {
        Ok(die)
    } else {
        Err(conflicts)
    }
}

fn agree<T: PartialEq + std::fmt::Debug>(
    conflicts: &mut Vec<String>,
    part: &str,
    what: &str,
    known: &T,
    seen: &T,
) {
    if known != seen {
        conflicts.push(format!(
            "{part}: {what} is {seen:?}, another part of the die says {known:?}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glue_model::{Interrupt, Peripheral, PeripheralPin};

    fn part(feature: &'static str, package: &'static str) -> Part {
        Part {
            feature,
            series: "H7",
            die: 0x450,
            device: "STM32H743",
            package,
        }
    }

    fn metadata(pins: &[&str], usart_tx: &[(&str, u8)]) -> Metadata {
        Metadata {
            name: "STM32H743XX".into(),
            family: "STM32H7".into(),
            line: "STM32H743/753".into(),
            nvic_priority_bits: Some(4),
            memory: vec![],
            interrupts: vec![Interrupt {
                name: "USART1".into(),
                number: 37,
            }],
            dma_channels: vec![],
            pins: pins.iter().map(|p| p.to_string()).collect(),
            peripherals: vec![Peripheral {
                name: "USART1".into(),
                address: 0x4001_1000,
                kind: Some("usart".into()),
                version: Some("v4".into()),
                block: Some("USART".into()),
                rcc: None,
                pins: usart_tx
                    .iter()
                    .map(|(pin, af)| PeripheralPin {
                        pin: pin.to_string(),
                        signal: "TX".into(),
                        af: Some(*af),
                    })
                    .collect(),
                dma: vec![],
                interrupts: vec![],
            }],
        }
    }

    #[test]
    fn a_die_is_the_union_of_its_parts() {
        let small = (part("a", "LQFP100"), metadata(&["PA9"], &[("PA9", 7)]));
        let large = (
            part("b", "LQFP144"),
            metadata(&["PA9", "PB14"], &[("PA9", 7), ("PB14", 4)]),
        );
        let die = merge(0x450, &[small, large]).unwrap_or_else(|e| panic!("{e:?}"));
        let usart = &die.peripherals["USART1"];
        assert_eq!(usart.pins.len(), 2);
        assert_eq!(usart.parts.len(), 2);
        assert_eq!(die.packages["LQFP100"].len(), 1);
        assert_eq!(die.packages["LQFP144"].len(), 2);
    }

    #[test]
    fn parts_that_disagree_on_an_alternate_function_fail() {
        let one = (part("a", "LQFP144"), metadata(&["PA9"], &[("PA9", 7)]));
        let two = (part("b", "LQFP144"), metadata(&["PA9"], &[("PA9", 8)]));
        let conflicts = merge(0x450, &[one, two]).expect_err("must conflict");
        assert!(
            conflicts.iter().any(|c| c.contains("TX on PA9")),
            "{conflicts:?}"
        );
    }

    #[test]
    fn a_dmamux_request_set_is_compared_whole_between_parts() {
        let route = |request| PeripheralDma {
            signal: "TIMER".into(),
            channel: None,
            dmamux: Some("DMAMUX1".into()),
            dma: None,
            request: Some(request),
        };
        let with = |requests: &[u32]| {
            let mut m = metadata(&["PA9"], &[("PA9", 7)]);
            m.peripherals[0].dma = requests.iter().map(|r| route(*r)).collect();
            m
        };
        // Several requests under one signal name within a part are fine.
        let one = (part("a", "LQFP144"), with(&[96, 97, 98]));
        let two = (part("b", "LQFP144"), with(&[96, 97, 98]));
        assert!(merge(0x450, &[one.clone(), two]).is_ok());
        // A part with another set conflicts.
        let three = (part("c", "LQFP144"), with(&[96, 97]));
        let conflicts = merge(0x450, &[one, three]).expect_err("must conflict");
        assert!(
            conflicts.iter().any(|c| c.contains("TIMER requests")),
            "{conflicts:?}"
        );
    }

    #[test]
    fn a_vector_name_may_differ_between_devices_but_not_within_one() {
        let named = |device: &'static str, vector: &str| {
            let mut m = metadata(&["PA9"], &[("PA9", 7)]);
            m.peripherals[0].interrupts = vec![glue_model::PeripheralInterrupt {
                signal: "GLOBAL".into(),
                interrupt: vector.into(),
            }];
            (
                Part {
                    device,
                    ..part("x", "LQFP144")
                },
                m,
            )
        };
        assert!(
            merge(
                0x450,
                &[named("STM32H743", "RNG"), named("STM32H753", "HASH_RNG")]
            )
            .is_ok()
        );
        let conflicts = merge(
            0x450,
            &[named("STM32H743", "RNG"), named("STM32H743", "HASH_RNG")],
        )
        .expect_err("must conflict");
        assert!(
            conflicts.iter().any(|c| c.contains("GLOBAL interrupt")),
            "{conflicts:?}"
        );
    }

    #[test]
    fn analog_pads_are_not_held_to_the_package() {
        let pads = (
            part("a", "LQFP100"),
            metadata(&["PA9"], &[("PA9", 7), ("PC2_C", 0)]),
        );
        assert!(merge(0x450, &[pads]).is_ok());
    }

    #[test]
    fn a_package_bonds_one_pin_set_and_rows_name_only_bonded_pins() {
        let one = (part("a", "LQFP100"), metadata(&["PA9"], &[("PA9", 7)]));
        let two = (
            part("b", "LQFP100"),
            metadata(&["PA9", "PB6"], &[("PA9", 7)]),
        );
        let conflicts = merge(0x450, &[one, two]).expect_err("must conflict");
        assert!(
            conflicts.iter().any(|c| c.contains("bonds other pins")),
            "{conflicts:?}"
        );

        let stray = (part("c", "LQFP64"), metadata(&["PA9"], &[("PB6", 7)]));
        let conflicts = merge(0x450, &[stray]).expect_err("must conflict");
        assert!(
            conflicts.iter().any(|c| c.contains("does not bond")),
            "{conflicts:?}"
        );
    }
}
