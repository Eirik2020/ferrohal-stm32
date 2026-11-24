//! Hands the bench image the commit it is built from, for its identity
//! block (`hil/common/ident.rs`): `FERRO_GIT`, the short revision, with
//! `+dirty` when tracked files differ from it.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    // Candidate A: embedded-test's linker script, for the test targets only.
    println!("cargo::rustc-link-arg-tests=-Tembedded-test.x");
    println!("cargo::rustc-check-cfg=cfg(rust_analyzer)");
    // board.rs's identity block names the image by CARGO_BIN_NAME, which cargo
    // sets for binaries only, not for a test target.
    println!("cargo:rustc-env=CARGO_BIN_NAME=embedded-test");
    let rev = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    // --no-optional-locks: a plain `git status` rewrites the index when it
    // refreshes racily clean entries, and the index is watched below, so each
    // build would re-run this script and recompile the next one (measured
    // 2026-11-17: every H753 run recompiled its image, about 2 s each).
    let dirty = git(&["--no-optional-locks", "status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|s| !s.is_empty());
    println!("cargo:rustc-env=FERRO_GIT={rev}{}", if dirty { "+dirty" } else { "" });
    for path in ["memory.x", "src", "Cargo.toml", "../common", "../../src", "../../Cargo.toml"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let top = git(&["rev-parse", "--absolute-git-dir"]).unwrap_or_default();
    for file in ["HEAD", "index", "logs/HEAD"] {
        println!("cargo:rerun-if-changed={top}/{file}");
    }
}
