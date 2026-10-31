//! Nucleo-H753ZI (MB1364): the clock tree, and which pins and DMA streams
//! each UART uses. This file is chip-specific; a board on another chip has
//! its own. FerroHAL's routing traits check every choice here at compile
//! time: a pin without USART2's TX function does not build. Through DMAMUX
//! any DMA1 or DMA2 stream can serve USART2; the stream's type carries the
//! request ID.

use core::{
    ptr::addr_of_mut,
    sync::atomic::{AtomicBool, Ordering},
};

use ferrohal_stm32::{
    chip::{Peripherals, USART2},
    dma::Stream,
    gpio::{Alternate, Output, Pin, Pull, Speed},
    rcc::{
        AhbPrescaler, ApbPrescaler, ClockError, Clocks, Config, Pll, PllSource, Rcc, Supply,
        Sysclk, VoltageScale,
    },
    uart,
};

/// 400 MHz core, the most VOS1 allows, from HSI so it works on every board:
/// 64 MHz / 4 = 16 MHz reference, * 50 = 800 MHz VCO (wide range), / 2 =
/// 400 MHz. AXI and AHB at 200 MHz, every APB at 100 MHz.
pub const CLOCK_CONFIG: Config = Config {
    hse: None,
    pll1: Some(Pll {
        source: PllSource::Hsi,
        prediv: 4,
        mul: 50,
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

/// The frequencies `CLOCK_CONFIG` gives, worked out and checked while
/// compiling.
pub const CLOCKS: Clocks = match CLOCK_CONFIG.validate() {
    Ok(clocks) => clocks,
    Err(_) => panic!("CLOCK_CONFIG breaks the H753's limits"),
};

/// The loopback UART: USART2, TX on PD5 and RX on PD6. These are the Zio
/// connector's USART_B pins, CN9 pin 6 (D53) and CN9 pin 4 (D52), side by
/// side, so a jumper cap joins them. DMA1 stream 1 transmits, stream 0
/// receives.
pub type LoopbackUartWiring = uart::Wiring<
    USART2,
    Pin<'D', 5, Alternate<7>>,
    Pin<'D', 6, Alternate<7>>,
    Stream<1, 1>,
    Stream<1, 0>,
>;

/// The DMA buffers' block: the top 16 KiB of AXI SRAM, `DMA_NC` in
/// memory.x.
const DMA_NC_BASE: u32 = 0x2407_C000;

/// Makes the DMA buffer block non-cacheable with MPU region 0, then turns
/// the instruction and data caches on. The MPU and the caches are the
/// firmware's: FerroHAL only checks that its buffers sit in such a block.
pub fn caches(core: &mut cortex_m::Peripherals) {
    const ENABLE: u32 = 1;
    // 2^(13 + 1) bytes.
    const SIZE_16K: u32 = 13 << 1;
    // TEX 001, C 0, B 0: normal memory, not cached.
    const NORMAL_NON_CACHEABLE: u32 = 0b001 << 19;
    const SHAREABLE: u32 = 1 << 18;
    const FULL_ACCESS: u32 = 0b011 << 24;
    const EXECUTE_NEVER: u32 = 1 << 28;
    // PRIVDEFENA: everything outside the region keeps the default map.
    const DEFAULT_MAP: u32 = 1 << 2;
    // SAFETY: init runs before anything uses the region, with the MPU off
    // while it is programmed.
    unsafe {
        core.MPU.ctrl.write(0);
        core.MPU.rnr.write(0);
        core.MPU.rbar.write(DMA_NC_BASE);
        core.MPU.rasr.write(
            EXECUTE_NEVER | FULL_ACCESS | NORMAL_NON_CACHEABLE | SHAREABLE | SIZE_16K | ENABLE,
        );
        core.MPU.ctrl.write(DEFAULT_MAP | ENABLE);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    core.SCB.enable_icache();
    core.SCB.enable_dcache(&mut core.CPUID);
}

#[unsafe(link_section = ".dma_buffers")]
static mut TX: [u8; 64] = [0; 64];
#[unsafe(link_section = ".dma_buffers")]
static mut RX: [u8; 256] = [0; 256];

/// The UART's buffers, in the non-cacheable block. Call once.
pub fn dma_buffers() -> (&'static mut [u8], &'static mut [u8]) {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    if TAKEN.swap(true, Ordering::AcqRel) {
        defmt::panic!("DMA buffers taken twice");
    }
    // SAFETY: the flag makes these the only references. The section is not
    // loaded, so the bytes are cleared here first.
    let (tx, rx) = unsafe { (&mut *addr_of_mut!(TX), &mut *addr_of_mut!(RX)) };
    tx.fill(0);
    rx.fill(0);
    (tx, rx)
}

pub struct Board {
    pub rcc: Rcc,
    pub loopback_uart: LoopbackUartWiring,
    /// LD1, the green user LED.
    pub led: Pin<'B', 0, Output>,
}

pub fn init(p: Peripherals) -> Result<Board, ClockError> {
    let mut rcc = p.RCC.freeze(&CLOCK_CONFIG)?;
    let mut gpiob = p.GPIOB.split(&mut rcc);
    let mut gpiod = p.GPIOD.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);

    let mut tx = gpiod.p5.into_alternate::<7>(&mut gpiod.ctrl);
    tx.set_speed(&mut gpiod.ctrl, Speed::Medium);
    let mut rx = gpiod.p6.into_alternate::<7>(&mut gpiod.ctrl);
    // Without the jumper the pin would float and read noise; pulled up it
    // reads an idle line, and the test reports silence instead.
    rx.set_pull(&mut gpiod.ctrl, Pull::Up);

    let led = gpiob.p0.into_push_pull_output(&mut gpiob.ctrl);

    Ok(Board {
        rcc,
        loopback_uart: uart::Wiring {
            uart: p.USART2,
            tx,
            rx,
            tx_dma: dma1.s1,
            rx_dma: dma1.s0,
        },
        led,
    })
}
