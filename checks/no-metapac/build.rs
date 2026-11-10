//! Puts the H743's memory map where the linker looks for `memory.x`. The
//! probes are never flashed, but a probe that wrongly compiles must link
//! cleanly, so the runner's verdict is about the API, not the linker.

use std::{env, fs, path::PathBuf};

fn main() {
    // AXI SRAM: DMA1 and DMA2 cannot reach the DTCM.
    let memory = "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 2048K\n RAM : ORIGIN = 0x24000000, LENGTH = 512K }\n";
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), memory).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
}
