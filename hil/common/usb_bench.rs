//! The bench for a flight controller with no probe: everything goes through
//! its USB port, to `hil/usb_cdc_host.py`.
//!
//! Commands from the host, each an 8-byte marker: `BENCHRUN` runs the checks
//! and reports `PASS` and `FAIL` lines, ending `DONE`; `BENCHECH` echoes until
//! `BENCHEND`; `BENCHDFU` restarts into the ROM DFU bootloader.
//!
//! The board has no other way back but its BOOT button, so a run that goes
//! wrong restarts into the bootloader too: a panic, a fault, a watchdog reset
//! (3 s without the main loop), clocks that don't start, or no host
//! configuring the device within 30 s. That the board comes up in DFU rather
//! than as a serial port is the report, for those.

// Each board compiles this file and uses what it needs.
#![allow(dead_code)]

use core::{
    fmt::{self, Write as _},
    panic::PanicInfo,
    ptr::{addr_of_mut, read_volatile},
};

use cortex_m::peripheral::DWT;
use cortex_m_rt::{ExceptionFrame, exception};
use ferrohal_stm32::{
    iwdg::Watchdog,
    reset::{self, ResetCause},
    signature,
    timer::Timebase,
    usb::{
        UsbBus, UsbFs,
        usb_device::{
            bus::UsbBusAllocator,
            device::{StringDescriptors, UsbDeviceBuilder, UsbDeviceState, UsbVidPid},
            prelude::UsbDevice,
        },
    },
};
use usbd_serial::SerialPort;

pub use ferrohal_stm32::reset::reboot_to_bootloader as reboot_to_dfu;

macro_rules! check {
    ($report:expr, $link:expr, $ok:expr, $($arg:tt)*) => {
        $report.check($link, $ok, format_args!($($arg)*))
    };
}

const RUN: &[u8; 8] = b"BENCHRUN";
const ECHO: &[u8; 8] = b"BENCHECH";
const END: &[u8; 8] = b"BENCHEND";
const DFU: &[u8; 8] = b"BENCHDFU";

pub const WATCHDOG_US: u32 = 3_000_000;
pub const CONFIGURED_PATIENCE_US: u32 = 30_000_000;
const SEND_PATIENCE_US: u32 = 2_000_000;

static mut EP_MEMORY: [u32; crate::EP_WORDS] = [0; crate::EP_WORDS];
static mut BUS: Option<UsbBusAllocator<UsbBus>> = None;

/// Enters the ROM bootloader if the last reset asked for it or was the
/// watchdog's. First thing in `main`.
pub fn enter_bootloader_if_requested() {
    reset::enter_bootloader_if_requested();
    if reset::peek_cause().independent_watchdog {
        // SAFETY: nothing is set up yet.
        unsafe { reset::enter_bootloader() }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    reboot_to_dfu()
}

#[exception]
unsafe fn HardFault(_: &ExceptionFrame) -> ! {
    reboot_to_dfu()
}

/// The USB port and what has to keep running while anything else waits.
pub struct Link {
    device: UsbDevice<'static, UsbBus>,
    serial: SerialPort<'static, UsbBus>,
    watchdog: Watchdog,
    clock: Timebase,
}

impl Link {
    /// Builds the device on `usb`, named so the host can tell boards apart.
    /// Call it once.
    pub fn new(
        usb: UsbFs,
        serial_number: &'static str,
        watchdog: Watchdog,
        clock: Timebase,
    ) -> Self {
        // SAFETY: called once, and these are the only references.
        let bus: &'static UsbBusAllocator<UsbBus> = unsafe {
            let memory = &mut *addr_of_mut!(EP_MEMORY);
            (*addr_of_mut!(BUS)).insert(UsbBus::new(usb, memory))
        };
        let serial = SerialPort::new(bus);
        let Ok(builder) = UsbDeviceBuilder::new(bus, UsbVidPid(0x16c0, 0x27dd)).strings(&[
            StringDescriptors::default()
                .manufacturer("FerroHAL")
                .product("FerroHAL bench")
                .serial_number(serial_number),
        ]) else {
            reboot_to_dfu()
        };
        let device = builder.device_class(usbd_serial::USB_CLASS_CDC).build();
        Self {
            device,
            serial,
            watchdog,
            clock,
        }
    }

    /// Microseconds, from the timebase.
    pub fn now(&self) -> u32 {
        self.clock.now()
    }

    pub fn service(&mut self) {
        self.watchdog.feed();
        self.device.poll(&mut [&mut self.serial]);
    }

    /// Whether a host has configured the device.
    pub fn configured(&self) -> bool {
        self.device.state() == UsbDeviceState::Configured
    }

    /// One go at the device from its interrupt: echoes what's there, and
    /// restarts into the bootloader on `BENCHDFU`. For a firmware whose only
    /// service of USB is its interrupt. A host that stops reading loses the
    /// rest of the packet, once the bound on waiting for it runs out.
    pub fn echo_from_interrupt(&mut self, tail: &mut [u8; 8]) {
        if !self.device.poll(&mut [&mut self.serial]) {
            return;
        }
        let mut pending = [0u8; 64];
        let Ok(count) = self.serial.read(&mut pending) else {
            return;
        };
        for &byte in pending.get(..count).unwrap_or(&[]) {
            tail.rotate_left(1);
            tail[7] = byte;
            if tail == DFU {
                reboot_to_dfu();
            }
        }
        let mut sent = 0;
        let mut tries = 200_000u32;
        while sent < count && tries > 0 {
            tries -= 1;
            match self.serial.write(pending.get(sent..count).unwrap_or(&[])) {
                Ok(written) => sent += written,
                Err(_) => {
                    self.device.poll(&mut [&mut self.serial]);
                }
            }
        }
    }

    pub fn settle(&mut self, us: u32) {
        let start = self.now();
        while self.now().wrapping_sub(start) < us {
            self.service();
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let start = self.now();
        let mut sent = 0;
        while sent < bytes.len() && self.now().wrapping_sub(start) < SEND_PATIENCE_US {
            self.service();
            if let Ok(count) = self.serial.write(&bytes[sent..]) {
                sent += count;
            }
        }
    }

    pub fn line(&mut self, args: fmt::Arguments) {
        let mut line = Line {
            bytes: [0; 160],
            len: 0,
        };
        let _ = line.write_fmt(args);
        let _ = line.write_str("\n");
        let len = line.len;
        self.send(&line.bytes[..len]);
    }
}

/// One report line, cut short rather than refused if it doesn't fit.
struct Line {
    bytes: [u8; 160],
    len: usize,
}

impl fmt::Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for &byte in text.as_bytes() {
            if self.len < self.bytes.len() {
                self.bytes[self.len] = byte;
                self.len += 1;
            }
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    pub fn check(&mut self, link: &mut Link, ok: bool, args: fmt::Arguments) {
        if ok {
            self.passed += 1;
            link.line(format_args!("PASS {args}"));
        } else {
            self.failed += 1;
            link.line(format_args!("FAIL {args}"));
        }
    }
}

/// Whether `got` is within `hundredths` of a percent of `want`.
pub fn close(got: u32, want: u32, hundredths: u32) -> bool {
    u64::from(got.abs_diff(want)) * 10_000 <= u64::from(want) * u64::from(hundredths)
}

/// The most specific name for what `cause` holds.
pub fn cause_text(cause: ResetCause) -> &'static str {
    if cause.power_on {
        "power on"
    } else if cause.software {
        "software"
    } else if cause.brownout {
        "brownout"
    } else if cause.pin {
        "reset pin"
    } else {
        "other"
    }
}

/// The chip's unique ID, for the log.
pub fn uid_suite(report: &mut Report, link: &mut Link) {
    let uid = signature::device_uid();
    check!(
        report,
        link,
        uid != [0; 12] && uid != [0xFF; 12],
        "uid: {:02x?}",
        uid
    );
}

/// The CPU's cycles counted against the host's 1 kHz start-of-frames, the one
/// clock here that doesn't come from the crystal, then the timebase against
/// the CPU. `dsts` is the USB core's device status register.
pub fn clock_suite(report: &mut Report, link: &mut Link, dsts: *const u32, cpu_hz: u32) {
    // SAFETY: a read of the device status register, whose frame number the
    // USB driver doesn't use. Full speed numbers its frames in 11 bits.
    let frame = || unsafe { (read_volatile(dsts) >> 8) & 0x7FF };
    const FRAMES: u32 = 1000;
    let begun = link.now();
    let first = frame();
    let mut cycles = None;
    let mut start_cycles = 0;
    let mut started = false;
    while link.now().wrapping_sub(begun) < 2_000_000 {
        link.service();
        let now = frame();
        if !started && now != first {
            started = true;
            start_cycles = DWT::cycle_count();
        } else if started && now.wrapping_sub(first) & 0x7FF > FRAMES {
            cycles = Some(DWT::cycle_count().wrapping_sub(start_cycles));
            break;
        }
    }
    let measured = cycles.unwrap_or(0);
    check!(
        report,
        link,
        close(measured, cpu_hz, 10),
        "clocks: {} CPU cycles per 1000 host frames, want {} within 0.1%",
        measured,
        cpu_hz
    );

    let cycle_start = DWT::cycle_count();
    link.settle(100_000);
    let elapsed = DWT::cycle_count().wrapping_sub(cycle_start);
    check!(
        report,
        link,
        close(elapsed, cpu_hz / 10, 10),
        "clocks: {} CPU cycles per 100 ms of the timebase, want {} within 0.1%",
        elapsed,
        cpu_hz / 10
    );
}

/// Waits for and answers commands, calling `run` for `BENCHRUN`.
pub fn serve(link: &mut Link, cause: &str, mut run: impl FnMut(&mut Report, &mut Link)) -> ! {
    let boot = link.now();
    let mut configured = false;
    let mut echoing = false;
    let mut tail = [0u8; 8];
    let mut pending = [0u8; 64];
    let (mut head, mut len) = (0, 0);
    loop {
        link.service();
        if !configured {
            if link.device.state() == UsbDeviceState::Configured {
                configured = true;
            } else if link.now().wrapping_sub(boot) > CONFIGURED_PATIENCE_US {
                reboot_to_dfu();
            }
        }
        if len == 0 {
            head = 0;
            if let Ok(count) = link.serial.read(&mut pending) {
                len = count;
            }
        }
        if echoing {
            if len > 0
                && let Ok(count) = link.serial.write(&pending[head..head + len])
            {
                for &byte in &pending[head..head + count] {
                    tail.rotate_left(1);
                    tail[7] = byte;
                    echoing &= tail != *END;
                }
                head += count;
                len -= count;
            }
            continue;
        }
        for &byte in &pending[head..head + len] {
            tail.rotate_left(1);
            tail[7] = byte;
            if tail == *DFU {
                // Long enough for the host to let go of the port.
                link.settle(100_000);
                reboot_to_dfu();
            }
            if tail == *ECHO {
                echoing = true;
            }
            if tail == *RUN {
                let mut report = Report::default();
                link.line(format_args!("INFO reset cause: {cause}"));
                run(&mut report, link);
                link.line(format_args!(
                    "DONE {} passed, {} failed",
                    report.passed, report.failed
                ));
            }
        }
        len = 0;
    }
}
