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
