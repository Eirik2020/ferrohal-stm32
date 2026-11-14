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
mod probe;
mod registry;
mod silicon;
mod tables;

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
    let (command, selector) = args
        .split_first()
        .ok_or("usage: glue-gen <parts | dump | model | emit | tables | probe> <selector>")?;
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
        other => Err(format!(
            "unknown command {other}: parts, dump, model, emit, tables or probe"
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

/// One part's metadata, from `glue-dump` built with that part's feature.
fn dump(part: &Part) -> Result<Metadata, String> {
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
