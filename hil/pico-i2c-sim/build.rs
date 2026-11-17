//! Puts the RP2040's memory map where the linker can find it: the
//! cortex-m-rt script the `-Tlink.x` flag passes INCLUDEs a `memory.x`,
//! and embassy-rp's `-Tlink-rp.x` places the boot second stage in the
//! BOOT2 region it defines.

use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let memory = out.join("memory.x");
    let mut file = File::create(&memory).unwrap();
    // The RP2040 boots through a 256-byte second-stage flash image; the
    // program follows it in the 2 MiB flash. 264 KiB of RAM.
    write!(
        file,
        "MEMORY\n{{\n    BOOT2 : ORIGIN = 0x10000000, LENGTH = 256\n    FLASH : ORIGIN = 0x10000100, LENGTH = 2048K\n    RAM : ORIGIN = 0x20000000, LENGTH = 264K\n}}\n"
    )
    .unwrap();
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
    // The commit the image is built from, for its FERRO-ID line.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let rev = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // --no-optional-locks: a plain `git status` rewrites the index when it
    // refreshes racily clean entries, and the index is watched below, so each
    // build would re-run this script and recompile the next one (measured
    // 2026-11-17: every H753 run recompiled its image, about 2 s each).
    let dirty = git(&["--no-optional-locks", "status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.is_empty());
    println!("cargo:rustc-env=FERRO_GIT={rev}{}", if dirty { "+dirty" } else { "" });
    for path in ["src", "Cargo.toml"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let top = git(&["rev-parse", "--absolute-git-dir"]).unwrap_or_default();
    for file in ["HEAD", "index", "logs/HEAD"] {
        println!("cargo:rerun-if-changed={top}/{file}");
    }
}
