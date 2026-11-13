//! The chip-glue generator (ADR-008, `docs/design/glue-generator.md`).
//!
//!     cargo run --manifest-path tools/glue-gen/Cargo.toml -- <command> <selector>
//!
//! Commands: `parts` lists the parts a selector names; `dump` reads each
//! one's metadata through `glue-dump` and writes it to
//! `tools/glue-gen/target/parts/<part>.json`. Selectors: `all`,
//! `series F4`, `die 0x413` (or a line cut from it, `die STM32F407`),
//! `part stm32f405rg`.
//!
//! Before anything, the tool checks that its stm32-metapac is the one the
//! library's `Cargo.lock` pins, so the glue is generated from the metadata
//! the library builds against.

mod registry;

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
        .ok_or("usage: glue-gen <parts | dump> <selector>")?;
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
        other => Err(format!("unknown command {other}: parts or dump")),
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
