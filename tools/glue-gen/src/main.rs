//! The chip-glue generator (ADR-008, `docs/design/glue-generator.md`).
//!
//!     cargo run --manifest-path tools/glue-gen/Cargo.toml -- <command> <selector>
//!
//! Commands: `parts` lists the parts a selector names; `dump` reads each
//! one's metadata through `glue-dump` and writes it to
//! `tools/glue-gen/target/parts/<part>.json`; `model` builds the die table of
//! every die the selector touches, from all of that die's registered parts,
//! and fails on any fact two of them disagree on (`silicon.rs`). Selectors: `all`,
//! `series F4`, `die 0x413` (or a line cut from it, `die STM32F407`),
//! `part stm32f405rg`.
//!
//! Before anything, the tool checks that its stm32-metapac is the one the
//! library's `Cargo.lock` pins, so the glue is generated from the metadata
//! the library builds against.

mod emit;
#[cfg(feature = "probe")]
mod probe;
mod registry;
mod select;
mod silicon;
mod tables;
mod tiers;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use glue_model::Part as Metadata;
use registry::{Part, Selector};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("glue-gen: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let (command, selector) = args.split_first().ok_or(
        "usage: glue-gen <parts | dump | model | emit | tables | tiers | probe> <selector>",
    )?;
    registry::check()?;
    let parts = Selector::parse(selector)?.parts()?;
    match command.as_str() {
        "parts" => {
            for part in &parts {
                println!(
                    "{:<12} series {}  die {:#05x}  {}  {}",
                    part.feature, part.series, part.die, part.device, part.package
                );
            }
            Ok(())
        }
        "dump" => {
            let version = same_metapac()?;
            for part in &parts {
                let metadata = dump(part)?;
                let path = parts_dir().join(format!("{}.json", part.feature));
                let json = serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?;
                fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
                println!(
                    "{:<12} metapac {version}: {} peripherals, {} pins, {} interrupts -> {}",
                    part.feature,
                    metadata.peripherals.len(),
                    metadata.pins.len(),
                    metadata.interrupts.len(),
                    path.display()
                );
            }
            Ok(())
        }
        "model" => {
            let version = same_metapac()?;
            for id in dies_of(&parts) {
                report(&merged(id, &version)?);
            }
            Ok(())
        }
        "emit" => {
            let version = same_metapac()?;
            let out = tool_dir().join("../../src/chips/gen");
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            for id in dies_of(&parts) {
                let die = merged(id, &version)?;
                for part in parts.iter().filter(|part| part.die == id) {
                    let text = emit::emit(&die, part, &version)?;
                    let path = out.join(format!("{}.rs", part.feature));
                    fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
                    // As `cargo fmt` leaves it, so the merge gate's regeneration
                    // compares equal.
                    let status = Command::new("rustfmt")
                        .args(["--edition", "2024"])
                        .arg(&path)
                        .status()
                        .map_err(|e| format!("running rustfmt: {e}"))?;
                    if !status.success() {
                        return Err(format!("rustfmt failed on {}", path.display()));
                    }
                    println!(
                        "{:<12} -> src/chips/gen/{}.rs ({} lines)",
                        part.feature,
                        part.feature,
                        text.lines().count()
                    );
                }
            }
            // The chip selection follows the whole registry, whatever the
            // selector named.
            select::write(&tool_dir().join("../.."))?;
            println!(
                "src/lib.rs: the chip selection for {} parts",
                registry::PARTS.len()
            );
            Ok(())
        }
        "tables" => {
            let version = same_metapac()?;
            let root = tool_dir().join("../..");
            let mut dies = std::collections::BTreeMap::new();
            for id in dies_of(registry::PARTS) {
                dies.insert(id, merged(id, &version)?);
            }
            let (silicon, instances) = tables::tables(&root, &dies, registry::PARTS)?;
            let path = root.join("docs/CHIPS.md");
            let doc = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let doc = tables::splice(&doc, "silicon", &silicon)?;
            let doc = tables::splice(&doc, "instances", &instances)?;
            fs::write(&path, doc).map_err(|e| format!("{}: {e}", path.display()))?;
            println!(
                "docs/CHIPS.md: silicon and instance tables for {} parts",
                registry::PARTS.len()
            );
            Ok(())
        }
        #[cfg(feature = "probe")]
        "probe" => {
            let version = same_metapac()?;
            let selector = std::env::var("PROBE_RS_PROBE").map_err(|_| {
                "set PROBE_RS_PROBE to the board's probe, VID:PID:Serial as probe-rs list prints it"
            })?;
            let mut failed = 0;
            for part in &parts {
                let die = merged(part.die, &version)?;
                let (passed, fails) = probe::probe(&die, part, &selector)?;
                println!(
                    "{}: {passed} passed, {fails} failed (metapac {version})",
                    part.feature
                );
                failed += fails;
            }
            if failed == 0 {
                Ok(())
            } else {
                Err(format!("{failed} glue fact(s) failed on the silicon"))
            }
        }
        "tiers" => {
            let version = same_metapac()?;
            let root = tool_dir().join("../..");
            let reqtool = std::env::var("REQTOOL").unwrap_or_else(|_| "reqtool".into());
            let output = Command::new(&reqtool)
                .args(["check", "--json"])
                .current_dir(&root)
                .output()
                .map_err(|e| format!("running {reqtool} (set REQTOOL to its path): {e}"))?;
            if !output.status.success() {
                return Err(format!(
                    "reqtool check --json: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            let mut dies = std::collections::BTreeMap::new();
            for id in dies_of(registry::PARTS) {
                dies.insert(id, merged(id, &version)?);
            }
            let (chips, coverage) = tiers::tables(&dies, &String::from_utf8_lossy(&output.stdout))?;
            for (file, name, table) in [
                ("docs/CHIPS.md", "tiers", &chips),
                ("hil/COVERAGE.md", "coverage", &coverage),
            ] {
                let path = root.join(file);
                let doc =
                    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                fs::write(&path, tables::splice(&doc, name, table)?)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            }
            println!("docs/CHIPS.md and hil/COVERAGE.md: tiers from the evidence");
            Ok(())
        }
        other => Err(format!(
            "unknown command {other}: parts, dump, model, emit, tables, tiers or probe"
        )),
    }
}

/// One die table, summarised: what every part has, and what only some do.
/// The dies `parts` are cut from, each once.
fn dies_of(parts: &[Part]) -> Vec<u16> {
    let mut dies: Vec<u16> = parts.iter().map(|part| part.die).collect();
    dies.sort_unstable();
    dies.dedup();
    dies
}

/// A die's table, merged from every registered part cut from it.
fn merged(id: u16, version: &str) -> Result<silicon::Die, String> {
    let mut dumped = Vec::new();
    for part in registry::PARTS.iter().filter(|part| part.die == id) {
        dumped.push((*part, dump(part)?));
    }
    // A flash-size variant joins as its part's device and package, so any
    // fact it disagrees on (a pin, a route, a vector) is a conflict.
    for (variant, of) in registry::VARIANTS {
        let Some(part) = registry::PARTS
            .iter()
            .find(|p| p.feature == *of && p.die == id)
        else {
            continue;
        };
        let as_part = Part {
            feature: variant,
            ..*part
        };
        dumped.push((as_part, dump(&as_part)?));
    }
    silicon::merge(id, &dumped).map_err(|conflicts| {
        format!(
            "die {id:#05x} (metapac {version}): {} conflict(s):\n  {}",
            conflicts.len(),
            conflicts.join("\n  ")
        )
    })
}

fn report(die: &silicon::Die) {
    let rows: usize = die.peripherals.values().map(|p| p.pins.len()).sum();
    let routes: usize = die.peripherals.values().map(|p| p.dma.len()).sum();
    println!(
        "die {:#05x}: parts {}; {} peripherals, {} pin rows, {} DMA routes, {} vectors",
        die.id,
        die.parts.join(", "),
        die.peripherals.len(),
        rows,
        routes,
        die.interrupts.len()
    );
    for (package, pins) in &die.packages {
        println!("  package {package}: {} bonded pins", pins.len());
    }
    for (device, peripherals) in &die.devices {
        println!("  device {device}: {} peripherals", peripherals.len());
    }
    let partial: Vec<String> = die
        .peripherals
        .iter()
        .filter(|(_, p)| p.parts.len() < die.parts.len())
        .map(|(name, p)| {
            format!(
                "{name} ({})",
                p.parts.iter().cloned().collect::<Vec<_>>().join(", ")
            )
        })
        .collect();
    if !partial.is_empty() {
        println!("  on some parts only: {}", partial.join("; "));
    }
}

fn tool_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn parts_dir() -> PathBuf {
    let dir = tool_dir().join("target/parts");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// One part's metadata: from the dump cache while its key still matches,
/// otherwise from `glue-dump` built with that part's feature, then cached.
///
/// Building `glue-dump` compiles stm32-metapac once per part, which is most of
/// a cold regeneration. The cache skips only that: every part is still
/// emitted, merged and checked. A part's key covers everything its dump can
/// depend on (`dump_key`), so a stale entry is never read. `GLUE_DUMP_CACHE`
/// names the cache directory (CI keeps it between runs); `off` disables it.
fn dump(part: &Part) -> Result<Metadata, String> {
    let cache = match std::env::var("GLUE_DUMP_CACHE") {
        Ok(dir) if dir == "off" => None,
        Ok(dir) => Some(PathBuf::from(dir)),
        Err(_) => Some(tool_dir().join("target/dump-cache")),
    };
    let Some(cache) = cache else {
        return dumped(part);
    };
    let key = dump_key(part.feature)?;
    let path = cache.join(format!("{}.json", part.feature));
    if let Ok(text) = fs::read_to_string(&path)
        && let Some((head, json)) = text.split_once('\n')
        && head == key
        && let Ok(metadata) = serde_json::from_str::<Metadata>(json)
        && metadata.name.eq_ignore_ascii_case(part.feature)
    {
        return Ok(metadata);
    }
    let metadata = dumped(part)?;
    // Best effort: a cache that cannot be written only costs the next run time.
    if let Ok(json) = serde_json::to_string(&metadata) {
        let _ = fs::create_dir_all(&cache);
        let _ = fs::write(&path, format!("{key}\n{json}"));
    }
    Ok(metadata)
}

/// What one part's dump depends on, hashed: the generator's lockfile (the
/// metapac and serde_json it builds), `glue-dump` and `glue-model`'s sources,
/// and `dump/Cargo.toml` with the other parts' feature lines left out, so
/// registering a part does not invalidate every other part's entry.
fn dump_key(feature: &str) -> Result<String, String> {
    let root = tool_dir();
    let mut files = vec![root.join("Cargo.lock"), root.join("model/Cargo.toml")];
    for dir in ["dump/src", "model/src"] {
        sources(&root.join(dir), &mut files)?;
    }
    files.sort();
    let mut hash = Fnv::default();
    hash.write(b"glue-dump cache 1\0");
    hash.write(feature.as_bytes());
    for file in &files {
        let text = fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        hash.write(
            file.strip_prefix(&root)
                .unwrap_or(file)
                .to_string_lossy()
                .as_bytes(),
        );
        hash.write(b"\0");
        hash.write(&text);
        hash.write(b"\0");
    }
    let manifest = root.join("dump/Cargo.toml");
    let text = fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    for line in text.lines() {
        let other_part = line
            .split_once(" = [\"stm32-metapac/")
            .is_some_and(|(name, _)| name.starts_with("stm32") && name != feature);
        if !other_part {
            hash.write(line.as_bytes());
            hash.write(b"\n");
        }
    }
    Ok(format!(
        "{feature} metapac {} inputs {:016x}",
        locked_metapac(&root.join("Cargo.lock"))?,
        hash.0
    ))
}

/// Every file under `dir`, at any depth.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            sources(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// FNV-1a, 64 bits: a hash that is the same on every toolchain and host,
/// unlike std's, so a cache restored on another runner still matches.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

/// One part's metadata, from `glue-dump` built with that part's feature.
fn dumped(part: &Part) -> Result<Metadata, String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "--release",
            "--locked",
            "-p",
            "glue-dump",
            "--features",
        ])
        .arg(part.feature)
        .current_dir(tool_dir())
        .output()
        .map_err(|e| format!("running glue-dump: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "glue-dump for {} failed:\n{}",
            part.feature,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("{}: {e}", part.feature))?;
    if !metadata.name.eq_ignore_ascii_case(part.feature) {
        return Err(format!(
            "{}: metapac dumped {}, not the part asked for",
            part.feature, metadata.name
        ));
    }
    Ok(metadata)
}

/// The stm32-metapac version both lockfiles pin, or why they differ.
fn same_metapac() -> Result<String, String> {
    let library = locked_metapac(&tool_dir().join("../../Cargo.lock"))?;
    let tool = locked_metapac(&tool_dir().join("Cargo.lock"))?;
    if library == tool {
        Ok(library)
    } else {
        Err(format!(
            "the library pins stm32-metapac {library} and the generator {tool}: generate from the library's"
        ))
    }
}

fn locked_metapac(lockfile: &Path) -> Result<String, String> {
    let text = fs::read_to_string(lockfile).map_err(|e| format!("{}: {e}", lockfile.display()))?;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"stm32-metapac\""
            && let Some(version) = lines
                .next()
                .and_then(|l| l.trim().strip_prefix("version = "))
        {
            return Ok(version.trim_matches('"').to_string());
        }
    }
    Err(format!("{}: no stm32-metapac", lockfile.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_its_reference_values() {
        let hash = |text: &[u8]| {
            let mut fnv = Fnv::default();
            fnv.write(text);
            fnv.0
        };
        assert_eq!(hash(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(hash(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn a_dump_key_names_its_part_and_metapac() {
        let key = dump_key("stm32f401re").unwrap();
        assert!(key.starts_with("stm32f401re metapac "), "{key}");
        assert_ne!(key, dump_key("stm32f405rg").unwrap());
        assert!(!key.contains('\n'));
    }
}
