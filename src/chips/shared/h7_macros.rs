//! Macros the H7 parts' generated glue (`src/chips/gen/`) is written in:
//! DMAMUX1, one request ID per instance and direction, any DMA1 or DMA2 stream.
//!
//! Requirements: GLUE-001

macro_rules! ports {
    ($($letter:literal => $regs:ident, $bit:literal;)+) => {
        $(
            impl crate::Sealed for Port<$letter> {}

            impl crate::gpio::PortRegisters for Port<$letter> {
                fn regs() -> pac::gpio::Gpio {
                    pac::$regs
                }
            }

            impl PortInstance for Port<$letter> {
                const ENABLE: RccBit = RccBit::new(Bus::Ahb4, $bit);
            }
        )+
    };
}

macro_rules! usarts {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(LineRegs::new(pac::$name));

            impl crate::uart::Instance for $name {
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Bus(Bus::$bus);
                const NAME: &'static str = stringify!($name);
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const INTERRUPT: Interrupt = Interrupt::$name;
                const INTERRUPT_NAME: &'static str = stringify!($name);
                fn info() -> &'static Info {
                    &$info
                }
            }
        )+
    };
}

macro_rules! dma {
    ($rx_trait:ident, $tx_trait:ident; $($usart:ident: rx $rx:literal, tx $tx:literal;)+) => {
        $(
            // SAFETY: the DMAMUX1 request IDs for this instance, from
            // stm32-metapac 21's metadata (RM0433 table 121).
            unsafe impl<const D: u8, const N: u8> $rx_trait<$usart> for Stream<D, N>
            where
                Dma<D>: DmaInstance,
                Stream<D, N>: crate::dma::StreamInterrupt,
            {
                const REQUEST: u8 = $rx;
            }

            // SAFETY: as above.
            unsafe impl<const D: u8, const N: u8> $tx_trait<$usart> for Stream<D, N>
            where
                Dma<D>: DmaInstance,
                Stream<D, N>: crate::dma::StreamInterrupt,
            {
                const REQUEST: u8 = $tx;
            }
        )+
    };
}

/// DMA controllers: `N => token, bus, enable bit, mux first DMAMUX1
/// channel;`. Both reach their requests through DMAMUX1.
macro_rules! dmas {
    ($($n:literal => $regs:ident, $bus:ident, $bit:literal, mux $base:literal;)+) => {
        $(
            impl crate::Sealed for Dma<$n> {}
            impl crate::dma::DmaRegisters for Dma<$n> {
                fn regs() -> pac::dma::Dma {
                    pac::$regs
                }
                fn mux() -> pac::dmamux::Dmamux {
                    pac::DMAMUX1
                }
            }
            impl DmaInstance for Dma<$n> {
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const MUX_BASE: u8 = $base;
            }
        )+
    };
}
