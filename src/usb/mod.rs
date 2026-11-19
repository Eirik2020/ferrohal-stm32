//! The full-speed USB OTG port as a device, for `usb-device` and the class
//! crates on top of it (`usbd-serial` for CDC): [`UsbFs`] gives
//! `synopsys-usb-otg` the chip's facts. Behind the `usb` feature.
//!
//! The controller's driver is `synopsys-usb-otg` 0.4.0 from a fork pinned by
//! revision. The fork makes it panic-free, with every wait on the core
//! bounded, so the bus and its vector service sit inside FerroHAL's no-panic
//! proof. It also carries two errata. ES0392 2.25.7: a zero-length IN
//! packet enabled with CNAK set can send a wrong packet on the H7's core, so
//! the fork sets SNAK, waits, enables the endpoint, waits, and only then
//! clears the NAK. The transmit FIFO erratum: another core register access
//! cutting into a FIFO write corrupts the FIFO (ES0182 2.14.1, ES0206 2.14.1,
//! ES0287 2.12.1, ES0298 2.16.1, ES0299 2.12.1, ES0305 2.16.1,
//! ES0372 2.16.1, ES0334 2.19.1, and ES0392 2.25.2 on revisions Y and W), and a
//! deferred vector reads and masks the core's interrupts at its own
//! priority, so the fork writes each IN packet inside a critical section.
//! The `usb-device` stack above the bus (enumeration, the
//! classes) is the firmware's protocol code, re-exported only so the firmware
//! builds against the same version; it is not FerroHAL's, and its own code
//! can panic.
//!
//! The firmware binds the vector in [`UsbFs::vectors`], or polls, and calls
//! `UsbDevice::poll` from one context; FerroHAL enables nothing in the NVIC.
//! Either the vector polls the device itself ([`UsbBus::new`]), or it defers
//! to a task: [`UsbFs::deferred`] gives the bus and a [`UsbVector`] whose
//! [`Service::on_interrupt`] masks what is pending and reports it, and the
//! task's next `UsbDevice::poll` serves it and unmasks it. The core's
//! start-of-frame interrupt stays on as a heartbeat, every millisecond on a
//! full-speed bus, repeating the report while anything still waits, so a
//! wake the scheduler dropped comes again rather than stalling the port.
//! The data pins are the chip's own (`chip::UsbDm`, `chip::UsbDp`, from the
//! glue); set both to very high speed and hand them over in a [`Wiring`].
//!
//! Requirements: USB-001, USB-F4-001, API-002

pub use synopsys_usb_otg::Waiting as Events;
pub use usb_device;
use usb_device::bus::UsbBusAllocator;

use crate::{
    chip::{
        USB_FS, USB_FS_ENDPOINTS, USB_FS_FIFO_WORDS, USB_FS_HS_CORE, USB_FS_INTERRUPT, USB_OTG_FS,
        UsbDm, UsbDp,
    },
    rcc::{ClockError, Rcc},
    resource::{Refused, Resource, Service, Signal},
};

/// The lowest AHB clock the OTG core works at: "the AHB frequency should be
/// higher than 14.2 MHz" (RM0368 22.3 and RM0090 34.3, cautions).
const MIN_HCLK: u32 = 14_200_000;

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
        Resource::pin::<UsbDm>(Signal::Dm),
        Resource::pin::<UsbDp>(Signal::Dp),
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
pub type UsbRefused = Refused<ClockError, Wiring>;

impl UsbFs {
    /// Starts the port's 48 MHz clock (the F4's PLLQ must already give it;
    /// the H7 turns on HSI48 and the USB supply) and enables the controller.
    /// Refuses with the wiring handed back: `ClockError::Hclk` for an AHB
    /// clock at or below 14.2 MHz, which the core does not work at.
    pub fn new(rcc: &mut Rcc, wiring: Wiring) -> Result<Self, UsbRefused> {
        if rcc.clocks().hclk.0 <= MIN_HCLK {
            return Err(Refused::new(ClockError::Hclk, wiring));
        }
        if let Err(error) = rcc.usb_clock() {
            return Err(Refused::new(error, wiring));
        }
        rcc.open(USB_FS.enable);
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

    /// The vector to bind to the handler that polls the device, or to the
    /// one that calls a [`UsbVector`]'s service.
    pub const fn vectors() -> crate::resource::Vectors {
        crate::resource::Vectors::one(USB_FS_INTERRUPT)
    }

    /// Builds the bus for a vector that defers to a task: the bus, for
    /// `usb-device`, and the [`UsbVector`] the vector serves. `ep_memory`
    /// holds the OUT endpoints' packets, as for [`UsbBus::new`].
    pub fn deferred(self, ep_memory: &'static mut [u32]) -> (UsbBusAllocator<UsbBus>, UsbVector) {
        let (bus, handle) = UsbBus::new_deferred(self, ep_memory);
        (bus, UsbVector { handle })
    }

    /// As [`UsbFs::deferred`], with the start-of-frame heartbeat masked: for
    /// a task woken by something that holds a wake until the task reads it,
    /// so that no report needs repeating. A quiet configured bus then raises
    /// the vector not at all, instead of once a frame. PROTOTYPE.
    pub fn deferred_without_heartbeat(
        self,
        ep_memory: &'static mut [u32],
    ) -> (UsbBusAllocator<UsbBus>, UsbVector) {
        let (bus, handle) = UsbBus::deferred_bus_without_heartbeat(self, ep_memory);
        (UsbBusAllocator::new(bus), UsbVector { handle })
    }
}

/// The vector's half of a deferred bus, from [`UsbFs::deferred`]. It owns
/// nothing the bus needs: it reads the core's interrupt status and masks
/// what it found, and the bus's `poll` unmasks it inside a critical section.
pub struct UsbVector {
    handle: synopsys_usb_otg::InterruptHandle<UsbFs>,
}

impl Service for UsbVector {
    type Event = Events;

    /// Masks what is pending until the next `UsbDevice::poll`, clears a
    /// start of frame, and reports everything waiting for that poll, masked
    /// or not. A report with [`Events::any`] is the task's cue to poll.
    fn on_interrupt(&mut self) -> Events {
        self.handle.on_interrupt()
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
