//! `docs/CHIPS.md`'s generated tables (ADR-008 item 7): the silicon table and
//! the instance matrix, written between `<!-- generated: glue-gen ... -->`
//! markers so the prose around them stays hand-written. Every cell restates
//! the registry, the die tables or a hand fact the chip module states; none
//! is typed here.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::emit;
use crate::registry::Part;
use crate::silicon::Die;

/// `["TIM1", "TIM2", "TIM3", "TIM9"]` -> `TIM1-3/9`; stems in first-seen order.
fn compact(names: &[String]) -> String {
    if names
        .iter()
        .any(|name| !name.ends_with(|c: char| c.is_ascii_digit()))
    {
        return names.join(", ");
    }
    let mut stems: Vec<(String, Vec<u32>)> = Vec::new();
    for name in names {
        let digits: String = name
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        let stem = name[..name.len() - digits.len()].to_string();
        let n: u32 = digits
            .chars()
            .rev()
            .collect::<String>()
            .parse()
            .unwrap_or(0);
        match stems.iter_mut().find(|(s, _)| *s == stem) {
            Some((_, ns)) => ns.push(n),
            None => stems.push((stem, vec![n])),
        }
    }
    stems
        .into_iter()
        .map(|(stem, mut ns)| {
            ns.sort_unstable();
            let mut runs: Vec<String> = Vec::new();
            let mut i = 0;
            while i < ns.len() {
                let mut j = i;
                while j + 1 < ns.len() && ns[j + 1] == ns[j] + 1 {
                    j += 1;
                }
                runs.push(if j >= i + 2 {
                    format!("{}-{}", ns[i], ns[j])
                } else {
                    ns[i..=j]
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join("/")
                });
                i = j + 1;
            }
            format!("{stem}{}", runs.join("/"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A part's module text: its line's chip file, the one that mounts the
/// part's generated glue, and every module that file mounts, one level down
/// (a die file, the family's common file, an errata table, the macros).
fn module_text(root: &Path, part: &Part) -> String {
    let Ok(files) = crate::select::chip_files(root) else {
        return String::new();
    };
    let Some(file) = files.get(part.feature) else {
        return String::new();
    };
    let path = root.join("src").join(file);
    let own = fs::read_to_string(&path).unwrap_or_default();
    let here = path.parent().unwrap_or(root);
    let mut text = own.clone();
    for line in own.lines() {
        if let Some(rest) = line.trim().strip_prefix("#[path = \"")
            && let Some(file) = rest.strip_suffix("\"]")
            && !file.starts_with("../gen/")
        {
            text += &fs::read_to_string(here.join(file)).unwrap_or_default();
        }
    }
    text
}

fn clock(text: &str) -> String {
    let mhz = |key: &str| {
        let start = text.find(key)? + key.len();
        let digits: String = text[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse::<u32>().ok()
    };
    if let Some(m) = mhz("sysclk_max: Hertz::mhz(") {
        format!("{m} MHz")
    } else if let Some(m) = mhz("scale1: (Hertz::mhz(") {
        format!("{m} MHz (VOS1)")
    } else {
        "?".into()
    }
}

fn core(series: &str) -> &'static str {
    match series {
        "F4" => "Cortex-M4F",
        "F7" | "H7" => "Cortex-M7",
        _ => "?",
    }
}

/// The two generated tables, for every registered part.
pub fn tables(
    root: &Path,
    dies: &BTreeMap<u16, Die>,
    parts: &[Part],
) -> Result<(String, String), String> {
    let mut silicon = String::from(
        "| part | series, core | die | device | package (bonded pins) | clock as offered |\n|---|---|---|---|---|---|\n",
    );
    let mut matrix: Vec<(&str, Vec<String>)> = vec![
        ("**UART** (`Uart`; receive-only by `new_rx`)", vec![]),
        ("**SPI** over DMA (`Spi`)", vec![]),
        ("**SPI** polled (`BlockingSpi`)", vec![]),
        ("**I2C** (`I2c`)", vec![]),
        ("**Timers** (`Tick`, `Timebase`, `Pwm`)", vec![]),
        ("compare channels (PWM, lanes)", vec![]),
        ("complementary outputs (CHxN)", vec![]),
        ("**ADC** (`Adc`, `AdcScan`)", vec![]),
        ("**USB** device (`UsbFs`)", vec![]),
        ("**SDMMC** (`Sdmmc`)", vec![]),
        ("**DMA** controllers", vec![]),
    ];
    let mut header = String::from("| capability |");
    let mut rule = String::from("|---|");
    for part in parts {
        let die = dies.get(&part.die).ok_or("die not merged")?;
        let bonded = die.packages.get(part.package).map(|p| p.len()).unwrap_or(0);
        let text = module_text(root, part);
        silicon += &format!(
            "| {} | {}, {} | {:#05x} | {} | {} ({bonded}) | {} |\n",
            part.feature.to_uppercase(),
            part.series,
            core(part.series),
            part.die,
            part.device,
            part.package,
            clock(&text)
        );
        header += &format!(
            " {} |",
            part.feature.trim_start_matches("stm32").to_uppercase()
        );
        rule += "---|";
        let offer = emit::offer(die, part)?;
        let cells = [
            (&offer.uarts, "usart"),
            (&offer.spis_dma, "spi"),
            (&offer.spis, "spi"),
            (&offer.i2cs, "i2c"),
            (&offer.timers, "timer"),
            (&offer.channel_timers, "timer"),
            (&offer.complementary, "timer"),
            (&offer.adcs, "adc"),
            (&offer.usb, "otg"),
            (&offer.sdmmcs, "sdmmc"),
            (&offer.dmas, "dma"),
        ];
        for ((_, column), (names, kind)) in matrix.iter_mut().zip(cells) {
            column.push(if names.is_empty() && offer.undriven.contains(&kind) {
                "–".into()
            } else if names.is_empty() {
                "n/a".into()
            } else {
                compact(names)
            });
        }
    }
    let mut instances = format!("{header}\n{rule}\n");
    for (capability, cells) in matrix {
        instances += &format!("| {capability} | {} |\n", cells.join(" | "));
    }
    Ok((silicon, instances))
}

/// Replaces the text between `<!-- generated: glue-gen <name> -->` and
/// `<!-- end generated -->`.
pub fn splice(doc: &str, name: &str, table: &str) -> Result<String, String> {
    let open = format!("<!-- generated: glue-gen {name} -->\n");
    let close = "<!-- end generated -->";
    let start = doc
        .find(&open)
        .ok_or_else(|| format!("CHIPS.md has no `{}` marker", open.trim()))?
        + open.len();
    let end = start + doc[start..].find(close).ok_or("unclosed generated block")?;
    Ok(format!("{}{table}{}", &doc[..start], &doc[end..]))
}
