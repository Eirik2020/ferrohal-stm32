//! USB under RTIC, as the flight code runs it: `hil/common/usb_irq.rs`.

#![no_std]
#![no_main]

#[macro_use]
#[path = "../../../common/usb_bench.rs"]
mod usb_bench;
#[macro_use]
#[path = "../../../common/usb_irq.rs"]
mod usb_irq;
#[allow(dead_code)]
#[path = "../clocks.rs"]
mod clocks;

/// The endpoint memory `usb_bench` gives the driver.
#[path = "../ident.rs"]
mod ident;

const EP_WORDS: usize = 1024;

usb_irq_app!(board: "lucid-h7", iwdg: IWDG1, record: false);
