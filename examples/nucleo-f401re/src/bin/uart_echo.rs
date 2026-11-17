//! UART over DMA: USART2 sends back every byte it receives, through the
//! ST-LINK's virtual COM port. Open the port the ST-LINK shows on the host
//! (`/dev/ttyACM*` on Linux, a COM port on Windows) at 115200 8N1 and type.
//!
//! - TX PA2, RX PA3, alternate function 7 (DS10086).
//! - DMA1 stream 6 transmits, stream 5 receives (RM0368, DMA1 request
//!   mapping, channel 4).
//!
//! No wiring: SB13 and SB14 connect USART2 to the ST-LINK as shipped
//! (UM1724, USART communication).
//!
//! The driver keeps a receive ring filling by DMA all the time and queues
//! transmit frames; this loop only copies between the two. Nothing is bound
//! to an interrupt: each read also moves the transmit queue on, which is all
//! a polling firmware needs. `rtic-uart-echo` is the same echo served from
//! interrupts.

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use embedded_io::Write as _;
use ferrohal_stm32::{
    chip::Peripherals,
    gpio::{Alternate, Pin, Pull, Speed},
    rcc,
    time::Hertz,
    uart::{Buffers, Config, Uart, Wiring},
};
use panic_probe as _;

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    let mut rcc = match p.RCC.freeze(&rcc::Config::hsi()) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };
    let mut gpioa = p.GPIOA.split(&mut rcc);
    let dma1 = p.DMA1.split(&mut rcc);

    let mut tx: Pin<'A', 2, Alternate<7>> = gpioa.p2.into_alternate(&mut gpioa.ctrl);
    tx.set_speed(&mut gpioa.ctrl, Speed::Medium);
    // Pulled up, so an unconnected RX idles rather than reading noise.
    let mut rx: Pin<'A', 3, Alternate<7>> = gpioa.p3.into_alternate(&mut gpioa.ctrl);
    rx.set_pull(&mut gpioa.ctrl, Pull::Up);
    // The driver's bounds check every part against USART2: a pin or stream
    // that cannot carry it does not build.
    let wiring = Wiring {
        uart: p.USART2,
        tx,
        rx,
        tx_dma: dma1.s6,
        rx_dma: dma1.s5,
    };

    // The DMA buffers live for the whole program: `singleton!` hands each
    // out once, with no unsafe here.
    let (Some(tx_buffer), Some(rx_ring)) = (
        cortex_m::singleton!(: [u8; 256] = [0; 256]),
        cortex_m::singleton!(: [u8; 256] = [0; 256]),
    ) else {
        defmt::panic!("buffers taken twice")
    };
    let buffers = Buffers {
        tx: tx_buffer,
        rx: rx_ring,
    };
    let mut uart = match Uart::new(&mut rcc, wiring, &Config::new(Hertz(115_200)), buffers) {
        Ok(uart) => uart,
        Err(refused) => defmt::panic!("USART2: {}", refused.error()),
    };
    defmt::info!("uart-echo: open the ST-LINK's serial port at 115200 and type");

    let mut buffer = [0u8; 64];
    loop {
        // Whatever has arrived, without waiting; 0 means nothing yet. A line
        // error comes back once, in its place among the bytes.
        match uart.try_read(&mut buffer) {
            Ok(0) => {}
            // Queues every byte, waiting (bounded) for room in the transmit
            // buffer.
            Ok(count) => {
                if let Err(error) = uart.write_all(&buffer[..count]) {
                    defmt::warn!("send: {}", error);
                }
            }
            Err(error) => defmt::warn!("receive: {}", error),
        }
    }
}
