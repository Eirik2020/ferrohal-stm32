//! UART under RTIC: `uart-echo`'s echo through the ST-LINK's virtual COM
//! port (USART2, 115200 8N1), served from interrupts, with LD2 toggling on
//! each line the host sends. Same pins, streams and wiring as `uart-echo`.
//!
//! FerroHAL defines no handler and never touches the NVIC: RTIC binds the
//! vectors and enables them. The `Uart` is one shared resource. Its
//! [`vectors()`](ferrohal_stm32::uart::Uart::vectors) are the vectors to
//! bind, each handler calls `on_interrupt` under RTIC's lock, and the
//! `Events` it returns say when to wake the task that reads.
//!
//! | Vector | Serves | Priority |
//! | --- | --- | --- |
//! | `USART2` (the line), `DMA1_STREAM5` (receive), `DMA1_STREAM6` (transmit) | `Uart::on_interrupt` | 2 |
//! | `SPI1`, unused by this firmware | RTIC, dispatching the priority 1 tasks | 1 |

#![no_std]
#![no_main]

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use ferrohal_stm32::{
        dma::Stream,
        gpio::{Alternate, Output, Pin, Pull, Speed},
        rcc,
        time::Hertz,
        uart::{Buffers, Config, Uart, Vectors, Wiring},
    };

    /// USART2 on the ST-LINK's pins and its DMA1 streams.
    type Vcp = Wiring<
        ferrohal_stm32::chip::USART2,
        Pin<'A', 2, Alternate<7>>,
        Pin<'A', 3, Alternate<7>>,
        Stream<1, 6>,
        Stream<1, 5>,
    >;

    #[shared]
    struct Shared {
        uart: Uart<Vcp>,
    }

    #[local]
    struct Local {
        led: Pin<'A', 5, Output>,
        vectors: Vectors,
    }

    #[init(local = [
        tx_buffer: [u8; 256] = [0; 256],
        rx_ring: [u8; 256] = [0; 256],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let p = cx.device;
        let mut rcc = match p.RCC.freeze(&rcc::Config::hsi()) {
            Ok(rcc) => rcc,
            Err(refused) => defmt::panic!("clocks: {}", refused.error()),
        };
        let mut gpioa = p.GPIOA.split(&mut rcc);
        let dma1 = p.DMA1.split(&mut rcc);
        let led = gpioa.p5.into_push_pull_output(&mut gpioa.ctrl);
        let mut tx = gpioa.p2.into_alternate(&mut gpioa.ctrl);
        tx.set_speed(&mut gpioa.ctrl, Speed::Medium);
        let mut rx = gpioa.p3.into_alternate(&mut gpioa.ctrl);
        rx.set_pull(&mut gpioa.ctrl, Pull::Up);
        let wiring: Vcp = Wiring {
            uart: p.USART2,
            tx,
            rx,
            tx_dma: dma1.s6,
            rx_dma: dma1.s5,
        };
        // RTIC's init locals are `&'static mut`: the DMA buffers' lifetime.
        let buffers = Buffers {
            tx: cx.local.tx_buffer,
            rx: cx.local.rx_ring,
        };
        let uart = match Uart::new(&mut rcc, wiring, &Config::new(Hertz(115_200)), buffers) {
            Ok(uart) => uart,
            Err(refused) => defmt::panic!("USART2: {}", refused.error()),
        };
        let vectors = uart.vectors();
        if started::spawn().is_err() {
            defmt::panic!("started spawned twice")
        }
        (Shared { uart }, Local { led, vectors })
    }

    /// Runs once RTIC has enabled the bound vectors: `verify` reads the NVIC
    /// and reports any of the UART's vectors left unbound.
    #[task(priority = 1, local = [vectors])]
    async fn started(cx: started::Context) {
        match cx.local.vectors.verify() {
            Ok(()) => defmt::info!("rtic-uart-echo: open the ST-LINK's serial port and type"),
            Err(error) => defmt::error!("a UART vector is not bound: {}", error),
        }
    }

    // One handler per vector, each calling the same `on_interrupt`.

    #[task(binds = USART2, priority = 2, shared = [uart])]
    fn usart2(mut cx: usart2::Context) {
        cx.shared.uart.lock(serve);
    }

    #[task(binds = DMA1_STREAM5, priority = 2, shared = [uart])]
    fn usart2_rx(mut cx: usart2_rx::Context) {
        cx.shared.uart.lock(serve);
    }

    #[task(binds = DMA1_STREAM6, priority = 2, shared = [uart])]
    fn usart2_tx(mut cx: usart2_tx::Context) {
        cx.shared.uart.lock(serve);
    }

    /// Serves the UART, and wakes `echo` when there are bytes to read: the
    /// line went idle (the host paused) or half the ring filled.
    fn serve(uart: &mut Uart<Vcp>) {
        let events = uart.on_interrupt();
        if events.idle || events.filled || events.fault {
            // Already pending is fine: that run reads these bytes too.
            echo::spawn().ok();
        }
    }

    /// Sends back what arrived, a chunk at a time, under the lock.
    #[task(priority = 1, shared = [uart], local = [led])]
    async fn echo(mut cx: echo::Context) {
        let mut buffer = [0u8; 64];
        let mut line = false;
        cx.shared.uart.lock(|uart| {
            loop {
                match uart.try_read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        line |= buffer[..count].contains(&b'\r');
                        // Queued whole, or not at all when the transmit buffer
                        // is full: an echo that falls behind drops, never blocks.
                        if let Err(error) = uart.try_send(&buffer[..count]) {
                            defmt::warn!("send: {}", error);
                        }
                    }
                    Err(error) => {
                        defmt::warn!("receive: {}", error);
                        break;
                    }
                }
            }
        });
        if line {
            if cx.local.led.is_set_high() {
                cx.local.led.set_low();
            } else {
                cx.local.led.set_high();
            }
        }
    }
}
