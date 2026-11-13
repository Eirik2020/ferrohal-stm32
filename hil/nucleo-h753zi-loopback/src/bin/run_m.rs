//! Run M: USB CDC through the user USB connector, CN13, with
//! `hil/usb_cdc_host.py` on the host sending blocks and checking every byte
//! echoed. Needs a USB cable from CN13 to the host.
//!
//! The firmware polls the device from its main loop and echoes what arrives,
//! then reports over RTT once the host sends the end marker: that the host
//! configured the device, and how many bytes went through.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use core::ptr::addr_of_mut;

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    chip::Peripherals,
    time::Hertz,
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
use panic_probe as _;
use usbd_serial::SerialPort;

use tests::{Fail, Report};

const END: &[u8] = b"BENCHEND";
/// Seconds to wait for the host at each stage.
const PATIENCE_US: u32 = 30_000_000;

static mut EP_MEMORY: [u32; 1024] = [0; 1024];
static mut BUS: Option<UsbBusAllocator<UsbBus>> = None;

#[entry]
fn main() -> ! {
    let mut core = cortex_m::Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let p = Peripherals::take().unwrap_or_else(|| defmt::unreachable!());
    let mut run = match board::run_m(p) {
        Ok(run) => run,
        Err(error) => defmt::panic!("clocks: {}", error),
    };
    tests::set_cpu_hz(board::CLOCKS.cpu.0);
    defmt::info!("run M, clocks: {}", board::CLOCKS);
    let Ok(timebase) = Timebase::new(&mut run.rcc, run.tim2, Hertz::mhz(1)) else {
        defmt::panic!("TIM2 timebase")
    };
    let clock = timebase.reader();
    let usb = match UsbFs::new(&mut run.rcc, run.usb, run.dm, run.dp) {
        Ok(usb) => usb,
        Err(error) => defmt::panic!("USB clock: {}", error),
    };
    // Freed to its token and pins, and reopened from them: the echo below
    // runs on the reopened port.
    let (token, dm, dp) = usb.free();
    let (reopened, usb) = match UsbFs::new(&mut run.rcc, token, dm, dp) {
        Ok(usb) => (true, usb),
        Err(error) => defmt::panic!("USB reopen: {}", error),
    };
    // SAFETY: `main` runs once, and these are the only references.
    let bus: &'static UsbBusAllocator<UsbBus> = unsafe {
        let memory = &mut *addr_of_mut!(EP_MEMORY);
        (*addr_of_mut!(BUS)).insert(UsbBus::new(usb, memory))
    };
    let mut serial = SerialPort::new(bus);
    let Ok(builder) = UsbDeviceBuilder::new(bus, UsbVidPid(0x16c0, 0x27dd)).strings(&[
        StringDescriptors::default()
            .manufacturer("FerroHAL")
            .product("FerroHAL CDC bench")
            .serial_number("h753zi"),
    ]) else {
        defmt::panic!("USB strings")
    };
    let mut device: UsbDevice<UsbBus> = builder.device_class(usbd_serial::USB_CLASS_CDC).build();

    let mut report = Report::new();
    let name = "USB CDC CN13";
    report.check(
        name,
        "a freed port reopens from its token and pins, and the echo runs on it [API-002]",
        if reopened {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 1,
                got: 0,
            })
        },
    );
    // For the log only: the controller doesn't need VBUS to run.
    let vbus = embedded_hal::digital::InputPin::is_high(&mut run.vbus).unwrap_or(false);
    defmt::info!("{=str}: VBUS on PA9: {=bool}", name, vbus);
    let start = clock.now();
    let mut configured_at = None;
    let mut received = 0u32;
    let mut echoed = 0u32;
    let mut tail = [0u8; 8];
    let mut pending = [0u8; 64];
    let (mut head, mut len) = (0, 0);
    let mut ended = false;
    while clock.now().wrapping_sub(start) < PATIENCE_US && !ended {
        device.poll(&mut [&mut serial]);
        if configured_at.is_none() && device.state() == UsbDeviceState::Configured {
            configured_at = Some(clock.now().wrapping_sub(start));
        }
        if len == 0 {
            head = 0;
            if let Ok(count) = serial.read(&mut pending) {
                len = count;
                received += count as u32;
                for &byte in &pending[..count] {
                    tail.rotate_left(1);
                    tail[7] = byte;
                    if tail == *END {
                        ended = true;
                    }
                }
            }
        }
        if len > 0
            && let Ok(count) = serial.write(&pending[head..head + len])
        {
            head += count;
            len -= count;
            echoed += count as u32;
        }
    }
    report.check(
        name,
        "the host configured the device",
        if configured_at.is_some() {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: 1,
                got: 0,
            })
        },
    );
    report.check(
        name,
        "the host finished, every byte echoed but the marker",
        if ended && echoed + 8 >= received {
            Ok(())
        } else {
            Err(Fail::Count {
                expected: received,
                got: echoed,
            })
        },
    );
    defmt::info!(
        "{=str}: configured after {=u32} ms, {=u32} bytes received, {=u32} echoed",
        name,
        configured_at.unwrap_or(0) / 1000,
        received,
        echoed
    );
    let led = &mut run.led;
    tests::finish(&report, || led.set_high());
}
