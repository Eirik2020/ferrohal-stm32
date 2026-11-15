//! This image's identity (`hil/RIG.md`, Identity): `usb_bench` answers
//! `FERRO-ID?` with [`LINE`] and the die it reads. The board has no probe,
//! so USB serial is the only way to ask.

pub const LINE: &str = concat!(
    "FERRO-ID v=1 board=lucid-h7 part=STM32H743VI crate=",
    env!("CARGO_PKG_NAME"),
    " run=",
    env!("CARGO_BIN_NAME"),
    " git=",
    env!("FERRO_GIT"),
);

/// `DBGMCU`'s DEV_ID.
pub fn dev_id() -> u16 {
    stm32_metapac::DBGMCU.idc().read().dev_id()
}
