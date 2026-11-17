//! This image's identity (`hil/RIG.md`, Identity): `usb_bench` answers
//! `FERRO-ID?` with [`LINE`] and the die it reads. The board has no probe,
//! so USB serial is the only way to ask.

pub const LINE: &str = concat!(
    "FERRO-ID v=1 board=foxeer-f405-v2 part=STM32F405RG crate=",
    env!("CARGO_PKG_NAME"),
    " run=",
    env!("CARGO_BIN_NAME"),
    " git=",
    env!("FERRO_GIT"),
);

/// `DBGMCU`'s DEV_ID.
pub fn dev_id() -> u16 {
    stm32_metapac::DBGMCU.idcode().read().dev_id()
}

/// Where a run leaves its stop record (`usb_bench::record_stop`), if anywhere.
/// The top 32 bytes of SRAM, which `memory.x` keeps out of RAM: the ROM
/// bootloader uses 8 to 16 KiB from 0x2000_0000 (AN2606, STM32F40xxx/41xxx), and a
/// reset leaves SRAM as it was.
pub const STOP_RECORD: Option<usize> = Some(0x2001_FFE0);
