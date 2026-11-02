//! The real-board bench on the Foxeer F405 V2, reporting over USB (see
//! `hil/common/usb_bench.rs`). The checks: the crystal against the host's
//! start-of-frames, the IMU on SPI1 over DMA with its data-ready on EXTI4, the
//! NOR on SPI2 without DMA, and USB itself through the host's echo.
//!
//! The motor, receiver and ESC telemetry pins stay as reset left them. The
//! NOR checks erase and program only the scratch sector, which FerroWasp
//! keeps for destructive tests, and check that the sectors either side (a
//! configuration slot, the flight log) didn't change.

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
use embedded_hal::spi::SpiBus;
use ferrohal_stm32::{
    adc::{Adc, Config as AdcConfig},
    chip::{Peripherals, SPI1, SPI2, TIM2},
    dma::Stream,
    exti::Edge,
    gpio::{Alternate, Output, Pin, Pull, Speed},
    iwdg::Watchdog,
    rcc::{
        AhbPrescaler, ApbPrescaler, Clocks, Config as ClockConfig, Hse, HseMode, Pll, PllP,
        PllSource, Supply, Sysclk,
    },
    spi::{self, BlockingSpi, BlockingWiring, Buffers, Spi, Wiring},
    time::Hertz,
    timer::Timebase,
    usb::UsbFs,
};

use imu::Imu;
use usb_bench::{Link, Report, WATCHDOG_US, reboot_to_dfu};

/// The endpoint memory `usb_bench` gives the driver: the F4's OTG_FS FIFO.
const EP_WORDS: usize = 320;

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

const RCC_CR: *const u32 = 0x4002_3800 as *const u32;
const RCC_PLLCFGR: *const u32 = 0x4002_3804 as *const u32;
const RCC_CFGR: *const u32 = 0x4002_3808 as *const u32;
const OTG_FS_DSTS: *const u32 = 0x5000_0808 as *const u32;

static mut ADC_SAMPLES: [u16; 3] = [0; 3];
static mut IMU_TX: [u8; 16] = [0; 16];
static mut IMU_RX: [u8; 16] = [0; 16];

/// RCC's own view: the crystal really runs the chip.
fn rcc_suite(report: &mut Report, link: &mut Link) {
    // SAFETY: reads of RCC's status registers.
    let (cr, pllcfgr, cfgr) =
        unsafe { (read_volatile(RCC_CR), read_volatile(RCC_PLLCFGR), read_volatile(RCC_CFGR)) };
    check!(report, link, cr & (1 << 17) != 0, "clocks: HSE ready");
    check!(report, link, cr & (1 << 25) != 0, "clocks: PLL locked");
    check!(report, link, pllcfgr & (1 << 22) != 0, "clocks: PLL fed from HSE");
    check!(report, link, (cfgr >> 2) & 0b11 == 0b10, "clocks: SYSCLK is the PLL");
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
    let Ok(mut imu_spi) = Spi::new(&mut rcc, imu_wiring, &IMU_SLOW, buffers) else {
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
    let mut nor = Nor { spi, cs: nor_cs };

    let mut exti = p.EXTI.split(p.SYSCFG, &mut rcc);
    let mut ready_pin = c.p4.into_input(&mut c.ctrl);
    ready_pin.set_pull(&mut c.ctrl, Pull::Down);
    let mut imu_ready = exti.l4.route::<'C'>(&mut exti.ctrl, Edge::Rising);
    // In EXTI only, so the flag latches: its NVIC vector stays off.
    imu_ready.listen(&mut exti.ctrl);

    // The battery inputs, on ADC1 with the temperature sensor: the sequence
    // and the free stream the flight code uses.
    let Ok(mut adc) = Adc::new(&mut rcc, p.ADC1, &AdcConfig::slow_source()) else {
        reboot_to_dfu()
    };
    let voltage = c.p0.into_analog(&mut c.ctrl);
    let current = c.p1.into_analog(&mut c.ctrl);
    let vdda_mv = adc.vdda_millivolts();
    let die_tenths = adc.temperature_decidegrees();
    let channels = [adc.temperature_channel(), adc.channel(&voltage), adc.channel(&current)];
    // SAFETY: `main` runs once, and this is the only reference.
    let samples: &'static mut [u16] = unsafe { &mut *addr_of_mut!(ADC_SAMPLES) };
    let Ok(mut scan) = adc.into_scan(dma2.s4, &channels, samples) else {
        reboot_to_dfu()
    };

    let mut dm = a.p11.into_alternate::<10>(&mut a.ctrl);
    dm.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let mut dp = a.p12.into_alternate::<10>(&mut a.ctrl);
    dp.set_speed(&mut a.ctrl, Speed::VeryHigh);
    let Ok(usb) = UsbFs::new(&mut rcc, p.USB_OTG_FS, dm, dp) else {
        reboot_to_dfu()
    };
    let mut link = Link::new(usb, "foxeer-f405-v2", watchdog, clock);
    let mut imu = Imu {
        spi: &mut imu_spi,
        cs: &mut imu_cs,
        ready: imu_ready.handle(),
        slow: IMU_SLOW,
        fast: IMU_FAST,
    };
    usb_bench::serve(&mut link, cause, |report, link| {
        rcc_suite(report, link);
        usb_bench::clock_suite(report, link, OTG_FS_DSTS, CLOCKS.sysclk.0);
        usb_bench::uid_suite(report, link);
        imu.suite(report, link);
        nor_suite(report, link, &mut nor);
        let vdda = vdda_mv.unwrap_or(0);
        check!(report, link, (3000..=3600).contains(&vdda), "adc: VDDA is {} mV", vdda);
        check!(report, link, matches!(die_tenths, Ok(100..=800)), "adc: die {:?} tenths of a degree", die_tenths);
        board_adc::suite(report, link, &mut scan, &["temperature", "voltage PC0", "current PC1"], vdda);
    })
}
