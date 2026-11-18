//! The chip selection in `src/lib.rs`, generated from the registry and the
//! chip files (ADR-012): the check that exactly one chip feature is on, and
//! which chip file each product line mounts as `chip`.
//!
//! A line's chip file is the one under `src/chips/<family>/` that mounts its
//! parts' generated glue (`#[path = "../gen/<part>.rs"]`). Every part of a
//! line must be mounted by the same file, and every line by one.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::registry::{PARTS, VARIANTS};

/// Each registered part's chip file, relative to `src/`: `chips/f4/stm32f405.rs`.
pub fn chip_files(root: &Path) -> Result<BTreeMap<&'static str, String>, String> {
    let chips = root.join("src/chips");
    let mut files: Vec<PathBuf> = Vec::new();
    for family in fs::read_dir(&chips).map_err(|e| format!("{}: {e}", chips.display()))? {
        let family = family.map_err(|e| e.to_string())?.path();
        if !family.is_dir() || family.file_name().is_some_and(|name| name == "gen") {
            continue;
        }
        for entry in fs::read_dir(&family).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut found = BTreeMap::new();
    for part in PARTS {
        let mount = format!("#[path = \"../gen/{}.rs\"]", part.feature);
        let mut owners = files
            .iter()
            .filter(|path| fs::read_to_string(path).is_ok_and(|text| text.contains(&mount)));
        let owner = owners
            .next()
            .ok_or_else(|| format!("{}: no chip file mounts gen/{0}.rs", part.feature))?;
        if let Some(other) = owners.next() {
            return Err(format!(
                "{}: mounted by both {} and {}",
                part.feature,
                owner.display(),
                other.display()
            ));
        }
        let relative = owner
            .strip_prefix(root.join("src"))
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        found.insert(part.feature, relative);
    }
    Ok(found)
}

/// The line feature of a part: `_line-stm32f405`.
fn line_feature(device: &str) -> String {
    format!("_line-{}", device.to_ascii_lowercase())
}

/// The two generated blocks: the chip-feature count, and the line mounts.
pub fn blocks(root: &Path) -> Result<(String, String), String> {
    let files = chip_files(root)?;
    let mut features = Vec::new();
    for part in PARTS {
        features.push(part.feature);
        features.extend(
            VARIANTS
                .iter()
                .filter(|(_, of)| *of == part.feature)
                .map(|(variant, _)| *variant),
        );
    }
    let mut count = String::from(
        "/// How many chip features are on: exactly one must be.\nconst CHIP_FEATURES: usize = ",
    );
    for (index, feature) in features.iter().enumerate() {
        if index > 0 {
            count += "\n    + ";
        }
        count += &format!("cfg!(feature = \"{feature}\") as usize");
    }
    count += ";\n";

    let mut lines: Vec<(&str, &String)> = Vec::new();
    for part in PARTS {
        let file = &files[part.feature];
        match lines.iter().find(|(device, _)| *device == part.device) {
            Some((_, seen)) if *seen != file => {
                return Err(format!(
                    "{}: its parts are mounted by both {seen} and {file}",
                    part.device
                ));
            }
            Some(_) => {}
            None => lines.push((part.device, file)),
        }
    }
    let mut mounts = String::new();
    for (device, file) in lines {
        mounts += &format!(
            "#[cfg(feature = \"{}\")]\n#[path = \"{file}\"]\npub mod chip;\n",
            line_feature(device)
        );
    }
    Ok((count, mounts))
}

/// Replaces the text between `// generated: glue-gen <name>` and
/// `// end generated`.
pub fn splice(text: &str, name: &str, block: &str) -> Result<String, String> {
    let open = format!("// generated: glue-gen {name}\n");
    let close = "// end generated";
    let start = text
        .find(&open)
        .ok_or_else(|| format!("src/lib.rs has no `{}` marker", open.trim()))?
        + open.len();
    let end = start
        + text[start..]
            .find(close)
            .ok_or("unclosed generated block")?;
    Ok(format!("{}{block}{}", &text[..start], &text[end..]))
}

/// Rewrites both blocks in `src/lib.rs`.
pub fn write(root: &Path) -> Result<(), String> {
    let (count, mounts) = blocks(root)?;
    let path = root.join("src/lib.rs");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = splice(&text, "chip-count", &count)?;
    let text = splice(&text, "chip-mounts", &mounts)?;
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_replaces_only_the_named_block() {
        let text = "a\n// generated: glue-gen one\nold\n// end generated\nb\n// generated: glue-gen two\nkeep\n// end generated\n";
        let out = splice(text, "one", "new\n").unwrap();
        assert_eq!(
            out,
            "a\n// generated: glue-gen one\nnew\n// end generated\nb\n// generated: glue-gen two\nkeep\n// end generated\n"
        );
        assert!(splice(text, "three", "x").is_err());
    }

    #[test]
    fn every_line_mounts_one_chip_file() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let (_, mounts) = blocks(&root).unwrap();
        let mut devices: Vec<&str> = PARTS.iter().map(|part| part.device).collect();
        devices.dedup();
        for device in devices {
            let cfg = format!("#[cfg(feature = \"{}\")]", line_feature(device));
            assert_eq!(mounts.matches(&cfg).count(), 1, "{device}");
        }
    }
}
