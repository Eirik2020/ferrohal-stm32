//! The full-speed USB OTG port as a device, for `usb-device` and the class
//! crates on top of it (`usbd-serial` for CDC): [`UsbFs`] gives
//! `synopsys-usb-otg` the chip's facts. Behind the `usb` feature.
//!
//! The controller's driver is third-party, and it can panic: USB sits
//! outside FerroHAL's no-panic proof, apart from [`UsbFs::new`]. It is
//! `synopsys-usb-otg` 0.4.0 from a fork pinned by revision, which adds one
//! erratum: ES0392 2.25.7, a zero-length IN packet enabled with CNAK set can
//! send a wrong packet on the H7's core, so the fork sets SNAK, waits, enables
//! the endpoint, waits, and only then clears the NAK.
//!
//! The firmware binds [`UsbFs::interrupt`]'s vector, or polls, and calls
//! `UsbDevice::poll` from one context; FerroHAL enables nothing in the NVIC.
//! The data pins are the chip's own (`chip::UsbDm`, `chip::UsbDp`, from the
//! glue); set both to very high speed and hand them over in a [`Wiring`].
//!
//! Requirements: USB-001, USB-F4-001, API-002

pub use usb_device;

use crate::{
    chip::{
        Interrupt, USB_FS, USB_FS_ENDPOINTS, USB_FS_FIFO_WORDS, USB_FS_HS_CORE, USB_FS_INTERRUPT,
        USB_OTG_FS, UsbDm, UsbDp,
    },
    gpio::AltPin,
    rcc::{ClockError, Rcc},
    resource::{Refused, Resource, Signal},
};

/// The port's bus type, for `UsbDeviceBuilder` and the class crates.
pub type UsbBus = synopsys_usb_otg::UsbBus<UsbFs>;

/// What the board file hands to [`UsbFs::new`]: the OTG_FS token and its
/// two data pins, which the chip's glue names.
pub struct Wiring {
    pub usb: USB_OTG_FS,
    pub dm: UsbDm,
    pub dp: UsbDp,
}

impl Wiring {
    /// Everything this wiring uses, as data: the port and its two pins with
    /// their alternate functions.
    pub const RESOURCES: [Resource; 3] = [
        Resource::Peripheral("USB_OTG_FS"),
        Resource::Pin {
            signal: Signal::Dm,
            port: <UsbDm as AltPin>::PORT,
            pin: <UsbDm as AltPin>::PIN,
            af: <UsbDm as AltPin>::AF,
        },
        Resource::Pin {
            signal: Signal::Dp,
            port: <UsbDp as AltPin>::PORT,
            pin: <UsbDp as AltPin>::PIN,
            af: <UsbDp as AltPin>::AF,
        },
    ];
}

/// The OTG_FS port, with its clock running. It holds its [`Wiring`], which
/// [`free`](Self::free) hands back.
pub struct UsbFs {
    wiring: Wiring,
    hclk: u32,
}

/// What [`UsbFs::new`](UsbFs::new) hands back when it refuses: the reason,
/// and the wiring it took.
pub type UsbNewRefused = Refused<ClockError, Wiring>;

impl UsbFs {
    /// Starts the port's 48 MHz clock (the F4's PLLQ must already give it;
    /// the H7 turns on HSI48 and the USB supply) and enables the controller.
    /// Refuses with the wiring handed back.
    pub fn new(rcc: &mut Rcc, wiring: Wiring) -> Result<Self, UsbNewRefused> {
        if let Err(error) = rcc.usb_clock() {
            return Err(Refused::new(error, wiring));
        }
        rcc.enable(USB_FS.enable);
        rcc.reset(USB_FS.enable);
        Ok(Self {
            wiring,
            hclk: rcc.clocks().hclk.0,
        })
    }

    /// Hands back what [`UsbFs::new`] took: the wiring. The core stays
    /// clocked, as `new` left it, and a reopen through `UsbFs::new` resets
    /// it. A port already built into a [`UsbBus`] comes back through the
    /// bus's own `free()`; one inside a `usb-device` `UsbBusAllocator` never
    /// does, since that crate keeps its bus for good.
    pub fn free(self) -> Wiring {
        self.wiring
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
