//! The full-speed USB OTG port as a device, for `usb-device` and the class
//! crates on top of it (`usbd-serial` for CDC): [`UsbFs`] gives
//! `synopsys-usb-otg` the chip's facts. Behind the `usb` feature.
//!
//! The controller's driver is third-party, and it can panic: USB sits
//! outside FerroHAL's no-panic proof, apart from [`UsbFs::new`].
//!
//! The firmware binds [`UsbFs::interrupt`]'s vector, or polls, and calls
//! `UsbDevice::poll` from one context; FerroHAL enables nothing in the NVIC.
//! Set both pins to very high speed.

pub use usb_device;

use crate::{
    chip::{
        Interrupt, USB_FS, USB_FS_ENDPOINTS, USB_FS_FIFO_WORDS, USB_FS_HS_CORE, USB_FS_INTERRUPT,
        USB_OTG_FS,
    },
    gpio::{Alternate, Pin},
    rcc::{ClockError, Rcc},
};

/// The port's bus type, for `UsbDeviceBuilder` and the class crates.
pub type UsbBus = synopsys_usb_otg::UsbBus<UsbFs>;

/// The OTG_FS port on PA11 (DM) and PA12 (DP), with its clock running.
pub struct UsbFs {
    hclk: u32,
}

impl UsbFs {
    /// Starts the port's 48 MHz clock (the F4's PLLQ must already give it;
    /// the H7 turns on HSI48 and the USB supply) and enables the controller.
    pub fn new(
        rcc: &mut Rcc,
        _token: USB_OTG_FS,
        _dm: Pin<'A', 11, Alternate<10>>,
        _dp: Pin<'A', 12, Alternate<10>>,
    ) -> Result<Self, ClockError> {
        rcc.usb_clock()?;
        rcc.enable(USB_FS.enable);
        rcc.reset(USB_FS.enable);
        Ok(Self {
            hclk: rcc.clocks().hclk.0,
        })
    }

    /// The vector to bind to the handler that polls the device.
    pub const fn interrupt() -> Interrupt {
        USB_FS_INTERRUPT
    }
}

// SAFETY: the base address, FIFO depth and endpoint count are the chip's own
// OTG_FS block (RM0090 and RM0368: 1.25 KiB, RM0433: 4 KiB and 9 endpoints);
// `new` has enabled its clock before the bus is built.
unsafe impl synopsys_usb_otg::UsbPeripheral for UsbFs {
    const REGISTERS: *const () = USB_FS.registers;
    /// The controller's kind, not the port's speed: the H7's is
    /// high-speed-capable, and must be told to use its internal full-speed
    /// PHY (the default `phy_type`), or its core reset waits for an ULPI
    /// clock that isn't there. The crate's `hs` feature, on for the H7 chips,
    /// does that.
    const HIGH_SPEED: bool = USB_FS_HS_CORE;
    const FIFO_DEPTH_WORDS: usize = USB_FS_FIFO_WORDS;
    const ENDPOINT_COUNT: usize = USB_FS_ENDPOINTS;

    /// Already done, through `Rcc`, by [`UsbFs::new`].
    fn enable() {}

    fn ahb_frequency_hz(&self) -> u32 {
        self.hclk
    }
}
