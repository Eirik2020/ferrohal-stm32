//! The parts FerroHAL supports, and the dies they are cut from.
//!
//! The silicon model (`docs/design/glue-generator.md`): a **die** is the
//! silicon, named by its DBGMCU `DEV_ID`; a **device** (product line) is which
//! of the die's peripherals a part enables; a **package** is which pins are
//! bonded; a **part** is device, package and memory size, and is what a user
//! picks, by metapac's name.

/// A part FerroHAL supports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    /// metapac's name and FerroHAL's Cargo feature: `stm32f405rg`.
    pub feature: &'static str,
    pub series: &'static str,
    /// DBGMCU `DEV_ID`.
    pub die: u16,
    /// The product line: `STM32F405`.
    pub device: &'static str,
    /// The package, by its bonded pin set: parts whose packages bond the
    /// same pins (the H743VI's LQFP100 and TFBGA100) share one.
    pub package: &'static str,
}

/// A die: its `DEV_ID`, the product lines cut from it, and its reference
/// manual.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Die {
    pub id: u16,
    pub lines: &'static [&'static str],
    pub manual: &'static str,
}

pub const PARTS: &[Part] = &[
    Part {
        feature: "stm32f401re",
        series: "F4",
        die: 0x433,
        device: "STM32F401",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f405rg",
        series: "F4",
        die: 0x413,
        device: "STM32F405",
        package: "LQFP64",
    },
    Part {
        feature: "stm32h743vi",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        // The TBS Lucid H7 fits the TFBGA100, which DS12110 Table 8 shows
        // bonds the same GPIO set as the LQFP100: one package here.
        package: "LQFP100",
    },
    Part {
        feature: "stm32h753zi",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "LQFP144",
    },
];

pub const DIES: &[Die] = &[
    Die {
        id: 0x413,
        lines: &["STM32F405", "STM32F407", "STM32F415", "STM32F417"],
        manual: "RM0090",
    },
    Die {
        id: 0x433,
        lines: &["STM32F401"],
        manual: "RM0368",
    },
    Die {
        id: 0x450,
        lines: &["STM32H742", "STM32H743", "STM32H750", "STM32H753"],
        manual: "RM0433",
    },
];

/// Which parts a command acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    All,
    Series(String),
    Die(u16),
    Part(String),
}

impl Selector {
    /// `all`, `series F4`, `die 0x413` (or a line cut from it: `die STM32F407`),
    /// `part stm32f405rg`.
    pub fn parse(words: &[String]) -> Result<Self, String> {
        match words {
            [all] if all == "all" => Ok(Self::All),
            [kind, value] if kind == "series" => Ok(Self::Series(value.to_ascii_uppercase())),
            [kind, value] if kind == "die" => die_of(value).map(Self::Die),
            [kind, value] if kind == "part" => Ok(Self::Part(value.to_ascii_lowercase())),
            _ => Err(
                "a selector is `all`, `series <F4>`, `die <0x413 | STM32F405>` or `part <stm32f405rg>`"
                    .to_string(),
            ),
        }
    }

    /// The supported parts it names; at least one, or why none.
    pub fn parts(&self) -> Result<Vec<Part>, String> {
        let parts: Vec<Part> = PARTS
            .iter()
            .copied()
            .filter(|part| match self {
                Self::All => true,
                Self::Series(series) => part.series == series,
                Self::Die(die) => part.die == *die,
                Self::Part(name) => part.feature == name,
            })
            .collect();
        if parts.is_empty() {
            Err(format!("{self:?} names no supported part"))
        } else {
            Ok(parts)
        }
    }
}

/// A die by its `DEV_ID` (`0x413`) or by a product line cut from it.
fn die_of(value: &str) -> Result<u16, String> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        let id = u16::from_str_radix(hex, 16).map_err(|_| format!("{value} is not a die ID"))?;
        return DIES
            .iter()
            .find(|die| die.id == id)
            .map(|die| die.id)
            .ok_or_else(|| format!("die {value} is not in the registry"));
    }
    let line = value.to_ascii_uppercase();
    DIES.iter()
        .find(|die| die.lines.contains(&line.as_str()))
        .map(|die| die.id)
        .ok_or_else(|| format!("no registered die carries the line {value}"))
}

/// The registry's own consistency: every part's die is registered and lists
/// the part's line, and every part's feature names its line.
pub fn check() -> Result<(), String> {
    for part in PARTS {
        let die = DIES
            .iter()
            .find(|die| die.id == part.die)
            .ok_or_else(|| format!("{}: die {:#x} is not registered", part.feature, part.die))?;
        if !die.lines.contains(&part.device) {
            return Err(format!(
                "{}: die {:#x} does not list the line {}",
                part.feature, part.die, part.device
            ));
        }
        if !part.feature.starts_with(&part.device.to_ascii_lowercase()) {
            return Err(format!(
                "{}: is not a part of {}",
                part.feature, part.device
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    fn features(text: &str) -> Vec<&'static str> {
        let selector = Selector::parse(&words(text)).unwrap_or_else(|error| panic!("{error}"));
        let parts = selector.parts().unwrap_or_else(|error| panic!("{error}"));
        parts.iter().map(|part| part.feature).collect()
    }

    #[test]
    fn the_registry_is_consistent() {
        assert_eq!(check(), Ok(()));
    }

    #[test]
    fn selectors_resolve_to_parts() {
        assert_eq!(features("all").len(), PARTS.len());
        assert_eq!(features("series F4"), ["stm32f401re", "stm32f405rg"]);
        assert_eq!(features("series h7"), ["stm32h743vi", "stm32h753zi"]);
        assert_eq!(features("part STM32F405RG"), ["stm32f405rg"]);
    }

    #[test]
    fn a_die_is_named_by_its_id_or_any_line_cut_from_it() {
        assert_eq!(features("die 0x450"), ["stm32h743vi", "stm32h753zi"]);
        // The F407 shares the F405's die, so it selects the F405's parts.
        assert_eq!(features("die STM32F407"), ["stm32f405rg"]);
        assert_eq!(features("die stm32h750"), ["stm32h743vi", "stm32h753zi"]);
    }

    #[test]
    fn a_selector_that_names_nothing_is_refused() {
        assert!(Selector::parse(&words("die 0x999")).is_err());
        assert!(Selector::parse(&words("die STM32G474")).is_err());
        assert!(Selector::parse(&words("chip f4")).is_err());
        assert!(Selector::Part("stm32g474re".into()).parts().is_err());
    }
}
