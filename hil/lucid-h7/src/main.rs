//! The real-board bench on the TBS Lucid H7, reporting over USB (see
//! `hil/common/usb_bench.rs`), and the H743VI build's own run on hardware.
//! The checks: the crystal against the host's start-of-frames, the IMU on
//! SPI1 over DMA with its data-ready on EXTI2, and USB itself through the
//! host's echo. The caches stay off, as FerroWasp keeps them on this board.
//!
//! The motor, receiver and ESC telemetry pins stay as reset left them, and
//! the SD card isn't touched. Flashing replaces only the sectors the image
//! lands in: whatever else the board's flash holds stays.

#![no_std]
#![no_main]

#[macro_use]
#[path = "../../common/usb_bench.rs"]
mod usb_bench;
#[path = "../../common/imu.rs"]
mod imu;

use core::ptr::{addr_of_mut, read_volatile, write_volatile};

use cortex_m_rt::entry;
use ferrohal_stm32::{
    chip::{Peripherals, SPI1, TIM2},
    dma::Stream,
    exti::Edge,
    gpio::{Alternate, Pin, Pull, Speed},
    iwdg::Watchdog,
    rcc::{
        AhbPrescaler, ApbPrescaler, Clocks, Config as ClockConfig, Hse, HseMode, Pll, PllSource,
        Supply, Sysclk, VoltageScale,
    },
    spi::{self, Buffers, Spi, Wiring},
    time::Hertz,
    timer::Timebase,
    usb::UsbFs,
};

use imu::Imu;
use usb_bench::{Link, Report, WATCHDOG_US, reboot_to_dfu};

/// The endpoint memory `usb_bench` gives the driver: the H7's OTG_FS FIFO.
const EP_WORDS: usize = 1024;

/// The board's 8 MHz crystal to a 400 MHz core: 800 MHz wide VCO, 200 MHz AXI
/// and AHB, every APB at 100 MHz, 100 MHz on PLL1Q for SPI1 to SPI3. USB runs
/// from HSI48.
const CLOCK_CONFIG: ClockConfig = ClockConfig {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Oscillator,
    }),
    pll1: Some(Pll {
        source: PllSource::Hse,
        prediv: 1,
        mul: 100,
        divp: Some(2),
        divq: Some(8),
        divr: None,
    }),
    sys: Sysclk::Pll1P,
    cpu_pre: AhbPrescaler::Div1,
    ahb_pre: AhbPrescaler::Div2,
    apb1_pre: ApbPrescaler::Div2,
    apb2_pre: ApbPrescaler::Div2,
    apb3_pre: ApbPrescaler::Div2,
    apb4_pre: ApbPrescaler::Div2,
    supply: Supply::Ldo,
    voltage_scale: VoltageScale::Scale1,
};
const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the H743's limits"),
};
const _: () = assert!(Timebase::check::<TIM2>(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The IMU's register speed, on the routes FerroWasp's manifest gives it.
const IMU_SLOW: spi::Config = spi::Config::new(Hertz::mhz(1)).with_mode(spi::MODE_3);
const IMU_FAST: spi::Config = spi::Config::new(Hertz::mhz(24)).with_mode(spi::MODE_3);
const _: () = assert!(matches!(
    Spi::check::<SPI1>(&CLOCKS, &IMU_SLOW),
    Ok(Hertz(781_250))
));
const _: () = assert!(matches!(
    Spi::check::<SPI1>(&CLOCKS, &IMU_FAST),
    Ok(Hertz(12_500_000))
));

type ImuWiring = Wiring<
    SPI1,
    Pin<'A', 5, Alternate<5>>,
    Pin<'A', 6, Alternate<5>>,
    Pin<'D', 7, Alternate<5>>,
    Stream<1, 5>,
    Stream<1, 4>,
>;

// The bootloader, by address: nothing of the HAL is up when it's entered.
const SYSTEM_MEMORY: u32 = 0x1FF0_9800;
const RCC_CR: *const u32 = 0x5802_4400 as *const u32;
const RCC_CFGR: *const u32 = 0x5802_4410 as *const u32;
const RCC_PLLCKSELR: *const u32 = 0x5802_4428 as *const u32;
const RCC_RSR: *mut u32 = 0x5802_44D0 as *mut u32;
const OTG_FS_DSTS: *const u32 = 0x4008_0808 as *const u32;

const RSR_RMVF: u32 = 1 << 16;
const RSR_IWDG1RSTF: u32 = 1 << 26;

static mut IMU_TX: [u8; 16] = [0; 16];
static mut IMU_RX: [u8; 16] = [0; 16];

/// Enters the ROM bootloader if the last reset asked for it or was the
/// watchdog's. First thing in `main`: the bootloader expects the chip as
/// reset left it, before the supply, clocks or caches are set up.
fn enter_bootloader_if_requested() {
    let requested = usb_bench::take_dfu_request();
    // SAFETY: nothing else runs yet and nothing else owns RCC, and
    // SYSTEM_MEMORY is this chip's bootloader.
    unsafe {
        let rsr = read_volatile(RCC_RSR);
        if !requested && rsr & RSR_IWDG1RSTF == 0 {
            return;
        }
        // A stale watchdog flag would bring the next boot back here.
        write_volatile(RCC_RSR, rsr | RSR_RMVF);
        usb_bench::enter_bootloader(SYSTEM_MEMORY)
    }
}

/// RCC's own view: the crystal really runs the chip.
fn rcc_suite(report: &mut Report, link: &mut Link) {
    // SAFETY: reads of RCC's status registers.
    let (cr, cfgr, pllckselr) = unsafe {
        (
            read_volatile(RCC_CR),
            read_volatile(RCC_CFGR),
            read_volatile(RCC_PLLCKSELR),
        )
    };
    check!(report, link, cr & (1 << 17) != 0, "clocks: HSE ready");
    check!(report, link, cr & (1 << 25) != 0, "clocks: PLL1 locked");
    check!(report, link, pllckselr & 0b11 == 0b10, "clocks: PLLs fed from HSE");
    check!(report, link, (cfgr >> 3) & 0b111 == 0b011, "clocks: SYSCLK is PLL1");
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
    let cause = usb_bench::cause_text(rcc.reset_cause());
    let Ok(watchdog) = Watchdog::start(p.IWDG1, WATCHDOG_US) else {
        reboot_to_dfu()
    };
    let Ok(clock) = Timebase::new(&mut rcc, p.TIM2, Hertz::mhz(1)) else {
        reboot_to_dfu()
    };

    let mut a = p.GPIOA.split(&mut rcc);
    let mut b = p.GPIOB.split(&mut rcc);
    let mut c = p.GPIOC.split(&mut rcc);
    let mut d = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);

    // PC15 is a low-power pin: it stays at its reset speed.
    let mut imu_cs = c.p15.into_push_pull_output(&mut c.ctrl);
    imu_cs.set_high();
    let mut sck = a.p5.into_alternate::<5>(&mut a.ctrl);
    sck.set_speed(&mut a.ctrl, Speed::High);
    let mut miso = a.p6.into_alternate::<5>(&mut a.ctrl);
    miso.set_pull(&mut a.ctrl, Pull::Down);
    let mut mosi = d.p7.into_alternate::<5>(&mut d.ctrl);
    mosi.set_speed(&mut d.ctrl, Speed::High);
    let imu_wiring: ImuWiring = Wiring {
        spi: p.SPI1,
        sck,
        miso,
        mosi,
        tx_dma: dma1.s5,
        rx_dma: dma1.s4,
    };
    // SAFETY: `main` runs once, and these are the only references.
    let buffers = unsafe {
        Buffers {
            tx: &mut *addr_of_mut!(IMU_TX),
            rx: &mut *addr_of_mut!(IMU_RX),
        }
    };
    let Ok(mut imu_spi) = Spi::new(&mut rcc, imu_wiring, &IMU_SLOW, buffers) else {
        reboot_to_dfu()
    };

    let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let mut ready_pin = b.p2.into_input(&mut b.ctrl);
    ready_pin.set_pull(&mut b.ctrl, Pull::Down);
    let mut imu_ready = exti.l2.route::<'B'>(&mut exti.ctrl, Edge::Rising);
    // In EXTI only, so the flag latches: its NVIC vector stays off.
    imu_ready.listen(&mut exti.ctrl);

    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let Ok(usb) = UsbFs::new(&mut rcc, p.USB_OTG_FS, dm, dp) else {
        reboot_to_dfu()
    };
    let mut link = Link::new(usb, "lucid-h7", watchdog, clock);
    let mut imu = Imu {
        spi: &mut imu_spi,
        cs: &mut imu_cs,
        ready: imu_ready.handle(),
        slow: IMU_SLOW,
        fast: IMU_FAST,
    };
    usb_bench::serve(&mut link, cause, |report, link| {
        rcc_suite(report, link);
        usb_bench::clock_suite(report, link, OTG_FS_DSTS, CLOCKS.cpu.0);
        imu.suite(report, link);
    })
}
