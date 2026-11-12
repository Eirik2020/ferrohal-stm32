//! USB under RTIC, as the flight code runs it: bound to its interrupt at
//! priority 5, and nothing else services it. The host's echo checks every byte;
//! `BENCHDFU` restarts into the bootloader. Idle feeds the watchdog, so an
//! interrupt that never lets go ends in the bootloader too, and so does no
//! host configuring the device within 30 s.

#![no_std]
#![no_main]

#[macro_use]
#[path = "../../../common/usb_bench.rs"]
mod usb_bench;
#[allow(dead_code)]
#[path = "../clocks.rs"]
mod clocks;

/// The endpoint memory `usb_bench` gives the driver.
const EP_WORDS: usize = 320;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true)]
mod app {
    use ferrohal_stm32::{gpio::Speed, iwdg::Watchdog, time::Hertz, timer::Timebase, usb::UsbFs};

    use crate::{
        clocks::CLOCK_CONFIG,
        usb_bench::{self, CONFIGURED_PATIENCE_US, Link, WATCHDOG_US, reboot_to_dfu},
    };

    #[shared]
    struct Shared {
        link: Link,
    }

    #[local]
    struct Local {
        tail: [u8; 8],
        idle_dog: Watchdog,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        usb_bench::enter_bootloader_if_requested();
        let p = cx.device;
        let Ok(mut rcc) = p
            .RCC
            .freeze(&CLOCK_CONFIG)
            .map_err(|refused| refused.error())
        else {
            reboot_to_dfu()
        };
        let _ = rcc.reset_cause();
        let (Ok(watchdog), Ok(clock)) = (
            Watchdog::start(p.IWDG, WATCHDOG_US),
            Timebase::new(&mut rcc, p.TIM2, Hertz::mhz(1)),
        ) else {
            reboot_to_dfu()
        };
        let mut a = p.GPIOA.split(&mut rcc);
        let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
        dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
        let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
        dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
        let Ok(usb) = UsbFs::new(&mut rcc, p.USB_OTG_FS, dm, dp) else {
            reboot_to_dfu()
        };
        let idle_dog = watchdog.handle();
        let link = Link::new(usb, "foxeer-f405-v2", watchdog, clock.reader());
        (
            Shared { link },
            Local {
                tail: [0; 8],
                idle_dog,
            },
        )
    }

    #[idle(shared = [link], local = [idle_dog])]
    fn idle(mut cx: idle::Context) -> ! {
        let boot = cx.shared.link.lock(|link| link.now());
        let mut waiting = true;
        loop {
            cx.local.idle_dog.feed();
            cortex_m::asm::delay(1_000_000);
            if waiting {
                let (configured, now) = cx.shared.link.lock(|link| (link.configured(), link.now()));
                waiting = !configured;
                if waiting && now.wrapping_sub(boot) > CONFIGURED_PATIENCE_US {
                    reboot_to_dfu();
                }
            }
        }
    }

    #[task(binds = OTG_FS, priority = 5, shared = [link], local = [tail])]
    fn usb_fs(mut cx: usb_fs::Context) {
        let tail = cx.local.tail;
        cx.shared.link.lock(|link| link.echo_from_interrupt(tail));
    }
}
