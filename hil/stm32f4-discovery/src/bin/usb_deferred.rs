//! Run E: the F4's USB core with its vector deferred (USB-002) on CN5, under
//! the rule of use that the transmit FIFO erratum (ES0182 2.14.1) sets: the
//! vector's service and every bus call never preempt one another. The bus is
//! built without the start-of-frame heartbeat
//! (`UsbFs::deferred_without_heartbeat`); the vector, the device and its CDC
//! class sit in one shared resource, so the vector serves the core only by
//! taking the same lock the echo task holds for each pass. The echo task
//! waits on a signal the vector writes, and `hil/usb_cdc_host.py` checks
//! every byte it sends back.
//!
//! | Vector | Serves | Priority |
//! | --- | --- | --- |
//! | `OTG_FS` | `on_usb`: the vector's service, under the lock | 2 |
//! | `SPI1` | RTIC, dispatching `echo` | 1 |
//!
//! `idle` judges the run against the TIM2 timebase.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use ferrohal_stm32::usb::{
    UsbBus, UsbVector,
    usb_device::{device::UsbDevice, prelude::UsbDeviceState},
};
use panic_probe as _;
use usbd_serial::SerialPort;

const END: &[u8] = b"BENCHEND";
/// Seconds to wait for the host to finish.
const PATIENCE_US: u32 = 30_000_000;

/// The vector's service, the device and its one class: all behind one lock.
pub struct Stack {
    vector: UsbVector,
    device: UsbDevice<'static, UsbBus>,
    serial: SerialPort<'static, UsbBus>,
}

/// What the echo task has moved.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    configured: bool,
    received: u32,
    echoed: u32,
    ended: bool,
    /// The longest one pass held the lock, in CPU cycles.
    hold_max: u32,
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use cortex_m::peripheral::DWT;
    use ferrohal_stm32::{
        Service,
        chip::TIM2,
        time::Hertz,
        timer::{Timebase, TimebaseHandle},
        usb::{
            UsbBus, UsbFs, Wiring,
            usb_device::{
                bus::UsbBusAllocator,
                device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid},
            },
        },
    };
    use rtic_sync::signal::{Signal, SignalReader, SignalWriter};
    use usbd_serial::SerialPort;

    use super::{END, PATIENCE_US, Stack, Stats, UsbDeviceState};
    use crate::{
        board,
        tests::{self, Fail, Report},
    };

    static WAKE: Signal<()> = Signal::new();

    #[shared]
    struct Shared {
        usb: Stack,
        stats: Stats,
    }

    #[local]
    struct Local {
        wake: SignalWriter<'static, ()>,
        waiter: SignalReader<'static, ()>,
        clock: TimebaseHandle,
        /// Kept running for `clock`.
        timebase: Timebase<TIM2>,
        led: board::Led,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let mut run = match board::run_b(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        defmt::info!("run E, clocks: {}", board::CLOCKS);
        let Ok(timebase) = Timebase::new(&mut run.rcc, run.tim2, Hertz::mhz(1)) else {
            defmt::panic!("TIM2 timebase")
        };
        let clock = timebase.handle();
        let wiring = Wiring {
            usb: run.usb,
            dm: run.dm,
            dp: run.dp,
        };
        let usb = match UsbFs::new(&mut run.rcc, wiring) {
            Ok(usb) => usb,
            Err(error) => defmt::panic!("USB clock: {}", error),
        };
        let memory = cortex_m::singleton!(: [u32; 1024] = [0; 1024])
            .unwrap_or_else(|| defmt::panic!("endpoint memory"));
        let (bus, vector) = usb.deferred_without_heartbeat(memory);
        let bus: &'static UsbBusAllocator<UsbBus> =
            cortex_m::singleton!(: UsbBusAllocator<UsbBus> = bus)
                .unwrap_or_else(|| defmt::panic!("bus"));
        let serial = SerialPort::new(bus);
        let Ok(builder) = UsbDeviceBuilder::new(bus, UsbVidPid(0x16c0, 0x27dd)).strings(&[
            StringDescriptors::default()
                .manufacturer("FerroHAL")
                .product("FerroHAL CDC bench")
                .serial_number("f407vg-deferred"),
        ]) else {
            defmt::panic!("USB strings")
        };
        let device = builder.device_class(usbd_serial::USB_CLASS_CDC).build();
        let (wake, waiter) = WAKE.split();
        echo::spawn().ok();
        (
            Shared {
                usb: Stack {
                    vector,
                    device,
                    serial,
                },
                stats: Stats::default(),
            },
            Local {
                wake,
                waiter,
                clock,
                timebase,
                led: run.led,
            },
        )
    }

    /// The vector's service: takes the stack's lock, as every bus call does,
    /// masks what is pending and wakes the echo task.
    #[task(binds = OTG_FS, priority = 2, shared = [usb], local = [wake])]
    fn on_usb(mut cx: on_usb::Context) {
        if cx.shared.usb.lock(|usb| usb.vector.on_interrupt().any()) {
            cx.local.wake.write(());
        }
    }

    /// Polls and echoes once per wake, holding the stack's lock for each
    /// pass, until a pass moves nothing.
    #[task(priority = 1, shared = [usb, stats], local = [waiter])]
    async fn echo(mut cx: echo::Context) {
        let mut pending = [0u8; 64];
        let (mut head, mut len) = (0usize, 0usize);
        let mut tail = [0u8; 8];
        let mut stats = Stats::default();
        loop {
            cx.local.waiter.wait().await;
            loop {
                let started = DWT::cycle_count();
                let moved = cx.shared.usb.lock(|usb| {
                    let Stack { device, serial, .. } = usb;
                    let mut moved = device.poll(&mut [serial]);
                    if len == 0 {
                        head = 0;
                        if let Ok(count) = serial.read(&mut pending) {
                            moved |= count > 0;
                            len = count;
                            stats.received += count as u32;
                            for &byte in &pending[..count] {
                                tail.rotate_left(1);
                                tail[7] = byte;
                                if tail == *END {
                                    stats.ended = true;
                                }
                            }
                        }
                    }
                    if len > 0
                        && let Ok(count) = serial.write(&pending[head..head + len])
                    {
                        moved |= count > 0;
                        head += count;
                        len -= count;
                        stats.echoed += count as u32;
                    }
                    stats.configured |= device.state() == UsbDeviceState::Configured;
                    moved
                });
                stats.hold_max = stats.hold_max.max(DWT::cycle_count().wrapping_sub(started));
                cx.shared.stats.lock(|shared| *shared = stats);
                if !moved {
                    break;
                }
            }
        }
    }

    /// Waits for the host to finish, or for the patience to run out, and
    /// judges the run.
    #[idle(shared = [stats], local = [clock, timebase, led])]
    fn idle(mut cx: idle::Context) -> ! {
        let clock = cx.local.clock;
        let _ = &cx.local.timebase;
        let start = clock.now();
        let mut stats = Stats::default();
        while clock.now().wrapping_sub(start) < PATIENCE_US && !stats.ended {
            stats = cx.shared.stats.lock(|shared| *shared);
        }
        let name = "USB CDC CN5, deferred";
        let mut report = Report::new();
        report.check(
            name,
            "the host configured the device",
            if stats.configured {
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
            "the host finished, every byte echoed but the marker [USB-002]",
            if stats.ended && stats.echoed + 8 >= stats.received {
                Ok(())
            } else {
                Err(Fail::Count {
                    expected: stats.received,
                    got: stats.echoed,
                })
            },
        );
        defmt::info!(
            "{=str}: {=u32} bytes received, {=u32} echoed, longest lock hold {=u32} cycles",
            name,
            stats.received,
            stats.echoed,
            stats.hold_max
        );
        let led = cx.local.led;
        tests::finish(&report, || led.set_high())
    }
}
