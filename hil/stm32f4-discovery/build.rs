//! Hands the bench image the commit it is built from, for its identity
//! block (`hil/common/ident.rs`): `FERRO_GIT`, the short revision, with
//! `+dirty` when tracked files differ from it.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let rev = git(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    println!("cargo:rustc-env=FERRO_GIT={rev}{}", if dirty { "+dirty" } else { "" });
    for path in ["memory.x", "src", "Cargo.toml", "../common", "../../src", "../../Cargo.toml"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let top = git(&["rev-parse", "--absolute-git-dir"]).unwrap_or_default();
    for file in ["HEAD", "index", "logs/HEAD"] {
        println!("cargo:rerun-if-changed={top}/{file}");
    }
}
