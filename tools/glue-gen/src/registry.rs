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
        feature: "stm32f401ce",
        series: "F4",
        die: 0x433,
        device: "STM32F401",
        package: "UFQFPN48",
    },
    Part {
        feature: "stm32f401re",
        series: "F4",
        die: 0x433,
        device: "STM32F401",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f401ve",
        series: "F4",
        die: 0x433,
        device: "STM32F401",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f411ce",
        series: "F4",
        die: 0x431,
        device: "STM32F411",
        package: "UFQFPN48",
    },
    Part {
        feature: "stm32f411re",
        series: "F4",
        die: 0x431,
        device: "STM32F411",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f411ve",
        series: "F4",
        die: 0x431,
        device: "STM32F411",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f405og",
        series: "F4",
        die: 0x413,
        device: "STM32F405",
        package: "WLCSP90",
    },
    Part {
        feature: "stm32f405rg",
        series: "F4",
        die: 0x413,
        device: "STM32F405",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f405vg",
        series: "F4",
        die: 0x413,
        device: "STM32F405",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f405zg",
        series: "F4",
        die: 0x413,
        device: "STM32F405",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f407vg",
        series: "F4",
        die: 0x413,
        device: "STM32F407",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f407zg",
        series: "F4",
        die: 0x413,
        device: "STM32F407",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f407ig",
        series: "F4",
        die: 0x413,
        device: "STM32F407",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f427vi",
        series: "F4",
        die: 0x419,
        device: "STM32F427",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f427zi",
        series: "F4",
        die: 0x419,
        device: "STM32F427",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f427ii",
        series: "F4",
        die: 0x419,
        device: "STM32F427",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f427ai",
        series: "F4",
        die: 0x419,
        device: "STM32F427",
        package: "UFBGA169",
    },
    Part {
        feature: "stm32f429vi",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f429zi",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f429ii",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f429ai",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "UFBGA169",
    },
    Part {
        feature: "stm32f429bi",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "LQFP208",
    },
    Part {
        feature: "stm32f429ni",
        series: "F4",
        die: 0x419,
        device: "STM32F429",
        package: "TFBGA216",
    },
    Part {
        feature: "stm32f412cg",
        series: "F4",
        die: 0x441,
        device: "STM32F412",
        package: "UFQFPN48",
    },
    Part {
        feature: "stm32f412rg",
        series: "F4",
        die: 0x441,
        device: "STM32F412",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f412vg",
        series: "F4",
        die: 0x441,
        device: "STM32F412",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f412zg",
        series: "F4",
        die: 0x441,
        device: "STM32F412",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f413ch",
        series: "F4",
        die: 0x463,
        device: "STM32F413",
        package: "UFQFPN48",
    },
    Part {
        feature: "stm32f413mh",
        series: "F4",
        die: 0x463,
        device: "STM32F413",
        package: "WLCSP81",
    },
    Part {
        feature: "stm32f413rh",
        series: "F4",
        die: 0x463,
        device: "STM32F413",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f413vh",
        series: "F4",
        die: 0x463,
        device: "STM32F413",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f413zh",
        series: "F4",
        die: 0x463,
        device: "STM32F413",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f446me",
        series: "F4",
        die: 0x421,
        device: "STM32F446",
        package: "WLCSP81",
    },
    Part {
        feature: "stm32f446re",
        series: "F4",
        die: 0x421,
        device: "STM32F446",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f446ve",
        series: "F4",
        die: 0x421,
        device: "STM32F446",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f446ze",
        series: "F4",
        die: 0x421,
        device: "STM32F446",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f722re",
        series: "F7",
        die: 0x452,
        device: "STM32F722",
        package: "LQFP64",
    },
    Part {
        feature: "stm32f722ve",
        series: "F7",
        die: 0x452,
        device: "STM32F722",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f722ze",
        series: "F7",
        die: 0x452,
        device: "STM32F722",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f722ie",
        series: "F7",
        die: 0x452,
        device: "STM32F722",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f745vg",
        series: "F7",
        die: 0x449,
        device: "STM32F745",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f745zg",
        series: "F7",
        die: 0x449,
        device: "STM32F745",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f745ig",
        series: "F7",
        die: 0x449,
        device: "STM32F745",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f767vi",
        series: "F7",
        die: 0x451,
        device: "STM32F767",
        package: "LQFP100",
    },
    Part {
        feature: "stm32f767zi",
        series: "F7",
        die: 0x451,
        device: "STM32F767",
        package: "LQFP144",
    },
    Part {
        feature: "stm32f767ii",
        series: "F7",
        die: 0x451,
        device: "STM32F767",
        package: "LQFP176",
    },
    Part {
        feature: "stm32f767bi",
        series: "F7",
        die: 0x451,
        device: "STM32F767",
        package: "LQFP208",
    },
    Part {
        feature: "stm32f767ni",
        series: "F7",
        die: 0x451,
        device: "STM32F767",
        package: "TFBGA216",
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
        feature: "stm32h743zi",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        package: "LQFP144",
    },
    Part {
        feature: "stm32h743ii",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        package: "LQFP176",
    },
    Part {
        feature: "stm32h743bi",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        package: "LQFP208",
    },
    Part {
        feature: "stm32h743ai",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        package: "UFBGA169",
    },
    Part {
        feature: "stm32h743xi",
        series: "H7",
        die: 0x450,
        device: "STM32H743",
        package: "TFBGA240",
    },
    Part {
        feature: "stm32h753vi",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "LQFP100",
    },
    Part {
        feature: "stm32h753zi",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "LQFP144",
    },
    Part {
        feature: "stm32h753ii",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "LQFP176",
    },
    Part {
        feature: "stm32h753bi",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "LQFP208",
    },
    Part {
        feature: "stm32h753ai",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "UFBGA169",
    },
    Part {
        feature: "stm32h753xi",
        series: "H7",
        die: 0x450,
        device: "STM32H753",
        package: "TFBGA240",
    },
];

/// Parts that differ from a registered part only in flash size: the same
/// device and package, so the same bonded pins and the same glue. Each is a
/// Cargo feature of its own that mounts its registered part's glue. `model`
/// dumps them too, so a variant whose metadata disagrees with its part fails
/// the generator.
pub const VARIANTS: &[(&str, &str)] = &[
    ("stm32f401cd", "stm32f401ce"),
    ("stm32f401rd", "stm32f401re"),
    ("stm32f401vd", "stm32f401ve"),
    ("stm32f411cc", "stm32f411ce"),
    ("stm32f411rc", "stm32f411re"),
    ("stm32f411vc", "stm32f411ve"),
    ("stm32f405oe", "stm32f405og"),
    ("stm32f407ve", "stm32f407vg"),
    ("stm32f407ze", "stm32f407zg"),
    ("stm32f407ie", "stm32f407ig"),
    ("stm32f446mc", "stm32f446me"),
    ("stm32f446rc", "stm32f446re"),
    ("stm32f446vc", "stm32f446ve"),
    ("stm32f446zc", "stm32f446ze"),
    ("stm32f722rc", "stm32f722re"),
    ("stm32f722vc", "stm32f722ve"),
    ("stm32f722zc", "stm32f722ze"),
    ("stm32f722ic", "stm32f722ie"),
    ("stm32f745ve", "stm32f745vg"),
    ("stm32f745ze", "stm32f745zg"),
    ("stm32f745ie", "stm32f745ig"),
    ("stm32f767vg", "stm32f767vi"),
    ("stm32f767zg", "stm32f767zi"),
    ("stm32f767ig", "stm32f767ii"),
    ("stm32f767bg", "stm32f767bi"),
    ("stm32f767ng", "stm32f767ni"),
    ("stm32h743vg", "stm32h743vi"),
    ("stm32h743zg", "stm32h743zi"),
    ("stm32h743ig", "stm32h743ii"),
    ("stm32h743bg", "stm32h743bi"),
    ("stm32h743ag", "stm32h743ai"),
    ("stm32h743xg", "stm32h743xi"),
    ("stm32f427vg", "stm32f427vi"),
    ("stm32f427zg", "stm32f427zi"),
    ("stm32f427ig", "stm32f427ii"),
    ("stm32f427ag", "stm32f427ai"),
    ("stm32f429vg", "stm32f429vi"),
    ("stm32f429ve", "stm32f429vi"),
    ("stm32f429zg", "stm32f429zi"),
    ("stm32f429ze", "stm32f429zi"),
    ("stm32f429ig", "stm32f429ii"),
    ("stm32f429ie", "stm32f429ii"),
    ("stm32f429ag", "stm32f429ai"),
    ("stm32f429bg", "stm32f429bi"),
    ("stm32f429be", "stm32f429bi"),
    ("stm32f429ng", "stm32f429ni"),
    ("stm32f429ne", "stm32f429ni"),
    ("stm32f412ce", "stm32f412cg"),
    ("stm32f412re", "stm32f412rg"),
    ("stm32f412ve", "stm32f412vg"),
    ("stm32f412ze", "stm32f412zg"),
    ("stm32f413cg", "stm32f413ch"),
    ("stm32f413mg", "stm32f413mh"),
    ("stm32f413rg", "stm32f413rh"),
    ("stm32f413vg", "stm32f413vh"),
    ("stm32f413zg", "stm32f413zh"),
];

/// The benches evidence is recorded on (`hil/<crate>`), each with the part
/// it carries: what the tier command reads a board's die and series from.
pub const BENCHES: &[(&str, &str)] = &[
    ("nucleo-f401re-loopback", "stm32f401re"),
    ("nucleo-h753zi-loopback", "stm32h753zi"),
    ("stm32f4-discovery", "stm32f407vg"),
    ("foxeer-f405-v2", "stm32f405rg"),
    ("lucid-h7", "stm32h743vi"),
];

pub const DIES: &[Die] = &[
    Die {
        id: 0x413,
        lines: &["STM32F405", "STM32F407", "STM32F415", "STM32F417"],
        manual: "RM0090",
    },
    Die {
        id: 0x419,
        lines: &["STM32F427", "STM32F429", "STM32F437", "STM32F439"],
        manual: "RM0090",
    },
    Die {
        id: 0x421,
        lines: &["STM32F446"],
        manual: "RM0390",
    },
    Die {
        id: 0x431,
        lines: &["STM32F411"],
        manual: "RM0383",
    },
    Die {
        id: 0x433,
        lines: &["STM32F401"],
        manual: "RM0368",
    },
    Die {
        id: 0x441,
        lines: &["STM32F412"],
        manual: "RM0402",
    },
    Die {
        id: 0x449,
        lines: &["STM32F745", "STM32F746", "STM32F756"],
        manual: "RM0385",
    },
    Die {
        id: 0x450,
        lines: &["STM32H742", "STM32H743", "STM32H750", "STM32H753"],
        manual: "RM0433",
    },
    Die {
        id: 0x451,
        lines: &[
            "STM32F765",
            "STM32F767",
            "STM32F768",
            "STM32F769",
            "STM32F777",
            "STM32F778",
            "STM32F779",
        ],
        manual: "RM0410",
    },
    Die {
        id: 0x452,
        lines: &["STM32F722", "STM32F723", "STM32F732", "STM32F733"],
        manual: "RM0431",
    },
    Die {
        id: 0x463,
        lines: &["STM32F413", "STM32F423"],
        manual: "RM0430",
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
    for (variant, part) in VARIANTS {
        let registered = PARTS
            .iter()
            .find(|p| p.feature == *part)
            .ok_or_else(|| format!("{variant}: {part} is not a registered part"))?;
        // stm32 + line (3) + pin count (1): `stm32f401c` of `stm32f401cd`.
        let prefix = &registered.feature[..10];
        if variant.len() != registered.feature.len() || !variant.starts_with(prefix) {
            return Err(format!(
                "{variant}: differs from {part} in more than its flash size"
            ));
        }
        if PARTS.iter().any(|p| p.feature == *variant) {
            return Err(format!("{variant}: registered as a part and a variant"));
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
        assert_eq!(
            features("die 0x433"),
            ["stm32f401ce", "stm32f401re", "stm32f401ve"]
        );
        assert_eq!(features("series F4").len(), 36);
        assert_eq!(features("series f7").len(), 12);
        assert_eq!(features("series h7").len(), 12);
        assert_eq!(features("part STM32F405RG"), ["stm32f405rg"]);
    }

    #[test]
    fn a_die_is_named_by_its_id_or_any_line_cut_from_it() {
        // The F407 shares the F405's die, so either line selects both lines'
        // parts.
        let f405_f407 = [
            "stm32f405og",
            "stm32f405rg",
            "stm32f405vg",
            "stm32f405zg",
            "stm32f407vg",
            "stm32f407zg",
            "stm32f407ig",
        ];
        assert_eq!(features("die STM32F407"), f405_f407);
        assert_eq!(features("die 0x413"), f405_f407);
        assert_eq!(features("die stm32h750"), features("die 0x450"));
        assert_eq!(features("die 0x450").len(), 12);
    }

    #[test]
    fn a_variant_differs_only_in_flash_size() {
        for (variant, part) in VARIANTS {
            assert_eq!(variant[..10], part[..10], "{variant}");
            assert_ne!(variant, part);
        }
        // A variant is not a selectable part: it has no glue of its own.
        assert!(Selector::Part("stm32f401rd".into()).parts().is_err());
    }

    #[test]
    fn a_selector_that_names_nothing_is_refused() {
        assert!(Selector::parse(&words("die 0x999")).is_err());
        assert!(Selector::parse(&words("die STM32G474")).is_err());
        assert!(Selector::parse(&words("chip f4")).is_err());
        assert!(Selector::Part("stm32g474re".into()).parts().is_err());
    }
}
