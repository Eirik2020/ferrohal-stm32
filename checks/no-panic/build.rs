//! Puts the chosen chip's memory map where the linker looks for `memory.x`.

use std::{env, fs, path::PathBuf};

fn main() {
    let memory = if env::var_os("CARGO_FEATURE_STM32H753ZI").is_some() {
        // AXI SRAM: DMA1 and DMA2 cannot reach the DTCM.
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 2048K\n RAM : ORIGIN = 0x24000000, LENGTH = 512K }\n"
    } else {
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 512K\n RAM : ORIGIN = 0x20000000, LENGTH = 96K }\n"
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), memory).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
}
