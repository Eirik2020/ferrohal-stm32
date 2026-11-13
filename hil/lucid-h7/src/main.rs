//! The real-board bench on the TBS Lucid H7, reporting over USB (see
//! `hil/common/usb_bench.rs`), and the H743VI build's own run on hardware.
//! The checks: the crystal against the host's start-of-frames, the IMU on
//! SPI1 over DMA with its data-ready on EXTI2, its battery inputs, the SD card
//! if one is in, and USB itself through the host's echo. The caches stay off,
//! as FerroWasp keeps them on this board.
//!
//! Every constructor that can refuse does, on purpose: each hands its parts
//! back, and the parts build the rest of this bench — including the SD card,
//! whose constructor refuses a bus speed past the default limit before the
//! peripheral is touched. The staged refusals are reported first, and the
//! reuse is the checks that follow. USB is left to the Nucleo-F401's run V:
//! this board's only USB port is its report.
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
mod clocks;
#[path = "../../common/imu.rs"]
mod imu;
#[path = "../../common/motor_lanes.rs"]
mod motor_lanes;

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
    rcc::ClockError,
    sdmmc::{self, BLOCK_SIZE, Sdmmc},
    spi::{self, Buffers, Spi, Wiring},
    time::Hertz,
    timer::{Awg, Pwm, Timebase},
    usb::UsbFs,
};

use clocks::{CLOCK_CONFIG, CLOCKS, REFUSAL_CONFIG};
use imu::Imu;
use usb_bench::{Link, Report, WATCHDOG_US, reboot_to_dfu};

/// The endpoint memory `usb_bench` gives the driver: the H7's OTG_FS FIFO.
const EP_WORDS: usize = 1024;

const _: () = assert!(Timebase::<TIM2>::check(&CLOCKS, Hertz::mhz(1)).is_ok());

/// The IMU's register speed, on the routes FerroWasp's manifest gives it.
const IMU_SLOW: spi::Config = spi::Config::new(Hertz::mhz(1)).with_mode(spi::MODE_3);
const IMU_FAST: spi::Config = spi::Config::new(Hertz::mhz(24)).with_mode(spi::MODE_3);
/// A config no clock can make: 0 Hz, for the constructor refusals.
const SPI_ZERO: spi::Config = spi::Config::new(Hertz(0));
const _: () = assert!(matches!(
    ferrohal_stm32::spi::check::<SPI1>(&CLOCKS, &IMU_SLOW),
    Ok(Hertz(781_250))
));
const _: () = assert!(matches!(
    ferrohal_stm32::spi::check::<SPI1>(&CLOCKS, &IMU_FAST),
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
static mut LANES: [[u32; 20]; 4] = [[0; 20]; 4];
static mut LANE_READS: [[u16; 64]; 4] = [[0; 64]; 4];
/// Blocks in the multi-block check, and the saved, written and read copies.
const SD_BLOCKS: usize = 16;
static mut SD_SAVED: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut SD_WRITE: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut SD_READ: [[u8; BLOCK_SIZE]; SD_BLOCKS + 1] = [[0; BLOCK_SIZE]; SD_BLOCKS + 1];
static mut IMU_TX: [u8; 16] = [0; 16];
static mut IMU_RX: [u8; 16] = [0; 16];

/// The SD card, if there's one in the slot.
fn sd_suite<W>(report: &mut Report, link: &mut Link, sd: &mut Sdmmc<W>) {
    link.service();
    let card = match sd.init_card() {
        Ok(card) => card,
        Err(sdmmc::Error::NoCard) => {
            link.line(format_args!(
                "INFO sd: no card in the slot; nothing to check"
            ));
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
    check!(
        report,
        link,
        result.is_ok(),
        "sd: saved 17 blocks: {:?}",
        result
    );
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
    ((index as u32)
        .wrapping_mul(2_654_435_761)
        .wrapping_add(seed)
        >> 24) as u8
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
    check!(
        report,
        link,
        pllckselr & 0b11 == 0b10,
        "clocks: PLLs fed from HSE"
    );
    check!(
        report,
        link,
        (cfgr >> 3) & 0b111 == 0b011,
        "clocks: SYSCLK is PLL1"
    );
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
    let (rcc_refused, mut rcc) = match p.RCC.freeze(&REFUSAL_CONFIG) {
        Err(refused) => {
            let refused_pll1 = matches!(refused.error(), ClockError::PllNotConfigured);
            // The refused token is the one that freezes the bench's clocks.
            let token = refused.into_parts();
            match token
                .freeze(&CLOCK_CONFIG)
                .map_err(|refused| refused.error())
            {
                Ok(rcc) => (refused_pll1, rcc),
                Err(_) => reboot_to_dfu(),
            }
        }
        Ok(_) => reboot_to_dfu(),
    };
    let cause = usb_bench::cause_text(rcc.reset_cause());
    let (watchdog_refused, iwdg) = match Watchdog::start(p.IWDG1, 1) {
        Err(refused) => {
            let refused_timeout = matches!(refused.error(), ferrohal_stm32::iwdg::Error::Timeout);
            (refused_timeout, refused.into_parts())
        }
        Ok(_) => reboot_to_dfu(),
    };
    let Ok(watchdog) = Watchdog::start(iwdg, WATCHDOG_US) else {
        reboot_to_dfu()
    };
    let (timebase_refused, tim2) = match Timebase::new(&mut rcc, p.TIM2, Hertz::mhz(7)) {
        Err(refused) => {
            let refused_inexact =
                matches!(refused.error(), ferrohal_stm32::timer::ConfigError::Inexact);
            (refused_inexact, refused.into_parts())
        }
        Ok(_) => reboot_to_dfu(),
    };
    let Ok(clock) = Timebase::new(&mut rcc, tim2, Hertz::mhz(1)) else {
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
    let (spi_refused, imu_wiring, buffers) =
        match Spi::new(&mut rcc, imu_wiring, &SPI_ZERO, buffers) {
            Err(refused) => {
                let refused_zero = matches!(refused.error(), spi::ConfigError::Frequency);
                let (wiring, buffers) = refused.into_parts();
                (refused_zero, wiring, buffers)
            }
            Ok(_) => reboot_to_dfu(),
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

    // The motor pads get their timers with duty 0 and the counters stopped,
    // and lose them again before anything counts: no frame reaches one. The
    // streams are the flight code's.
    let (pwm_refused, tim3) = match Pwm::new(&mut rcc, p.TIM3, Hertz(0)) {
        Err(refused) => {
            let refused_zero =
                matches!(refused.error(), ferrohal_stm32::timer::ConfigError::Inexact);
            (refused_zero, refused.into_parts())
        }
        Ok(_) => reboot_to_dfu(),
    };
    let (Ok(tim3), Ok(tim5)) = (
        Pwm::new(&mut rcc, tim3, Hertz::khz(600)),
        // A different rate on purpose, for the waveform generator's refusal.
        Pwm::new(&mut rcc, p.TIM5, Hertz::khz(300)),
    ) else {
        reboot_to_dfu()
    };
    let mut pad1 = b.p0.into_alternate::<2>(&mut b.ctrl);
    pad1.set_pull(&mut b.ctrl, Pull::Up);
    let channel3 = tim3.channel::<3, _>(pad1);
    tim3.set_output::<3>(false);
    let mut pad2 = b.p1.into_alternate::<2>(&mut b.ctrl);
    pad2.set_pull(&mut b.ctrl, Pull::Up);
    let channel4 = tim3.channel::<4, _>(pad2);
    tim3.set_output::<4>(false);
    let mut pad3 = a.p0.into_alternate::<2>(&mut a.ctrl);
    pad3.set_pull(&mut a.ctrl, Pull::Up);
    let channel1 = tim5.channel::<1, _>(pad3);
    tim5.set_output::<1>(false);
    let mut pad4 = a.p1.into_alternate::<2>(&mut a.ctrl);
    pad4.set_pull(&mut a.ctrl, Pull::Up);
    let channel2 = tim5.channel::<2, _>(pad4);
    tim5.set_output::<2>(false);
    // SAFETY: `main` runs once, and these are the only references.
    let [one, two, three, four] = unsafe { &mut *addr_of_mut!(LANES) };
    let [read1, read2, read3, read4] = unsafe { &mut *addr_of_mut!(LANE_READS) };
    let dma2 = p.DMA2.split(&mut rcc);
    // SAFETY: the H7's DTCM at 0x20010000 is real memory the CPU can use,
    // but no DMA controller reaches it, and the constructor refuses the
    // buffer before programming anything. Nothing else touches it.
    let dtcm: &'static mut [u32] =
        unsafe { core::slice::from_raw_parts_mut(0x2001_0000 as *mut u32, 16) };
    let (lane_refused, channel3, stream3, lane_read1) =
        match channel3.into_two_way_lane::<_, 'B'>(dma2.s0, dtcm, &mut read1[..]) {
            Err(refused) => {
                let refused_dtcm = matches!(
                    refused.error(),
                    ferrohal_stm32::timer::ConfigError::Buffer(
                        ferrohal_stm32::dma::BufferError::Unreachable
                    )
                );
                let (channel, stream, _dtcm, read) = refused.into_parts();
                (refused_dtcm, channel, stream, read)
            }
            Ok(_) => reboot_to_dfu(),
        };
    let (Ok(lane1), Ok(lane2), Ok(lane3), Ok(lane4)) = (
        channel3.into_two_way_lane::<_, 'B'>(stream3, &mut one[..], lane_read1),
        channel4.into_two_way_lane::<_, 'B'>(dma2.s1, &mut two[..], &mut read2[..]),
        channel1.into_two_way_lane::<_, 'A'>(dma2.s2, &mut three[..], &mut read3[..]),
        channel2.into_two_way_lane::<_, 'A'>(dma2.s3, &mut four[..], &mut read4[..]),
    ) else {
        reboot_to_dfu()
    };
    // The waveform generator: TIM3 and TIM5 at different periods refuse;
    // both PWMs and both lane pairs come back, TIM5 re-rated, and the
    // independent pair is what the lane checks use.
    let (awg_refused, tim3, tim5, first, second) =
        match Awg::independent(tim3, tim5, (lane1, lane2), (lane3, lane4)) {
            Err(refused) => {
                let refused_mismatch = matches!(
                    refused.error(),
                    ferrohal_stm32::timer::ConfigError::PeriodMismatch
                );
                let (tim3, mut tim5, first, second) = refused.into_parts();
                let re_rated = tim5.set_frequency(Hertz::khz(600)).is_ok();
                (refused_mismatch && re_rated, tim3, tim5, first, second)
            }
            Ok(_) => reboot_to_dfu(),
        };
    let Ok(mut motors) = Awg::independent(tim3, tim5, first, second) else {
        reboot_to_dfu()
    };

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
    let (scan_refused, adc1, scan_stream, samples) = match adc1.into_scan(dma2.s5, &[], samples) {
        Err(refused) => {
            let refused_empty = matches!(refused.error(), ferrohal_stm32::adc::Error::Sequence);
            let (adc1, stream, samples) = refused.into_parts();
            (refused_empty, adc1, stream, samples)
        }
        Ok(_) => reboot_to_dfu(),
    };
    let Ok(mut scan) = adc1.into_scan(scan_stream, &channels, samples) else {
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
    let sd_wiring = sdmmc::Wiring {
        sdmmc: p.SDMMC1,
        ck,
        cmd,
        d0,
        d1,
        d2,
        d3,
    };
    let (sd_refused, sd_wiring) =
        match Sdmmc::new(&mut rcc, sd_wiring, &sdmmc::Config::new(Hertz::mhz(50))) {
            Err(refused) => {
                let refused_clock = matches!(refused.error(), sdmmc::Error::Clock);
                (refused_clock, refused.into_parts())
            }
            Ok(_) => reboot_to_dfu(),
        };
    let Ok(sd) = Sdmmc::new(&mut rcc, sd_wiring, &sdmmc::Config::new(Hertz::mhz(25))) else {
        reboot_to_dfu()
    };
    // Freed to its wiring and reopened from it: the card suite runs on the
    // reopened controller.
    let sd_wiring = sd.free();
    let (sd_reopened, mut sd) =
        match Sdmmc::new(&mut rcc, sd_wiring, &sdmmc::Config::new(Hertz::mhz(25))) {
            Ok(sd) => (true, sd),
            Err(_) => reboot_to_dfu(),
        };

    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let Ok(usb) = UsbFs::new(&mut rcc, p.USB_OTG_FS, dm, dp) else {
        reboot_to_dfu()
    };
    let mut link = Link::new(usb, "lucid-h7", watchdog, clock.reader());
    let mut imu = Imu {
        spi: &mut imu_spi,
        cs: &mut imu_cs,
        ready: imu_ready.handle(),
        slow: IMU_SLOW,
        fast: IMU_FAST,
    };
    usb_bench::serve(&mut link, cause, |report, link| {
        check!(
            report,
            link,
            rcc_refused,
            "refusals: a config naming Pll1P with no PLL1 refuses, and its token froze these clocks [API-001]"
        );
        check!(
            report,
            link,
            watchdog_refused,
            "refusals: a 1 us watchdog timeout refuses, and its peripheral runs the bench's [API-001]"
        );
        check!(
            report,
            link,
            timebase_refused,
            "refusals: a rate the clock cannot divide to refuses, and the timer keeps time [API-001]"
        );
        check!(
            report,
            link,
            spi_refused,
            "refusals: a 0 Hz SPI config refuses, and the wiring talks to the IMU [API-001]"
        );
        check!(
            report,
            link,
            scan_refused,
            "refusals: an empty ADC sequence refuses, and the parts scan the battery inputs [API-001]"
        );
        check!(
            report,
            link,
            pwm_refused,
            "refusals: a 0 rate refuses, and the timer carries the motor lanes [API-001]"
        );
        check!(
            report,
            link,
            lane_refused,
            "refusals: a DTCM lane buffer no DMA reaches refuses, and the lane carried the frames [API-001]"
        );
        check!(
            report,
            link,
            awg_refused,
            "refusals: mismatched periods refuse, and the pair ran independent after re-rating [API-001]"
        );
        check!(
            report,
            link,
            sd_refused,
            "refusals: a bus speed past the card's default limit refuses, and the wiring opened the real card [API-001]"
        );
        check!(
            report,
            link,
            sd_reopened,
            "sd: freed to its wiring and reopened; the card suite runs on it [API-002]"
        );
        rcc_suite(report, link);
        usb_bench::clock_suite(report, link, OTG_FS_DSTS, CLOCKS.cpu.0);
        usb_bench::uid_suite(report, link);
        usb_bench::revision_suite(report, link);
        imu.suite(report, link);
        let vdda = vdda_mv.unwrap_or(0);
        check!(
            report,
            link,
            (3000..=3600).contains(&vdda),
            "adc: VDDA is {} mV",
            vdda
        );
        check!(
            report,
            link,
            matches!(die_tenths, Ok(100..=800)),
            "adc: die {:?} tenths of a degree",
            die_tenths
        );
        board_adc::suite(
            report,
            link,
            &mut scan,
            &["voltage PC0", "current PC1"],
            vdda,
        );
        sd_suite(report, link, &mut sd);
        motor_lanes::suite(report, link, &mut motors);
        motor_lanes::listen_suite(report, link, &mut motors);
    })
}
