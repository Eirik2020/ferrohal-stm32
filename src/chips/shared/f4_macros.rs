//! Macros the F4 parts' generated glue (`src/chips/gen/`) is written in:
//! fixed-request DMA, one routing row per (controller, stream, channel).
//!
//! Requirements: GLUE-001, GLUE-002

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
                const ENABLE: RccBit = RccBit::new(Bus::Ahb1, $bit);
            }
        )+
    };
}

macro_rules! usarts {
    ($($kind:ident $name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: Info = Info::new(LineRegs::new(pac::$name));

            impl crate::uart::Instance for $name {
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Bus(Bus::$bus);
                const NAME: &'static str = stringify!($name);
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const INTERRUPT: Interrupt = Interrupt::$name;
                const INTERRUPT_NAME: &'static str = stringify!($name);
                const FRACTIONAL_STOP_BITS: bool = fractional_stop!($kind);
                fn info() -> &'static Info {
                    &$info
                }
            }
        )+
    };
}

macro_rules! fractional_stop {
    (usart) => {
        true
    };
    (uart) => {
        false
    };
}

macro_rules! dma {
    ($trait:ident<$usart:ident>: $(($dma:literal, $stream:literal, $request:literal)),+ $(,)?) => {
        $(
            // SAFETY: RM0090 tables 42 and 43 (DMA1 and DMA2 request
            // mapping), matching stm32-metapac 21's metadata.
            unsafe impl $trait<$usart> for Stream<$dma, $stream> {
                const REQUEST: u8 = $request;
            }
        )+
    };
}

/// DMA controllers: `N => token, bus, enable bit;`.
macro_rules! dmas {
    ($($n:literal => $regs:ident, $bus:ident, $bit:literal;)+) => {
        $(
            impl crate::Sealed for Dma<$n> {}
            impl crate::dma::DmaRegisters for Dma<$n> {
                fn regs() -> pac::dma::Dma {
                    pac::$regs
                }
            }
            impl DmaInstance for Dma<$n> {
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
            }
        )+
    };
}
