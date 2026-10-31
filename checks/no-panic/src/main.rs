//! Proves FerroHAL's public API has no reachable panic (§5, §13 of the
//! architecture brief), for one chip at a time:
//!
//!     cargo build --release --features stm32f401re
//!     cargo build --release --features stm32f405rg
//!     cargo build --release --features stm32h743vi
//!     cargo build --release --features stm32h753zi
//!
//! `main` calls every public operation, with every input passed through
//! `black_box` so the optimiser cannot fold a check away. The panic handler
//! calls a function that exists nowhere, so the build links only if the
//! optimised program contains no call to it: no panic path survives. A
//! failing build names `ferrohal_panic_is_reachable` as undefined.
//!
//! It is never flashed; linking is the test.

#![no_std]
#![no_main]

use core::{hint::black_box, ptr::addr_of_mut};

use cortex_m_rt::entry;
use embedded_hal::digital::{InputPin, OutputPin, StatefulOutputPin};
use ferrohal_stm32::{
    chip::Peripherals,
    dma::{Direction, FifoMode, Flags, Interrupts, Priority, StreamInstance, Transfer, WordSize},
    uart::{self, Buffers, Parity, StopBits, Uart, UartRx},
};

static mut TX: [u8; 64] = [0; 64];
static mut RX: [u8; 256] = [0; 256];
static mut RX2: [u8; 256] = [0; 256];

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        park()
    };
    let Some((mut rcc, wiring, rx_wiring, mut spare, mut out, mut input)) = chip::wire(p) else {
        park()
    };
    keep(rcc.clocks());
    // The fugit conversions.
    let rate = fugit::HertzU32::from(black_box(rcc.clocks()).hclk);
    keep(ferrohal_stm32::time::Hertz::from(black_box(rate)));

    // GPIO, through the embedded-hal traits and inherently.
    let _ = OutputPin::set_high(&mut out);
    let _ = OutputPin::set_low(&mut out);
    keep(StatefulOutputPin::is_set_high(&mut out));
    keep(StatefulOutputPin::is_set_low(&mut out));
    keep(InputPin::is_high(&mut input));
    keep(InputPin::is_low(&mut input));
    out.set_high();
    keep(out.is_set_high());

    // SAFETY: `main` runs once, and these are its only references.
    let buffers = Buffers {
        tx: black_box(unsafe { &mut *addr_of_mut!(TX) }),
        rx: black_box(unsafe { &mut *addr_of_mut!(RX) }),
    };
    let config = black_box(uart::Config::new(115_200));
    let Ok((mut uart, mut irq)) = Uart::new(&mut rcc, wiring, &config, buffers) else {
        park()
    };
    exercise_uart(&mut uart);

    // A receive-only UART, and the frame check with inversion asked for.
    let sbus = black_box(
        uart::Config::new(100_000)
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(black_box(true)),
    );
    keep(Uart::check::<chip::RxUart>(&black_box(rcc.clocks()), &sbus));
    // SAFETY: as TX and RX above.
    let ring = black_box(unsafe { &mut *addr_of_mut!(RX2) });
    let Ok((mut rx_only, mut rx_irq)) = UartRx::new(&mut rcc, rx_wiring, &sbus, ring) else {
        park()
    };
    let mut chunk = [0u8; 32];
    keep(rx_only.read_chunk(black_box(&mut chunk)));
    keep(rx_only.try_read(black_box(&mut chunk)));
    rx_irq.on_interrupt();
    // The buffer check on its own, MPU walk included on the H7.
    keep(ferrohal_stm32::chip::check_dma_buffer(black_box(&[0u8; 8][..])));
    // The interrupt requirements, checked against the NVIC, and the
    // resource lists.
    let vectors = irq.interrupts();
    keep(vectors.verify());
    keep(rx_irq.interrupts().verify());
    vectors.iter().for_each(keep);
    black_box(&chip::UartWiring::RESOURCES).iter().for_each(keep);
    black_box(&chip::RxOnlyWiring::RESOURCES).iter().for_each(keep);
    irq.on_interrupt();
    irq.on_uart();
    irq.on_rx_dma();
    irq.on_tx_dma();

    // The DMA API, on a stream the UART does not use.
    let mut channel = spare.take_channel();
    keep(channel.flags());
    channel.clear_flags(black_box(Flags::ALL));
    keep(channel.remaining());
    keep(channel.is_running());
    keep(channel.current_target());
    channel.request_stop();
    keep(channel.stop(black_box(1_000)));
    let handle = channel.flag_handle();
    keep(handle.flags());
    handle.clear(black_box(Flags::default()));
    keep(handle.remaining());
    let transfer = black_box(Transfer {
        request: 1,
        #[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
        bufferable: false,
        direction: Direction::MemoryToPeripheral,
        peripheral: 0x4000_0000,
        memory: 0x2400_0000,
        double_buffer: Some(0x2400_0100),
        len: 16,
        size: WordSize::Word,
        memory_increment: true,
        circular: false,
        priority: Priority::Low,
        fifo: FifoMode::Direct,
        interrupts: Interrupts::default(),
    });
    // SAFETY: never run; only linked.
    unsafe {
        keep(channel.start(&transfer));
        channel.set_idle_buffer(black_box(0x2400_0200));
    }

    park()
}

fn exercise_uart(uart: &mut Uart) {
    use embedded_io::{Read, ReadReady, Write, WriteReady};

    let mut buf = [0u8; 32];
    let data = black_box(&[0x55u8; 40][..]);
    keep(uart.tx().write(data));
    // The transmit queue.
    keep(uart.tx().try_send(data));
    uart.tx().poll();
    keep(uart.tx().queued());
    keep(uart.tx().health());
    keep(uart.tx().flush());
    keep(uart.tx().is_idle());
    keep(uart.tx().send_break());
    keep(uart.rx().try_read(black_box(&mut buf)));
    keep(uart.rx().read_within(black_box(&mut buf), black_box(10)));
    keep(uart.rx().available());
    keep(Read::read(uart, black_box(&mut buf)));
    keep(Write::write(uart, data));
    keep(Write::flush(uart));
    keep(uart.read_ready());
    keep(uart.write_ready());
    let config = black_box(
        uart::Config::new(9_600)
            .with_parity(Parity::Even)
            .with_stop_bits(StopBits::Two)
            .with_inverted_rx(true)
            .with_inverted_tx(true),
    );
    keep(uart.set_config(&config));
    let (tx, rx) = uart.halves();
    keep(tx.write(data));
    keep(rx.try_read(&mut buf));
    // The chunk reader, the fault counts and the caller clock.
    keep(rx.read_chunk(black_box(&mut buf)));
    keep(rx.generation());
    keep(rx.health());
    rx.set_clock(Some(&CLOCK));
    keep(rx.read_within(black_box(&mut buf), black_box(10)));
    tx.set_clock(Some(&CLOCK));
    keep(tx.flush());
    uart.set_clock(None);
}

/// Hands a result to the optimiser as used: the check is about which code
/// paths exist, not what they return.
fn keep<T>(value: T) {
    black_box(value);
}

/// A caller clock for the check; any cycle source will do.
struct Clock;

impl uart::CycleClock for Clock {
    fn cycles(&self) -> u32 {
        black_box(0)
    }
}

static CLOCK: Clock = Clock;

fn park() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}

/// A DMA stream the UART does not use, for the raw DMA API.
struct Spare<C: StreamInstance> {
    stream: Option<C>,
}

impl<C: StreamInstance> Spare<C> {
    fn take_channel(&mut self) -> ferrohal_stm32::dma::Channel {
        match self.stream.take() {
            Some(stream) => stream.into_channel(),
            None => park(),
        }
    }
}

#[cfg(feature = "stm32f401re")]
mod chip {
    use ferrohal_stm32::{
        chip::{Peripherals, USART1, USART6},
        dma::Stream,
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{AhbPrescaler, ApbPrescaler, Config, Pll, PllP, PllSource, Rcc, Supply, Sysclk},
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = USART6;
    pub type RxOnlyWiring = RxWiring<USART6, Pin<'A', 12, Alternate<8>>, Stream<2, 1>>;

    const CLOCKS: Config = Config {
        hse: None,
        pll: Some(Pll {
            source: PllSource::Hsi,
            prediv: 16,
            mul: 336,
            divp: PllP::Div4,
            divq: Some(7),
        }),
        sys: Sysclk::PllP,
        ahb_pre: AhbPrescaler::Div1,
        apb1_pre: ApbPrescaler::Div2,
        apb2_pre: ApbPrescaler::Div1,
        supply: Supply::V2_7To3_6,
    };

    pub type UartWiring = Wiring<
        USART1,
        Pin<'A', 9, Alternate<7>>,
        Pin<'A', 10, Alternate<7>>,
        Stream<2, 7>,
        Stream<2, 2>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        Spare<Stream<2, 0>>,
        Pin<'A', 5, Output>,
        Pin<'A', 0, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        let mut tx = a.p9.into_alternate::<7>(&mut a.ctrl);
        tx.set_speed(&mut a.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = a.p10.into_alternate::<7>(&mut a.ctrl);
        rx.set_pull(&mut a.ctrl, core::hint::black_box(Pull::Up));
        let out = a.p5.into_push_pull_output(&mut a.ctrl);
        let input = a.p0.into_input(&mut a.ctrl);
        let rx6 = a.p12.into_alternate::<8>(&mut a.ctrl);
        let _analog = a.p1.into_analog(&mut a.ctrl);
        Some((
            rcc,
            Wiring {
                uart: p.USART1,
                tx,
                rx,
                tx_dma: dma2.s7,
                rx_dma: dma2.s2,
            },
            RxWiring {
                uart: p.USART6,
                rx: rx6,
                rx_dma: dma2.s1,
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

/// The Foxeer F405 V2's ports, from its 8 MHz crystal: HSE in the path.
#[cfg(feature = "stm32f405rg")]
mod chip {
    use ferrohal_stm32::{
        chip::{Peripherals, UART4, USART2},
        dma::Stream,
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Hse, HseMode, Pll, PllP, PllSource, Rcc, Supply,
            Sysclk,
        },
        time::Hertz,
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = UART4;
    pub type RxOnlyWiring = RxWiring<UART4, Pin<'A', 1, Alternate<8>>, Stream<1, 2>>;

    const CLOCKS: Config = Config {
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

    pub type UartWiring = Wiring<
        USART2,
        Pin<'A', 2, Alternate<7>>,
        Pin<'A', 3, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 5>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        Spare<Stream<2, 0>>,
        Pin<'B', 0, Output>,
        Pin<'B', 1, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut a = p.GPIOA.split(&mut rcc);
        let mut b = p.GPIOB.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        let mut tx = a.p2.into_alternate::<7>(&mut a.ctrl);
        tx.set_speed(&mut a.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = a.p3.into_alternate::<7>(&mut a.ctrl);
        rx.set_pull(&mut a.ctrl, core::hint::black_box(Pull::Up));
        let out = b.p0.into_push_pull_output(&mut b.ctrl);
        let input = b.p1.into_input(&mut b.ctrl);
        let rx4 = a.p1.into_alternate::<8>(&mut a.ctrl);
        let _analog = b.p2.into_analog(&mut b.ctrl);
        Some((
            rcc,
            Wiring {
                uart: p.USART2,
                tx,
                rx,
                tx_dma: dma1.s6,
                rx_dma: dma1.s5,
            },
            RxWiring {
                uart: p.UART4,
                rx: rx4,
                rx_dma: dma1.s2,
            },
            Spare {
                stream: Some(dma2.s0),
            },
            out,
            input,
        ))
    }
}

#[cfg(any(feature = "stm32h743vi", feature = "stm32h753zi"))]
mod chip {
    use ferrohal_stm32::{
        chip::{Peripherals, USART2, USART3},
        dma::Stream,
        gpio::{Alternate, Input, Output, Pin, Pull, Speed},
        rcc::{
            AhbPrescaler, ApbPrescaler, Config, Pll, PllSource, Rcc, Supply, Sysclk,
            VoltageScale,
        },
        uart::{RxWiring, Wiring},
    };

    use super::Spare;

    pub type RxUart = USART3;
    pub type RxOnlyWiring = RxWiring<USART3, Pin<'B', 11, Alternate<7>>, Stream<1, 2>>;

    const CLOCKS: Config = Config {
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

    pub type UartWiring = Wiring<
        USART2,
        Pin<'D', 5, Alternate<7>>,
        Pin<'D', 6, Alternate<7>>,
        Stream<1, 1>,
        Stream<1, 0>,
    >;

    pub fn wire(
        p: Peripherals,
    ) -> Option<(
        Rcc,
        UartWiring,
        RxOnlyWiring,
        Spare<Stream<2, 3>>,
        Pin<'B', 0, Output>,
        Pin<'B', 1, Input>,
    )> {
        let mut rcc = p.RCC.freeze(&core::hint::black_box(CLOCKS)).ok()?;
        let mut b = p.GPIOB.split(&mut rcc);
        let mut d = p.GPIOD.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let dma2 = p.DMA2.split(&mut rcc);
        let mut tx = d.p5.into_alternate::<7>(&mut d.ctrl);
        tx.set_speed(&mut d.ctrl, core::hint::black_box(Speed::Medium));
        let mut rx = d.p6.into_alternate::<7>(&mut d.ctrl);
        rx.set_pull(&mut d.ctrl, core::hint::black_box(Pull::Up));
        let out = b.p0.into_push_pull_output(&mut b.ctrl);
        let input = b.p1.into_input(&mut b.ctrl);
        let rx3 = b.p11.into_alternate::<7>(&mut b.ctrl);
        let _analog = b.p2.into_analog(&mut b.ctrl);
        Some((
            rcc,
            Wiring {
                uart: p.USART2,
                tx,
                rx,
                tx_dma: dma1.s1,
                rx_dma: dma1.s0,
            },
            RxWiring {
                uart: p.USART3,
                rx: rx3,
                rx_dma: dma1.s2,
            },
            Spare {
                stream: Some(dma2.s3),
            },
            out,
            input,
        ))
    }
}

#[cfg(not(feature = "locate"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe extern "C" {
        fn ferrohal_panic_is_reachable() -> !;
    }
    // SAFETY: never linked: if this call survives optimisation, the build
    // fails, which is the point.
    unsafe { ferrohal_panic_is_reachable() }
}

/// With `--features locate`, the build links, so the disassembly shows which
/// functions still call into `core::panicking`.
#[cfg(feature = "locate")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    park()
}
