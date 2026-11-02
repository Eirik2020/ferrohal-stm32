//! The real-board bench on the TBS Lucid H7, reporting over USB (see
//! `hil/common/usb_bench.rs`), and the H743VI build's own run on hardware.
//! The checks: the crystal against the host's start-of-frames, the IMU on
//! SPI1 over DMA with its data-ready on EXTI2, its battery inputs, the SD card
//! if one is in, and USB itself through the host's echo. The caches stay off,
//! as FerroWasp keeps them on this board.
//!
//! The motor, receiver and ESC telemetry pins stay as reset left them.
//! Flashing replaces only the sectors the image lands in: whatever else the
//! board's flash holds stays.
//!
//! The SD check saves 17 blocks far past anything FerroWasp's layout uses,
//! writes over them, reads back and writes the saved contents back. A power
//! cut in between would leave those blocks changed; nothing else is written.

#![no_std]
#![no_main]

#[macro_use]
#[path = "../../common/usb_bench.rs"]
mod usb_bench;
#[path = "../../common/board_adc.rs"]
mod board_adc;
#[path = "../../common/imu.rs"]
mod imu;

use core::ptr::{addr_of_mut, read_volatile};

use cortex_m::peripheral::DWT;
use cortex_m_rt::entry;
use ferrohal_stm32::{
    adc::{Adc, Config as AdcConfig},
    chip::{Peripherals, SPI1, TIM2},
    dma::Stream,
    exti::Edge,
    gpio::{Alternate, Pin, Pull, Speed},
    iwdg::Watchdog,
    rcc::{
        AhbPrescaler, ApbPrescaler, Clocks, Config as ClockConfig, Hse, HseMode, Pll, PllSource,
        Supply, Sysclk, VoltageScale,
    },
    sdmmc::{self, BLOCK_SIZE, Sdmmc},
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

const RCC_CR: *const u32 = 0x5802_4400 as *const u32;
const RCC_CFGR: *const u32 = 0x5802_4410 as *const u32;
const RCC_PLLCKSELR: *const u32 = 0x5802_4428 as *const u32;
const OTG_FS_DSTS: *const u32 = 0x4008_0808 as *const u32;

static mut ADC_SAMPLES: [u16; 2] = [0; 2];
/// Blocks in the multi-block check, and the saved, written and read copies.
const SD_BLOCKS: usize = 16;
static mut SD_SAVED: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut SD_WRITE: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut SD_READ: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut IMU_TX: [u8; 16] = [0; 16];
static mut IMU_RX: [u8; 16] = [0; 16];

/// The SD card, if there's one in the slot.
fn sd_suite(report: &mut Report, link: &mut Link, sd: &mut Sdmmc) {
    link.service();
    let card = match sd.init_card() {
        Ok(card) => card,
        Err(sdmmc::Error::NoCard) => {
            link.line(format_args!("INFO sd: no card in the slot; nothing to check"));
            return;
        }
        Err(error) => {
            check!(report, link, false, "sd: bring-up: {:?}", error);
            return;
        }
    };
    check!(
        report,
        link,
        card.blocks > 0 && card.clock.0 == 25_000_000,
        "sd: {} blocks at {} Hz, CID {:032x}",
        card.blocks,
        card.clock.0,
        card.cid
    );
    if card.blocks < 1 << 16 {
        check!(report, link, false, "sd: too small to check safely");
        return;
    }
    let scratch = card.blocks - (1 << 15);
    // SAFETY: only this suite touches them, and it doesn't run twice at once.
    let (saved, written, read) = unsafe {
        (
            &mut *addr_of_mut!(SD_SAVED),
            &mut *addr_of_mut!(SD_WRITE),
            &mut *addr_of_mut!(SD_READ),
        )
    };
    link.service();
    let result = sd.read_blocks(scratch, &mut saved[..]);
    check!(report, link, result.is_ok(), "sd: saved 17 blocks: {:?}", result);
    if result.is_err() {
        return;
    }
    // Different each run, so an old pattern can't pass for a new write.
    let seed = DWT::cycle_count();
    for (index, block) in written.iter_mut().enumerate() {
        for (offset, byte) in block.iter_mut().enumerate() {
            *byte = pattern(index * BLOCK_SIZE + offset, seed);
        }
    }

    let (one, many) = written.split_at(1);
    let (read_one, read_many) = read.split_at_mut(1);
    link.service();
    let start = link.now();
    let wrote_one = sd.write_blocks(scratch, one);
    let one_write_us = link.now().wrapping_sub(start);
    link.service();
    let read_back_one = sd.read_blocks(scratch, read_one);
    check!(
        report,
        link,
        wrote_one.is_ok() && read_back_one.is_ok() && read_one[0] == one[0],
        "sd: one block written and read back in {} us: {:?} {:?}",
        one_write_us,
        wrote_one,
        read_back_one
    );

    link.service();
    let start = link.now();
    let wrote_many = sd.write_blocks(scratch + 1, many);
    let many_write_us = link.now().wrapping_sub(start);
    link.service();
    let start = link.now();
    let read_back_many = sd.read_blocks(scratch + 1, read_many);
    let many_read_us = link.now().wrapping_sub(start);
    check!(
        report,
        link,
        wrote_many.is_ok() && read_back_many.is_ok() && read_many == many,
        "sd: 16 blocks: written in {} us, read in {} us: {:?} {:?}",
        many_write_us,
        many_read_us,
        wrote_many,
        read_back_many
    );

    // Past the end is refused, before anything reaches the card.
    let refused = sd.read_blocks(card.blocks - 1, read_many);
    check!(
        report,
        link,
        refused == Err(sdmmc::Error::OutOfRange),
        "sd: a read past the end is refused: {:?}",
        refused
    );

    // One at a time reads what many at once wrote, the other way about.
    link.service();
    let single = sd.read_block(scratch + 1, &mut read_many[0]);
    check!(
        report,
        link,
        single.is_ok() && read_many[0] == many[0],
        "sd: a single read agrees with a multiple write: {:?}",
        single
    );

    link.service();
    let restored = sd.write_blocks(scratch, &saved[..]);
    link.service();
    let checked = sd.read_blocks(scratch, &mut read[..]);
    check!(
        report,
        link,
        restored.is_ok() && checked.is_ok() && *read == *saved,
        "sd: the 17 blocks are back as they were: {:?} {:?}",
        restored,
        checked
    );
}

fn pattern(index: usize, seed: u32) -> u8 {
    ((index as u32).wrapping_mul(2_654_435_761).wrapping_add(seed) >> 24) as u8
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
    usb_bench::enter_bootloader_if_requested();

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

    // The battery inputs on ADC1, over a stream the flight code leaves free;
    // VDDA and the die from ADC3's internal channels.
    let Ok(mut adc3) = Adc::new(&mut rcc, p.ADC3, &AdcConfig::slow_source()) else {
        reboot_to_dfu()
    };
    let vdda_mv = adc3.vdda_millivolts();
    let die_tenths = adc3.temperature_decidegrees();
    let Ok(adc1) = Adc::new(&mut rcc, p.ADC1, &AdcConfig::slow_source()) else {
        reboot_to_dfu()
    };
    let voltage = c.p0.into_analog(&mut c.ctrl);
    let current = c.p1.into_analog(&mut c.ctrl);
    let channels = [adc1.channel(&voltage), adc1.channel(&current)];
    // SAFETY: `main` runs once, and this is the only reference.
    let samples: &'static mut [u16] = unsafe { &mut *addr_of_mut!(ADC_SAMPLES) };
    let dma2 = p.DMA2.split(&mut rcc);
    let Ok(mut scan) = adc1.into_scan(dma2.s5, &channels, samples) else {
        reboot_to_dfu()
    };

    // The SD card: pull-ups on the command and data lines.
    let mut ck = c.p12.into_alternate::<12>(&mut c.ctrl);
    ck.set_speed(&mut c.ctrl, Speed::VeryHigh);
    let mut cmd = d.p2.into_alternate::<12>(&mut d.ctrl);
    cmd.set_speed(&mut d.ctrl, Speed::VeryHigh);
    cmd.set_pull(&mut d.ctrl, Pull::Up);
    let mut d0 = c.p8.into_alternate::<12>(&mut c.ctrl);
    d0.set_speed(&mut c.ctrl, Speed::VeryHigh);
    d0.set_pull(&mut c.ctrl, Pull::Up);
    let mut d1 = c.p9.into_alternate::<12>(&mut c.ctrl);
    d1.set_speed(&mut c.ctrl, Speed::VeryHigh);
    d1.set_pull(&mut c.ctrl, Pull::Up);
    let mut d2 = c.p10.into_alternate::<12>(&mut c.ctrl);
    d2.set_speed(&mut c.ctrl, Speed::VeryHigh);
    d2.set_pull(&mut c.ctrl, Pull::Up);
    let mut d3 = c.p11.into_alternate::<12>(&mut c.ctrl);
    d3.set_speed(&mut c.ctrl, Speed::VeryHigh);
    d3.set_pull(&mut c.ctrl, Pull::Up);
    let Ok(mut sd) = Sdmmc::new(
        &mut rcc,
        sdmmc::Wiring {
            sdmmc: p.SDMMC1,
            ck,
            cmd,
            d0,
            d1,
            d2,
            d3,
        },
        &sdmmc::Config::new(Hertz::mhz(25)),
    ) else {
        reboot_to_dfu()
    };

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
        usb_bench::uid_suite(report, link);
        imu.suite(report, link);
        let vdda = vdda_mv.unwrap_or(0);
        check!(report, link, (3000..=3600).contains(&vdda), "adc: VDDA is {} mV", vdda);
        check!(report, link, matches!(die_tenths, Ok(100..=800)), "adc: die {:?} tenths of a degree", die_tenths);
        board_adc::suite(report, link, &mut scan, &["voltage PC0", "current PC1"], vdda);
        sd_suite(report, link, &mut sd);
    })
}
