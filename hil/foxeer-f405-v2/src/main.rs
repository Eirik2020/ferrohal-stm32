//! The real-board bench on the Foxeer F405 V2: no probe, so everything goes
//! through the flight controller's USB port.
//!
//! Commands from `hil/usb_cdc_host.py`, each an 8-byte marker: `BENCHRUN`
//! runs the checks and reports `PASS` and `FAIL` lines, ending `DONE`;
//! `BENCHECH` echoes until `BENCHEND`; `BENCHDFU` restarts into the ROM DFU
//! bootloader.
//!
//! The board has no other way back but its BOOT button, so a run that goes
//! wrong restarts into the bootloader too: a panic, a fault, a watchdog reset
//! (3 s without the main loop), clocks that don't start, or no host
//! configuring the device within 30 s. That the board comes up in DFU rather
//! than as a serial port is the report, for those.
//!
//! The checks: the crystal against the host's start-of-frames, the IMU on
//! SPI1 over DMA with its data-ready on EXTI4, the NOR on SPI2 without DMA,
//! and USB itself through the host's echo.
//!
//! The motor, receiver and ESC telemetry pins stay as reset left them. The
//! NOR checks erase and program only the scratch sector, which FerroWasp
//! keeps for destructive tests, and check that the sectors either side (a
//! configuration slot, the flight log) didn't change.

#![no_std]
#![no_main]

use core::{
    fmt::{self, Write as _},
    mem::MaybeUninit,
    panic::PanicInfo,
    ptr::{addr_of_mut, read_volatile, write_volatile},
};

use cortex_m::peripheral::{DWT, SCB};
use cortex_m_rt::{ExceptionFrame, entry, exception};
use embedded_hal::spi::SpiBus;
use ferrohal_stm32::{
    chip::{Peripherals, SPI1, SPI2, TIM2},
    dma::Stream,
    exti::{Edge, Exti},
    gpio::{Alternate, Output, Pin, Pull, Speed},
    iwdg::Watchdog,
    rcc::{
        AhbPrescaler, ApbPrescaler, Clocks, Config as ClockConfig, Hse, HseMode, Pll, PllP,
        PllSource, Supply, Sysclk,
    },
    spi::{self, BlockingSpi, BlockingWiring, Buffers, Spi, Wiring},
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
use usbd_serial::SerialPort;

/// The board's 8 MHz crystal to 168 MHz, with 48 MHz on PLLQ for USB.
const CLOCK_CONFIG: ClockConfig = ClockConfig {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Oscillator,
    }),
    pll: Some(Pll {
        source: PllSource::Hse,
        prediv: 8,
        mul: 336,
        divp: PllP::Div2,
        divq: Some(7),
    }),
    sys: Sysclk::PllP,
    ahb_pre: AhbPrescaler::Div1,
    apb1_pre: ApbPrescaler::Div4,
    apb2_pre: ApbPrescaler::Div2,
    supply: Supply::V2_7To3_6,
};
const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the F405's limits"),
};
const _: () = assert!(Timebase::check::<TIM2>(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The IMU's register speed, on the routes FerroWasp's manifest gives it.
const IMU_SLOW: spi::Config = spi::Config::new(Hertz::mhz(1)).with_mode(spi::MODE_3);
const IMU_FAST: spi::Config = spi::Config::new(Hertz::mhz(11)).with_mode(spi::MODE_3);
const NOR_BUS: spi::Config = spi::Config::new(Hertz::mhz(11));
const _: () = assert!(matches!(
    Spi::check::<SPI1>(&CLOCKS, &IMU_SLOW),
    Ok(Hertz(656_250))
));
const _: () = assert!(matches!(
    Spi::check::<SPI1>(&CLOCKS, &IMU_FAST),
    Ok(Hertz(10_500_000))
));
const _: () = assert!(matches!(
    Spi::check::<SPI2>(&CLOCKS, &NOR_BUS),
    Ok(Hertz(10_500_000))
));

type ImuWiring = Wiring<
    SPI1,
    Pin<'A', 5, Alternate<5>>,
    Pin<'A', 6, Alternate<5>>,
    Pin<'A', 7, Alternate<5>>,
    Stream<2, 3>,
    Stream<2, 0>,
>;
type NorWiring = BlockingWiring<
    SPI2,
    Pin<'B', 13, Alternate<5>>,
    Pin<'C', 2, Alternate<5>>,
    Pin<'C', 3, Alternate<5>>,
>;

const RUN: &[u8; 8] = b"BENCHRUN";
const ECHO: &[u8; 8] = b"BENCHECH";
const END: &[u8; 8] = b"BENCHEND";
const DFU: &[u8; 8] = b"BENCHDFU";

const WATCHDOG_US: u32 = 3_000_000;
const CONFIGURED_PATIENCE_US: u32 = 30_000_000;
const SEND_PATIENCE_US: u32 = 2_000_000;

// The bootloader, by address: nothing of the HAL is up when it's entered.
const SYSTEM_MEMORY: u32 = 0x1FFF_0000;
const RCC_CR: *const u32 = 0x4002_3800 as *const u32;
const RCC_PLLCFGR: *const u32 = 0x4002_3804 as *const u32;
const RCC_CFGR: *const u32 = 0x4002_3808 as *const u32;
const RCC_APB2ENR: *mut u32 = 0x4002_3844 as *mut u32;
const RCC_CSR: *mut u32 = 0x4002_3874 as *mut u32;
const SYSCFG_MEMRMP: *mut u32 = 0x4001_3800 as *mut u32;
const OTG_FS_DSTS: *const u32 = 0x5000_0808 as *const u32;

const CSR_RMVF: u32 = 1 << 24;
const CSR_WDGRSTF: u32 = 1 << 29;
const APB2ENR_SYSCFGEN: u32 = 1 << 14;
const REQUEST_DFU: u32 = 0x4446_5521;

/// Left by [`reboot_to_dfu`]: `.uninit` is RAM a reset leaves alone.
#[unsafe(link_section = ".uninit.dfu_request")]
static mut REQUEST: MaybeUninit<u32> = MaybeUninit::uninit();

static mut EP_MEMORY: [u32; 320] = [0; 320];
static mut BUS: Option<UsbBusAllocator<UsbBus>> = None;
static mut IMU_TX: [u8; 16] = [0; 16];
static mut IMU_RX: [u8; 16] = [0; 16];

/// Resets into the ROM bootloader.
fn reboot_to_dfu() -> ! {
    // SAFETY: one aligned word, volatile, and the next thing is the reset.
    unsafe { addr_of_mut!(REQUEST).cast::<u32>().write_volatile(REQUEST_DFU) };
    cortex_m::asm::dsb();
    SCB::sys_reset()
}

/// Enters the ROM bootloader if the last reset asked for it or was the
/// watchdog's. First thing in `main`: the bootloader expects the chip as
/// reset left it, and this leaves one thing changed, the memory mapping it
/// would have had from the BOOT0 pin.
fn enter_bootloader_if_requested() {
    // SAFETY: nothing else runs yet and nothing else owns RCC or SYSCFG. The
    // request word is read as an integer, volatile: its power-on value is
    // whatever RAM held. SYSTEM_MEMORY holds the bootloader's stack pointer
    // and reset vector (AN2606).
    unsafe {
        let request = addr_of_mut!(REQUEST).cast::<u32>();
        let word = request.read_volatile();
        request.write_volatile(0);
        let csr = read_volatile(RCC_CSR);
        if word != REQUEST_DFU && csr & CSR_WDGRSTF == 0 {
            return;
        }
        // A stale watchdog flag would bring the next boot back here.
        write_volatile(RCC_CSR, csr | CSR_RMVF);
        write_volatile(RCC_APB2ENR, read_volatile(RCC_APB2ENR) | APB2ENR_SYSCFGEN);
        let _ = read_volatile(RCC_APB2ENR);
        write_volatile(SYSCFG_MEMRMP, 1);
        (*SCB::PTR).vtor.write(SYSTEM_MEMORY);
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
        cortex_m::asm::bootload(SYSTEM_MEMORY as *const u32)
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
struct Link {
    device: UsbDevice<'static, UsbBus>,
    serial: SerialPort<'static, UsbBus>,
    watchdog: Watchdog,
    clock: Timebase,
}

impl Link {
    /// Microseconds, from TIM2.
    fn now(&self) -> u32 {
        self.clock.now()
    }

    fn service(&mut self) {
        self.watchdog.feed();
        self.device.poll(&mut [&mut self.serial]);
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

    fn line(&mut self, args: fmt::Arguments) {
        let mut line = Line { bytes: [0; 160], len: 0 };
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
struct Report {
    passed: u32,
    failed: u32,
}

impl Report {
    fn check(&mut self, link: &mut Link, ok: bool, args: fmt::Arguments) {
        if ok {
            self.passed += 1;
            link.line(format_args!("PASS {args}"));
        } else {
            self.failed += 1;
            link.line(format_args!("FAIL {args}"));
        }
    }
}

macro_rules! check {
    ($report:expr, $link:expr, $ok:expr, $($arg:tt)*) => {
        $report.check($link, $ok, format_args!($($arg)*))
    };
}

/// Whether `got` is within `hundredths` of a percent of `want`.
fn close(got: u32, want: u32, hundredths: u32) -> bool {
    u64::from(got.abs_diff(want)) * 10_000 <= u64::from(want) * u64::from(hundredths)
}

/// The crystal really runs the chip: RCC's own view, then the CPU's cycles
/// counted against the host's 1 kHz start-of-frames, the one clock here that
/// doesn't come from the crystal, then TIM2 against the CPU.
fn clock_suite(report: &mut Report, link: &mut Link) {
    // SAFETY: reads of RCC's status registers.
    let (cr, pllcfgr, cfgr) =
        unsafe { (read_volatile(RCC_CR), read_volatile(RCC_PLLCFGR), read_volatile(RCC_CFGR)) };
    check!(report, link, cr & (1 << 17) != 0, "clocks: HSE ready");
    check!(report, link, cr & (1 << 25) != 0, "clocks: PLL locked");
    check!(report, link, pllcfgr & (1 << 22) != 0, "clocks: PLL fed from HSE");
    check!(report, link, (cfgr >> 2) & 0b11 == 0b10, "clocks: SYSCLK is the PLL");

    // SAFETY: a read of the device status register, whose frame number the
    // USB driver doesn't use. Full speed numbers its frames in 11 bits.
    let frame = || unsafe { (read_volatile(OTG_FS_DSTS) >> 8) & 0x7FF };
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
        close(measured, CLOCKS.sysclk.0, 10),
        "clocks: {} CPU cycles per 1000 host frames, want {} within 0.1%",
        measured,
        CLOCKS.sysclk.0
    );

    let cycle_start = DWT::cycle_count();
    let tick_start = link.now();
    while link.now().wrapping_sub(tick_start) < 100_000 {
        link.service();
    }
    let elapsed = DWT::cycle_count().wrapping_sub(cycle_start);
    check!(
        report,
        link,
        close(elapsed, CLOCKS.sysclk.0 / 10, 10),
        "clocks: {} CPU cycles per 100 ms of TIM2 (84 MHz timer clock), want {} within 0.1%",
        elapsed,
        CLOCKS.sysclk.0 / 10
    );
}

/// One read of WHO_AM_I over DMA, finished by polling.
fn imu_who_am_i(
    link: &mut Link,
    imu: &mut Spi,
    cs: &mut Pin<'A', 4, Output>,
) -> Result<u8, spi::Error> {
    const WHO_AM_I: u8 = 0x75;
    const READ: u8 = 0x80;
    cs.set_low();
    let mut result = imu.start(&[WHO_AM_I | READ, 0]);
    if result.is_ok() {
        let start = link.now();
        result = loop {
            link.service();
            if let Some(done) = imu.poll() {
                break done.map(|_| ());
            }
            if link.now().wrapping_sub(start) > 10_000 {
                imu.abort();
                break Err(spi::Error::Timeout);
            }
        };
    }
    cs.set_high();
    result?;
    Ok(imu.received().get(1).copied().unwrap_or(0))
}

/// The IMU over SPI1 on DMA2: the routes the flight code uses, on an F405.
fn imu_suite(report: &mut Report, link: &mut Link, board: &mut Board) {
    let Board { imu, imu_cs: cs, imu_ready, .. } = board;
    let id = imu_who_am_i(link, imu, cs);
    let known = matches!(id, Ok(0x47 | 0x70 | 0x68));
    match id {
        Ok(byte) => check!(
            report,
            link,
            known,
            "imu: WHO_AM_I over DMA is {:#04x} at {} Hz",
            byte,
            imu.frequency().0
        ),
        Err(error) => check!(report, link, false, "imu: WHO_AM_I over DMA: {:?}", error),
    }
    let mut same = true;
    for _ in 0..100 {
        same &= imu_who_am_i(link, imu, cs) == id;
    }
    check!(report, link, same, "imu: 100 more DMA reads say the same");

    cs.set_low();
    let blocking = imu.transfer(&[0x75 | 0x80, 0]).ok().and_then(|rx| rx.get(1).copied());
    cs.set_high();
    check!(
        report,
        link,
        blocking == id.ok(),
        "imu: the blocking path reads {:?}",
        blocking
    );

    // Only the ICM-42688-P takes registers this fast; the MPUs stop at 1 MHz.
    if id == Ok(0x47) {
        let changed = imu.set_config(&IMU_FAST).is_ok();
        let mut same = changed;
        for _ in 0..100 {
            same &= imu_who_am_i(link, imu, cs) == id;
        }
        check!(report, link, same, "imu: 100 DMA reads at {} Hz", imu.frequency().0);
        imu_data_suite(report, link, imu, cs, imu_ready);
        let _ = imu.set_config(&IMU_SLOW);
    }
    let health = imu.health();
    check!(
        report,
        link,
        health.dma_errors == 0 && health.overruns == 0 && health.mode_faults == 0 && health.timeouts == 0,
        "imu: {:?}",
        health
    );
}

fn imu_write(imu: &mut Spi, cs: &mut Pin<'A', 4, Output>, register: u8, value: u8) -> bool {
    cs.set_low();
    let ok = imu.transfer(&[register, value]).is_ok();
    cs.set_high();
    ok
}

fn settle(link: &mut Link, us: u32) {
    let start = link.now();
    while link.now().wrapping_sub(start) < us {
        link.service();
    }
}

/// The flight path on the real sensor, set up as FerroWasp sets it up: the
/// data-ready pulse on PC4 sets EXTI line 4, which starts a DMA read of
/// temperature, accelerometer and gyroscope. Polled here, with the vector
/// off: the pending flag and the driver's completion are what a handler
/// would see. Ends with a soft reset, leaving the sensor as power-on did.
fn imu_data_suite(
    report: &mut Report,
    link: &mut Link,
    imu: &mut Spi,
    cs: &mut Pin<'A', 4, Output>,
    ready: &mut Exti<'C', 4>,
) {
    const DEVICE_CONFIG: u8 = 0x11;
    const INT_CONFIG: u8 = 0x14;
    const TEMP_DATA1: u8 = 0x1D;
    const INT_STATUS: u8 = 0x2D;
    const INTF_CONFIG0: u8 = 0x4C;
    const PWR_MGMT0: u8 = 0x4E;
    const GYRO_CONFIG0: u8 = 0x4F;
    const ACCEL_CONFIG0: u8 = 0x50;
    const GYRO_ACCEL_CONFIG0: u8 = 0x52;
    const INT_CONFIG1: u8 = 0x64;
    const INT_SOURCE0: u8 = 0x65;
    const READ: u8 = 0x80;
    /// ±16 g, the full scale `ACCEL_CONFIG0` below picks with 2 kHz.
    const LSB_PER_G: u64 = 2048;
    const RATE_HZ: u32 = 2000;
    const SAMPLES: u32 = 400;

    let mut written = imu_write(imu, cs, DEVICE_CONFIG, 1);
    settle(link, 2_000);
    cs.set_low();
    let status = imu.transfer(&[INT_STATUS | READ, 0]).ok().and_then(|rx| rx.get(1).copied());
    cs.set_high();
    check!(
        report,
        link,
        written && matches!(status, Some(status) if status & (1 << 4) != 0),
        "imu: soft reset done, status {:?}",
        status
    );

    written = imu_write(imu, cs, INTF_CONFIG0, 0x33);
    written &= imu_write(imu, cs, GYRO_CONFIG0, 0x05);
    written &= imu_write(imu, cs, ACCEL_CONFIG0, 0x05);
    written &= imu_write(imu, cs, GYRO_ACCEL_CONFIG0, 0x11);
    // The last of the configuration: no writes for 200 us after it.
    written &= imu_write(imu, cs, PWR_MGMT0, 0x0F);
    settle(link, 1_000);
    written &= imu_write(imu, cs, INT_CONFIG, 0x03);
    written &= imu_write(imu, cs, INT_CONFIG1, 0);
    written &= imu_write(imu, cs, INT_SOURCE0, 1 << 3);
    settle(link, 49_000);
    check!(report, link, written, "imu: configured for 2 kHz data ready on INT1");

    // The rate, with nothing else in the way.
    ready.clear();
    let start = link.now();
    let mut pulses = 0u32;
    while link.now().wrapping_sub(start) < 200_000 {
        link.service();
        if ready.is_pending() {
            ready.clear();
            pulses += 1;
        }
    }
    check!(
        report,
        link,
        pulses.abs_diff(RATE_HZ / 5) <= RATE_HZ / 100,
        "imu: {} data-ready edges in 200 ms on EXTI4, want {} within 5%",
        pulses,
        RATE_HZ / 5
    );

    // Each pulse starts a burst, as in flight.
    let mut burst = [0u8; 15];
    burst[0] = TEMP_DATA1 | READ;
    let mut done = 0u32;
    let mut failures = 0u32;
    let mut slowest_us = 0;
    let mut magnitude_squared = 0u64;
    let mut temperature = 0i32;
    let start = link.now();
    ready.clear();
    while done < SAMPLES && link.now().wrapping_sub(start) < 1_000_000 {
        link.service();
        if !ready.is_pending() {
            continue;
        }
        ready.clear();
        let began = link.now();
        cs.set_low();
        let mut result = imu.start(&burst);
        if result.is_ok() {
            result = loop {
                if let Some(finished) = imu.poll() {
                    break finished.map(|_| ());
                }
                if link.now().wrapping_sub(began) > 1_000 {
                    imu.abort();
                    break Err(spi::Error::Timeout);
                }
            };
        }
        cs.set_high();
        slowest_us = slowest_us.max(link.now().wrapping_sub(began));
        let rx = imu.received();
        if result.is_err() || rx.len() != burst.len() {
            failures += 1;
            continue;
        }
        let word = |at: usize| i64::from(i16::from_be_bytes([rx[at], rx[at + 1]]));
        temperature = word(1) as i32;
        let (x, y, z) = (word(3), word(5), word(7));
        magnitude_squared += (x * x + y * y + z * z) as u64;
        done += 1;
    }
    let elapsed_us = link.now().wrapping_sub(start);
    check!(
        report,
        link,
        done == SAMPLES && failures == 0,
        "imu: {} bursts of 15 bytes at {} Hz, {} failed, slowest {} us",
        done,
        imu.frequency().0,
        failures,
        slowest_us
    );
    let want_us = SAMPLES * 1_000_000 / RATE_HZ;
    check!(
        report,
        link,
        elapsed_us.abs_diff(want_us) <= want_us / 20,
        "imu: they took {} us, want {} within 5%: none missed",
        elapsed_us,
        want_us
    );
    let milli_g = (magnitude_squared / u64::from(done.max(1))).isqrt() * 1000 / LSB_PER_G;
    check!(
        report,
        link,
        (800..=1200).contains(&milli_g),
        "imu: accelerometer reads {} mg: gravity, if the board isn't being moved",
        milli_g
    );
    // The datasheet: 25 C + raw / 132.48.
    let celsius = 25 + temperature * 100 / 13248;
    check!(report, link, (0..=90).contains(&celsius), "imu: {} C inside", celsius);

    let _ = imu_write(imu, cs, DEVICE_CONFIG, 1);
    settle(link, 2_000);
}

const NOR_SECTOR: usize = 4096;
const NOR_PAGE: usize = 256;
const NOR_SCRATCH: u32 = 0x2000;
const NOR_BUSY: u8 = 1;
const NOR_WRITE_ENABLED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
enum NorFail {
    Bus(spi::Error),
    StillBusy,
    NotEnabled,
}

impl From<spi::Error> for NorFail {
    fn from(error: spi::Error) -> Self {
        Self::Bus(error)
    }
}

/// The JEDEC commands FerroWasp's NOR driver sends, over the polled SPI.
struct Nor {
    spi: BlockingSpi,
    cs: Pin<'B', 12, Output>,
}

impl Nor {
    fn exchange(&mut self, write: &[u8], more: &[u8], read: &mut [u8]) -> Result<(), spi::Error> {
        self.cs.set_low();
        let mut result = self.spi.write(write);
        if result.is_ok() && !more.is_empty() {
            result = self.spi.write(more);
        }
        if result.is_ok() && !read.is_empty() {
            result = self.spi.read(read);
        }
        if result.is_ok() {
            result = self.spi.flush();
        }
        self.cs.set_high();
        result
    }

    fn jedec_id(&mut self) -> Result<[u8; 3], spi::Error> {
        let mut id = [0; 3];
        self.exchange(&[0x9F], &[], &mut id)?;
        Ok(id)
    }

    fn status(&mut self) -> Result<u8, spi::Error> {
        let mut status = [0];
        self.exchange(&[0x05], &[], &mut status)?;
        Ok(status[0])
    }

    fn write_enable(&mut self) -> Result<(), NorFail> {
        self.exchange(&[0x06], &[], &mut [])?;
        if self.status()? & NOR_WRITE_ENABLED == 0 {
            return Err(NorFail::NotEnabled);
        }
        Ok(())
    }

    /// Waits out a write or erase, keeping USB served; returns the time.
    fn wait(&mut self, link: &mut Link, bound_us: u32) -> Result<u32, NorFail> {
        let start = link.now();
        loop {
            link.service();
            if self.status()? & NOR_BUSY == 0 {
                return Ok(link.now().wrapping_sub(start));
            }
            if link.now().wrapping_sub(start) > bound_us {
                return Err(NorFail::StillBusy);
            }
        }
    }

    fn read(&mut self, address: u32, data: &mut [u8]) -> Result<(), spi::Error> {
        let [_, high, mid, low] = address.to_be_bytes();
        self.exchange(&[0x03, high, mid, low], &[], data)
    }

    fn erase_sector(&mut self, link: &mut Link, address: u32) -> Result<u32, NorFail> {
        self.write_enable()?;
        let [_, high, mid, low] = address.to_be_bytes();
        self.exchange(&[0x20, high, mid, low], &[], &mut [])?;
        self.wait(link, 1_000_000)
    }

    fn program_page(&mut self, link: &mut Link, address: u32, page: &[u8]) -> Result<u32, NorFail> {
        self.write_enable()?;
        let [_, high, mid, low] = address.to_be_bytes();
        self.exchange(&[0x02, high, mid, low], page, &mut [])?;
        self.wait(link, 100_000)
    }

    /// Checks the whole scratch sector against `byte_at`.
    fn verify(
        &mut self,
        link: &mut Link,
        byte_at: impl Fn(usize) -> u8,
    ) -> Result<Option<usize>, spi::Error> {
        let mut page = [0; NOR_PAGE];
        for start in (0..NOR_SECTOR).step_by(NOR_PAGE) {
            link.service();
            self.read(NOR_SCRATCH + start as u32, &mut page)?;
            for (offset, &byte) in page.iter().enumerate() {
                if byte != byte_at(start + offset) {
                    return Ok(Some(start + offset));
                }
            }
        }
        Ok(None)
    }
}

fn pattern(index: usize, seed: u32) -> u8 {
    ((index as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24) as u8
}

fn nor_suite(report: &mut Report, link: &mut Link, nor: &mut Nor) {
    let name = "nor";
    let id = nor.jedec_id();
    let mut stable = true;
    for _ in 0..10 {
        stable &= nor.jedec_id() == id;
    }
    let plausible = matches!(id, Ok(id) if id[0] != 0 && id[0] != 0xFF && id != [0xFF; 3]);
    check!(
        report,
        link,
        plausible && stable,
        "{}: JEDEC ID {:02x?} at {} Hz, same over 10 reads",
        name,
        id,
        nor.spi.frequency().0
    );
    if !plausible {
        // Nothing answers: erasing and programming would only add noise.
        return;
    }
    let status = nor.status();
    check!(
        report,
        link,
        matches!(status, Ok(status) if status & NOR_BUSY == 0),
        "{}: idle, status {:?}",
        name,
        status
    );

    let mut before = [0u8; NOR_PAGE];
    let mut after = [0u8; NOR_PAGE];
    let lower = NOR_SCRATCH - NOR_SECTOR as u32;
    let upper = NOR_SCRATCH + NOR_SECTOR as u32;
    let mut neighbours_read = nor.read(lower, &mut before).is_ok();
    let mut upper_before = [0u8; NOR_PAGE];
    neighbours_read &= nor.read(upper, &mut upper_before).is_ok();

    let erased = nor.erase_sector(link, NOR_SCRATCH);
    check!(report, link, erased.is_ok(), "{}: sector erased in {:?} us", name, erased);
    let blank = nor.verify(link, |_| 0xFF);
    check!(report, link, blank == Ok(None), "{}: reads blank, first difference {:?}", name, blank);

    // Different on every run, so old contents can't pass for a new write.
    let seed = DWT::cycle_count();
    let mut page = [0u8; NOR_PAGE];
    let mut slowest = Ok(0);
    for start in (0..NOR_SECTOR).step_by(NOR_PAGE) {
        for (offset, byte) in page.iter_mut().enumerate() {
            *byte = pattern(start + offset, seed);
        }
        match nor.program_page(link, NOR_SCRATCH + start as u32, &page) {
            Ok(time) => slowest = slowest.map(|slowest: u32| slowest.max(time)),
            Err(error) => slowest = Err(error),
        }
    }
    check!(report, link, slowest.is_ok(), "{}: 16 pages programmed, slowest {:?} us", name, slowest);
    let written = nor.verify(link, |index| pattern(index, seed));
    check!(report, link, written == Ok(None), "{}: reads back, first difference {:?}", name, written);

    let mut across = [0u8; 12];
    let at = NOR_PAGE - 6;
    let read = nor.read(NOR_SCRATCH + at as u32, &mut across).is_ok();
    let matches = across.iter().enumerate().all(|(i, &byte)| byte == pattern(at + i, seed));
    check!(report, link, read && matches, "{}: a read across a page boundary", name);

    let erased = nor.erase_sector(link, NOR_SCRATCH);
    check!(report, link, erased.is_ok(), "{}: erased again in {:?} us", name, erased);
    let blank = nor.verify(link, |_| 0xFF);
    check!(report, link, blank == Ok(None), "{}: left blank, first difference {:?}", name, blank);

    neighbours_read &= nor.read(lower, &mut after).is_ok();
    let lower_same = after == before;
    neighbours_read &= nor.read(upper, &mut after).is_ok();
    check!(
        report,
        link,
        neighbours_read && lower_same && after == upper_before,
        "{}: the sectors either side didn't change",
        name
    );
}

/// What the checks work on.
struct Board {
    imu: Spi,
    imu_cs: Pin<'A', 4, Output>,
    imu_ready: Exti<'C', 4>,
    nor: Nor,
}

/// Waits for and answers commands. Echoes between `BENCHECH` and `BENCHEND`.
fn serve(link: &mut Link, board: &mut Board, cause: &str) -> ! {
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
                let start = link.now();
                while link.now().wrapping_sub(start) < 100_000 {
                    link.service();
                }
                reboot_to_dfu();
            }
            if tail == *ECHO {
                echoing = true;
            }
            if tail == *RUN {
                let mut report = Report::default();
                link.line(format_args!("INFO reset cause: {cause}"));
                clock_suite(&mut report, link);
                imu_suite(&mut report, link, board);
                nor_suite(&mut report, link, &mut board.nor);
                link.line(format_args!("DONE {} passed, {} failed", report.passed, report.failed));
            }
        }
        len = 0;
    }
}

#[entry]
fn main() -> ! {
    enter_bootloader_if_requested();

    let Some(mut core) = cortex_m::Peripherals::take() else {
        reboot_to_dfu()
    };
    core.DCB.enable_trace();
    core.DWT.enable_cycle_counter();
    let Some(p) = Peripherals::take() else {
        reboot_to_dfu()
    };
    let Ok(mut rcc) = p.RCC.freeze(&CLOCK_CONFIG) else {
        reboot_to_dfu()
    };
    let cause = rcc.reset_cause();
    let cause = match (cause.power_on, cause.pin, cause.software, cause.brownout) {
        (true, ..) => "power on",
        (_, _, true, _) => "software",
        (_, _, _, true) => "brownout",
        (_, true, ..) => "reset pin",
        _ => "other",
    };
    let Ok(watchdog) = Watchdog::start(p.IWDG, WATCHDOG_US) else {
        reboot_to_dfu()
    };
    let Ok(clock) = Timebase::new(&mut rcc, p.TIM2, Hertz::mhz(1)) else {
        reboot_to_dfu()
    };

    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let dma2 = p.DMA2.split(&mut rcc);

    let mut imu_cs = a.p4.into_push_pull_output(&mut a.ctrl);
    imu_cs.set_high();
    let mut sck = a.p5.into_alternate::<5>(&mut a.ctrl);
    sck.set_speed(&mut a.ctrl, Speed::High);
    let mut miso = a.p6.into_alternate::<5>(&mut a.ctrl);
    miso.set_pull(&mut a.ctrl, Pull::Down);
    let mut mosi = a.p7.into_alternate::<5>(&mut a.ctrl);
    mosi.set_speed(&mut a.ctrl, Speed::High);
    let imu_wiring: ImuWiring = Wiring {
        spi: p.SPI1,
        sck,
        miso,
        mosi,
        tx_dma: dma2.s3,
        rx_dma: dma2.s0,
    };
    // SAFETY: `main` runs once, and these are the only references.
    let buffers = unsafe {
        Buffers {
            tx: &mut *addr_of_mut!(IMU_TX),
            rx: &mut *addr_of_mut!(IMU_RX),
        }
    };
    let Ok(imu) = Spi::new(&mut rcc, imu_wiring, &IMU_SLOW, buffers) else {
        reboot_to_dfu()
    };

    let mut nor_cs = b.p12.into_push_pull_output(&mut b.ctrl);
    nor_cs.set_high();
    let mut sck = b.p13.into_alternate::<5>(&mut b.ctrl);
    sck.set_speed(&mut b.ctrl, Speed::High);
    let mut miso = c.p2.into_alternate::<5>(&mut c.ctrl);
    miso.set_pull(&mut c.ctrl, Pull::Down);
    let mut mosi = c.p3.into_alternate::<5>(&mut c.ctrl);
    mosi.set_speed(&mut c.ctrl, Speed::High);
    let nor_wiring: NorWiring = BlockingWiring {
        spi: p.SPI2,
        sck,
        miso,
        mosi,
    };
    let Ok(spi) = BlockingSpi::new(&mut rcc, nor_wiring, &NOR_BUS) else {
        reboot_to_dfu()
    };
    let nor = Nor { spi, cs: nor_cs };

    let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let mut ready_pin = c.p4.into_input(&mut c.ctrl);
    ready_pin.set_pull(&mut c.ctrl, Pull::Down);
    let mut imu_ready = exti.l4.route::<'C'>(&mut exti.ctrl, Edge::Rising);
    // In EXTI only, so the flag latches: its NVIC vector stays off.
    imu_ready.listen(&mut exti.ctrl);

    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let Ok(usb) = UsbFs::new(&mut rcc, p.USB_OTG_FS, dm, dp) else {
        reboot_to_dfu()
    };
    // SAFETY: `main` runs once, and these are the only references.
    let bus: &'static UsbBusAllocator<UsbBus> = unsafe {
        let memory = &mut *addr_of_mut!(EP_MEMORY);
        (*addr_of_mut!(BUS)).insert(UsbBus::new(usb, memory))
    };
    let serial = SerialPort::new(bus);
    let Ok(builder) = UsbDeviceBuilder::new(bus, UsbVidPid(0x16c0, 0x27dd)).strings(&[
        StringDescriptors::default()
            .manufacturer("FerroHAL")
            .product("FerroHAL bench")
            .serial_number("foxeer-f405-v2"),
    ]) else {
        reboot_to_dfu()
    };
    let device = builder.device_class(usbd_serial::USB_CLASS_CDC).build();
    let mut link = Link {
        device,
        serial,
        watchdog,
        clock,
    };
    let mut board = Board {
        imu,
        imu_cs,
        imu_ready,
        nor,
    };
    serve(&mut link, &mut board, cause)
}
