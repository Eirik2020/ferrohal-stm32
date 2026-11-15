//! Maturity tiers (ADR-009, `docs/design/maturity.md`), computed from the
//! evidence, never declared: `reqtool check --json` gives each requirement's
//! verified boards with the tier each ran at; this module turns them into each
//! part's platform and capability tiers, and the coverage matrix.
//!
//! A requirement reqtool calls verified has every declaring file covered by
//! a counting board run or test, which is the car rule applied per file; its
//! tier is the weakest tier among the boards that verified it, each of which
//! may have covered a different file. A requirement applies to a part unless
//! a version suffix (`USART-V2`) ties it to another IP version, a series
//! suffix (`RESET-H7`) to another series, or `SCOPED` to another die or
//! series. A capability on a part is as strong as its weakest requirement and
//! its die axis (a board of its die whose glue probe, GLUE-001, is verified);
//! the platform likewise. A part with no verified board evidence of its own
//! inherits (`INH`). A capability whose IP version FerroHAL does not drive on
//! the part is n/a.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::registry::{BENCHES, PARTS, Part};
use crate::silicon::Die;

/// A capability: its name in the tables, the IP kind it rests on, and its
/// requirement families. A family with a `-V<n>` suffix holds for that IP
/// version only; one with a series suffix (`-F4`, `-H7`) for that series only.
const CAPABILITIES: &[(&str, &str, &[&str])] = &[
    (
        "UART",
        "usart",
        &["UART", "UARTDMA", "USART-V2", "USART-V4"],
    ),
    ("SPI", "spi", &["SPI", "SPIPOLL", "SPI-V2", "SPI-V4"]),
    ("I2C", "i2c", &["I2C", "I2C-V1", "I2C-V2"]),
    ("Timers", "timer", &["TIMER", "PWM", "AWG"]),
    ("ADC", "adc", &["ADC"]),
    ("USB", "otg", &["USB", "USB-F4"]),
    ("SDMMC", "sdmmc", &["SDMMC"]),
    ("Watchdog", "iwdg", &["IWDG", "IWDG-V2"]),
    ("EXTI", "exti", &["EXTI"]),
    ("DMA", "dma", &["DMA"]),
];

/// The platform's families: what every part needs to run at all.
const PLATFORM: &[&str] = &[
    "RCC", "GPIO", "RESET", "RESET-H7", "UID", "REVISION", "GLUE", "WAIT", "API", "RESOURCE",
];

/// Requirements whose ID does not say their scope: the die or series they
/// hold for, by what their declaring files are.
const SCOPED: &[(&str, Scope)] = &[
    // shared/h7.rs: the H7's DMA-unreachable memory.
    ("GLUE-003", Scope::Series("H7")),
    // shared/die_0x413.rs: the F405 die's CCM RAM.
    ("GLUE-004", Scope::Die(0x413)),
];

#[derive(Clone, Copy)]
enum Scope {
    Series(&'static str),
    Die(u16),
}

/// `UART-012` -> `UART`; `USART-V2-001` -> `USART-V2`.
fn family(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(family, _)| family)
}

/// Whether a family applies to a part: its version suffix matches the part's
/// IP version, and its series suffix the part's series.
fn applies(family: &str, version: Option<&str>, series: &str) -> bool {
    match family.rsplit_once('-') {
        Some((_, suffix))
            if suffix.starts_with('V') && suffix[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            version.is_some_and(|v| v.split('_').next() == Some(&suffix.to_ascii_lowercase()))
        }
        Some((_, suffix)) if matches!(suffix, "F4" | "H7" | "G4" | "F7") => suffix == series,
        _ => true,
    }
}

/// A requirement: whether it has a target entry, whether reqtool calls it
/// verified, and the boards that verified it with their tiers.
type Holding = (bool, bool, Vec<(String, String)>);

struct Evidence {
    /// Tiers, strongest first.
    order: Vec<String>,
    /// requirement -> (has a target entry, verified, the verified boards' tiers).
    requirements: BTreeMap<String, Holding>,
}

impl Evidence {
    fn parse(json: &str) -> Result<Self, String> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| format!("reqtool check --json: {e}"))?;
        let order = value["tiers"]
            .as_array()
            .ok_or("reqtool check --json: no tiers; set them in reqtool.toml")?
            .iter()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        let mut requirements = BTreeMap::new();
        for requirement in value["requirements"].as_array().ok_or("no requirements")? {
            let id = requirement["id"]
                .as_str()
                .ok_or("requirement without id")?
                .to_owned();
            let verified = requirement["status"] == "verified";
            let mut target = false;
            let mut boards = Vec::new();
            for entry in requirement["entries"].as_array().into_iter().flatten() {
                if matches!(entry["level"].as_str(), Some("target" | "system" | "field")) {
                    target = true;
                }
                for board in entry["boards"].as_array().into_iter().flatten() {
                    if board["status"] == "verified"
                        && let (Some(name), Some(tier)) =
                            (board["board"].as_str(), board["tier"].as_str())
                    {
                        boards.push((name.to_owned(), tier.to_owned()));
                    }
                }
            }
            requirements.insert(id, (target, verified, boards));
        }
        Ok(Self {
            order,
            requirements,
        })
    }

    fn rank(&self, tier: &str) -> usize {
        self.order
            .iter()
            .position(|t| t == tier)
            .unwrap_or(usize::MAX)
    }

    /// The weakest of `tiers`; `None` (not covered) when any is.
    fn weakest(&self, tiers: impl IntoIterator<Item = Option<String>>) -> Option<String> {
        let mut worst: Option<String> = None;
        for tier in tiers {
            let tier = tier?;
            if worst
                .as_ref()
                .is_none_or(|w| self.rank(&tier) > self.rank(w))
            {
                worst = Some(tier);
            }
        }
        worst
    }

    /// A requirement's tier: the weakest of the boards that verified it, while
    /// reqtool calls it verified.
    fn tier(&self, id: &str) -> Option<String> {
        let (_, verified, boards) = self.requirements.get(id)?;
        if !*verified || boards.is_empty() {
            return None;
        }
        self.weakest(boards.iter().map(|(_, tier)| Some(tier.clone())))
    }

    /// The board-verified requirements of `families` that apply to a part.
    fn applying<'a>(
        &'a self,
        families: &'a [&str],
        version: Option<&'a str>,
        part: &'a Part,
    ) -> impl Iterator<Item = &'a String> + 'a {
        self.requirements
            .iter()
            .filter(move |(id, (target, _, _))| {
                *target
                    && families.contains(&family(id))
                    && applies(family(id), version, part.series)
                    && in_scope(id, part)
            })
            .map(|(id, _)| id)
    }
}

fn in_scope(id: &str, part: &Part) -> bool {
    match SCOPED.iter().find(|(scoped, _)| *scoped == id) {
        Some((_, Scope::Series(series))) => part.series == *series,
        Some((_, Scope::Die(die))) => part.die == *die,
        None => true,
    }
}

/// The die and IP versions a bench's part carries.
fn bench_part(board: &str) -> Option<&'static Part> {
    let feature = BENCHES.iter().find(|(name, _)| *name == board)?.1;
    PARTS.iter().find(|part| part.feature == feature)
}

/// The version of IP `kind` a part's die carries, as metapac names it.
fn version(die: &Die, part: &Part, kind: &str) -> Option<String> {
    let enabled = die.devices.get(part.device)?;
    die.peripherals
        .iter()
        .filter(|(name, p)| enabled.contains(*name) && p.kind.as_deref() == Some(kind))
        .filter(|(_, p)| kind != "usart" || p.block.as_deref() == Some("USART"))
        .find_map(|(_, p)| p.version.clone())
        .filter(|v| {
            matches!(kind, "iwdg" | "exti" | "dma") || crate::emit::drives(part.series, kind, v)
        })
}

pub fn tables(dies: &BTreeMap<u16, Die>, json: &str) -> Result<(String, String), String> {
    let evidence = Evidence::parse(json)?;
    let die_of = |board: &str| bench_part(board).map(|part| part.die);
    // The die axis: the strongest board of the die whose glue probe is verified.
    let die_tier = |die: u16| -> Option<String> {
        evidence
            .requirements
            .get("GLUE-001")
            .and_then(|(_, _, boards)| {
                boards
                    .iter()
                    .filter(|(board, _)| die_of(board) == Some(die))
                    .map(|(_, tier)| tier.clone())
                    .min_by_key(|tier| evidence.rank(tier))
            })
    };
    let fmt = |tier: Option<String>| tier.unwrap_or_else(|| "–".into());
    let platform: Vec<&str> = PLATFORM.to_vec();

    let mut chips = String::from("| part | platform |");
    let mut rule = String::from("|---|---|");
    for (name, _, _) in CAPABILITIES {
        chips += &format!(" {name} |");
        rule += "---|";
    }
    chips += &format!("\n{rule}\n");
    for part in PARTS {
        let die = dies.get(&part.die).ok_or("die not merged")?;
        let own = evidence.requirements.values().any(|(_, _, boards)| {
            boards
                .iter()
                .any(|(board, _)| bench_part(board).is_some_and(|p| p.feature == part.feature))
        });
        let platform_tier = evidence.weakest(
            evidence
                .applying(&platform, None, part)
                .filter(|id| id.as_str() != "GLUE-001")
                .map(|id| evidence.tier(id))
                .chain([die_tier(part.die)]),
        );
        let inherited = if own { "" } else { " `INH`" };
        chips += &format!(
            "| {} | {}{inherited} |",
            part.feature.to_uppercase(),
            fmt(platform_tier)
        );
        for (_, kind, families) in CAPABILITIES {
            let Some(v) = version(die, part, kind) else {
                chips += " n/a |";
                continue;
            };
            let ids: Vec<&String> = evidence.applying(families, Some(&v), part).collect();
            let ip = if ids.is_empty() {
                None
            } else {
                evidence.weakest(ids.iter().map(|id| evidence.tier(id)))
            };
            chips += &format!(" {} |", fmt(evidence.weakest([ip, die_tier(part.die)])));
        }
        chips += "\n";
    }

    // The coverage matrix: each driven IP version, die and series.
    let mut coverage = String::from("| axis | tier | benches |\n|---|---|---|\n");
    let mut versions: BTreeMap<(String, String), &Part> = BTreeMap::new();
    for part in PARTS {
        for (_, kind, _) in CAPABILITIES {
            if let Some(v) = version(&dies[&part.die], part, kind) {
                versions.entry(((*kind).to_owned(), v)).or_insert(part);
            }
        }
    }
    // The benches in a row: those that verified one of its requirements and
    // carry what the row is about.
    let benches_of = |ids: &[&String], carries: &dyn Fn(&Part) -> bool| -> String {
        let benches: BTreeSet<&str> = ids
            .iter()
            .flat_map(|id| evidence.requirements[*id].2.iter().map(|(b, _)| b.as_str()))
            .filter(|board| bench_part(board).is_some_and(carries))
            .collect();
        benches.into_iter().collect::<Vec<_>>().join(", ")
    };
    for ((kind, v), part) in &versions {
        let (_, _, families) = CAPABILITIES
            .iter()
            .find(|(_, k, _)| k == kind)
            .ok_or("kind")?;
        let ids: Vec<&String> = evidence.applying(families, Some(v), part).collect();
        let tier = if ids.is_empty() {
            None
        } else {
            evidence.weakest(ids.iter().map(|id| evidence.tier(id)))
        };
        let carries =
            |p: &Part| dies.get(&p.die).and_then(|d| version(d, p, kind)).as_ref() == Some(v);
        coverage += &format!(
            "| IP `{kind}_{v}` | {} | {} |\n",
            fmt(tier),
            benches_of(&ids, &carries)
        );
    }
    for die in dies.keys() {
        // The benches of the die whose glue probe is verified.
        let benches: Vec<&str> = evidence
            .requirements
            .get("GLUE-001")
            .map(|(_, _, boards)| {
                boards
                    .iter()
                    .filter(|(b, _)| die_of(b) == Some(*die))
                    .map(|(b, _)| b.as_str())
                    .collect()
            })
            .unwrap_or_default();
        coverage += &format!(
            "| die `{die:#05x}` (glue probe) | {} | {} |\n",
            fmt(die_tier(*die)),
            benches.join(", ")
        );
    }
    for series in PARTS.iter().map(|p| p.series).collect::<BTreeSet<_>>() {
        let part = PARTS.iter().find(|p| p.series == series).ok_or("series")?;
        let ids: Vec<&String> = evidence
            .applying(&platform, None, part)
            .filter(|id| id.as_str() != "GLUE-001")
            .collect();
        let tier = evidence.weakest(ids.iter().map(|id| evidence.tier(id)));
        coverage += &format!(
            "| series {series} platform | {} | {} |\n",
            fmt(tier),
            benches_of(&ids, &|p: &Part| p.series == series)
        );
    }
    Ok((chips, coverage))
}
