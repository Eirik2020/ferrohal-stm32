//! Puts the chosen chip's memory map where the linker looks for `memory.x`.

use std::{env, fs, path::PathBuf};

fn main() {
    let feature = |chip: &str| env::var_os(format!("CARGO_FEATURE_{chip}")).is_some();
    let memory = if feature("STM32H753ZI") || feature("STM32H743VI") {
        // AXI SRAM: DMA1 and DMA2 cannot reach the DTCM.
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 2048K\n RAM : ORIGIN = 0x24000000, LENGTH = 512K }\n"
    } else if feature("STM32F411CE") || feature("STM32F446RE") {
        // RM0383 2.3.1 and RM0390 2.2.2: 128 KiB of SRAM; the CE's and RE's
        // 512 KiB of flash.
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 512K\n RAM : ORIGIN = 0x20000000, LENGTH = 128K }\n"
    } else if feature("STM32F427VI") || feature("STM32F429ZI") {
        // RM0090 2.3: SRAM1 to SRAM3, 192 KiB; the xI's 2 MiB of flash.
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 2048K\n RAM : ORIGIN = 0x20000000, LENGTH = 192K }\n"
    } else if feature("STM32F405RG") {
        // SRAM1 and SRAM2: the DMA cannot reach the CCM RAM.
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 1024K\n RAM : ORIGIN = 0x20000000, LENGTH = 128K }\n"
    } else {
        "MEMORY { FLASH : ORIGIN = 0x08000000, LENGTH = 512K\n RAM : ORIGIN = 0x20000000, LENGTH = 96K }\n"
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("memory.x"), memory).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=build.rs");
}
