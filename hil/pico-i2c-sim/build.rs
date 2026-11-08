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
}
