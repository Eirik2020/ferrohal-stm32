//! The macros every chip's glue builds from. They carry shapes only; the
//! facts (which peripherals, pins, streams and vectors) are the rows each
//! chip file passes them, which `tools/check_glue.py` reads.
//!
//! Names in the expansions (`Pin`, `Alternate`, `Interrupt`) resolve in the
//! chip file that invokes them, so each chip brings its own IP versions.

/// Zero-sized tokens for peripherals with no driver yet.
macro_rules! tokens {
    ($($name:ident),+ $(,)?) => {
        $(
            /// A peripheral token with no driver yet. Zero-sized.
            #[allow(non_camel_case_types)]
            pub struct $name {
                _private: (),
            }

            impl $name {
                const fn new() -> Self {
                    Self { _private: () }
                }

                /// A second token for this peripheral.
                ///
                /// # Safety
                ///
                /// Nothing else may use the peripheral while this token is
                /// in use.
                pub unsafe fn steal() -> Self {
                    Self::new()
                }
            }

            impl crate::Sealed for $name {}
        )+
    };
}

/// The chip's `Peripherals`, one field per token. `$doc` says which
/// peripherals are left out, and why.
macro_rules! peripherals {
    ($doc:literal; $($name:ident: $ty:ty),+ $(,)?) => {
        static TAKEN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

        /// Every peripheral of the chip, once. svd2rust's model: take this
        /// once, then move each token to the driver or task that owns it.
        ///
        #[doc = $doc]
        #[allow(non_snake_case)]
        pub struct Peripherals {
            $(pub $name: $ty,)+
        }

        impl Peripherals {
            /// The tokens, the first time. `None` after that.
            pub fn take() -> Option<Self> {
                if TAKEN.swap(true, core::sync::atomic::Ordering::AcqRel) {
                    None
                } else {
                    // SAFETY: the flag says this is the first time.
                    Some(unsafe { Self::steal() })
                }
            }

            /// The tokens, unconditionally. RTIC calls this when its app
            /// sets `peripherals = true`.
            ///
            /// # Safety
            ///
            /// No token from an earlier `take` or `steal` may still be in
            /// use.
            pub unsafe fn steal() -> Self {
                TAKEN.store(true, core::sync::atomic::Ordering::Release);
                Self {
                    $($name: <$ty>::new(),)+
                }
            }
        }
    };
}

/// Pin routing rows: (port, pin, alternate function).
macro_rules! pins {
    ($trait:ident<$usart:ident>: $(($port:literal, $pin:literal, $af:literal)),+ $(,)?) => {
        $(impl $trait<$usart> for Pin<$port, $pin, Alternate<$af>> {})+
    };
}

/// An SPI row's kernel clock column: `Pll1Q`, or the name of a bus.
macro_rules! spi_kernel {
    (Pll1Q) => {
        crate::spi::Kernel::Pll1Q
    };
    ($bus:ident) => {
        crate::spi::Kernel::Bus(Bus::$bus)
    };
}

/// SPI instance rows: `NAME => INFO, bus, enable bit, kernel clock;`.
macro_rules! spis {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal, $kernel:ident;)+) => {
        $(
            static $info: crate::spi::stream_dma::Info = crate::spi::stream_dma::Info::new(
                crate::spi_ip::Unit::new(pac::$name),
                RccBit::new(Bus::$bus, $bit),
            );

            impl crate::spi::Instance for $name {
                const KERNEL: crate::spi::Kernel = spi_kernel!($kernel);
                const NAME: &'static str = stringify!($name);
                fn info() -> &'static crate::spi::stream_dma::Info {
                    &$info
                }
            }
        )+
    };
}

/// Timer rows: `NAME => bus, enable bit, update vector, counter bits;`.
macro_rules! timers {
    ($($name:ident => $bus:ident, $bit:literal, $update:ident, $bits:tt;)+) => {
        $(
            impl crate::timer::Instance for $name {
                const NAME: &'static str = stringify!($name);
                const BUS: Bus = Bus::$bus;
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const UPDATE: Interrupt = Interrupt::$update;
                const UPDATE_NAME: &'static str = stringify!($update);
                const BITS: u8 = $bits;
                fn core() -> stm32_metapac::timer::TimCore {
                    // SAFETY: the timer's base address; the core registers
                    // sit at the start of every timer's block.
                    unsafe { stm32_metapac::timer::TimCore::from_ptr(pac::$name.as_ptr()) }
                }
            }
            timer_width!($name, $bits);
        )+
    };
}

macro_rules! timer_width {
    ($name:ident, 16) => {};
    ($name:ident, 32) => {
        impl crate::timer::Instance32 for $name {
            fn gp32() -> stm32_metapac::timer::TimGp32 {
                // SAFETY: a 32-bit general-purpose timer's base address.
                unsafe { stm32_metapac::timer::TimGp32::from_ptr(pac::$name.as_ptr()) }
            }
        }
    };
}

/// Channel timer rows: `NAME => channels, plain | break;`.
macro_rules! channel_timers {
    ($($name:ident => $channels:literal, $kind:ident;)+) => {
        $(
            impl crate::timer::ChannelTimer for $name {
                const CHANNELS: u8 = $channels;
                const BREAK: bool = timer_break!($kind);
            }
        )+
    };
}

macro_rules! timer_break {
    (plain) => {
        false
    };
    (break) => {
        true
    };
}

/// Timer channel pin rows: `TIM CHx: (port, pin, alternate function), ...`
/// or `TIM CHxN: ...` for a complementary output.
macro_rules! timer_pins {
    ($tim:ident $signal:ident: $(($port:literal, $pin:literal, $af:literal)),+ $(,)?) => {
        $(timer_pin!($signal, $tim, Pin<$port, $pin, Alternate<$af>>);)+
    };
}

macro_rules! timer_pin {
    (CH1, $tim:ident, $pin:ty) => { impl crate::timer::ChannelPin<$tim, 1> for $pin {} };
    (CH2, $tim:ident, $pin:ty) => { impl crate::timer::ChannelPin<$tim, 2> for $pin {} };
    (CH3, $tim:ident, $pin:ty) => { impl crate::timer::ChannelPin<$tim, 3> for $pin {} };
    (CH4, $tim:ident, $pin:ty) => { impl crate::timer::ChannelPin<$tim, 4> for $pin {} };
    (CH1N, $tim:ident, $pin:ty) => { impl crate::timer::ComplementaryPin<$tim, 1> for $pin {} };
    (CH2N, $tim:ident, $pin:ty) => { impl crate::timer::ComplementaryPin<$tim, 2> for $pin {} };
    (CH3N, $tim:ident, $pin:ty) => { impl crate::timer::ComplementaryPin<$tim, 3> for $pin {} };
}

/// Timer DMA rows. Fixed routing (F4): `TIM CHx: (controller, stream,
/// channel), ...` or `TIM UP: ...`. DMAMUX (H7): `TIM CHx: mux request;`,
/// for every DMA1 and DMA2 stream.
macro_rules! timer_dma {
    ($tim:ident $signal:ident: $(($dma:literal, $stream:literal, $request:literal)),+ $(,)?) => {
        $(
            // SAFETY: the chip's DMA request table, matching stm32-metapac
            // 21's metadata (check_glue).
            timer_dma_impl!($signal, $tim, crate::dma::Stream<$dma, $stream>, $request);
        )+
    };
    ($tim:ident $signal:ident: mux $request:literal) => {
        // SAFETY: the DMAMUX1 request ID, from stm32-metapac 21's metadata
        // (check_glue); through DMAMUX any stream serves it.
        timer_dma_impl!($signal, $tim, crate::dma::Stream<D, N>, $request, D, N);
    };
}

macro_rules! timer_dma_impl {
    (UP, $tim:ident, $stream:ty, $request:literal $(, $d:ident, $n:ident)?) => {
        unsafe impl$(<const $d: u8, const $n: u8>)? crate::timer::UpdateDma<$tim> for $stream
        $(where
            crate::dma::Dma<$d>: crate::dma::DmaInstance,
            crate::dma::Stream<$d, $n>: crate::dma::StreamInterrupt,)?
        {
            const REQUEST: u8 = $request;
        }
    };
    ($signal:ident, $tim:ident, $stream:ty, $request:literal $(, $d:ident, $n:ident)?) => {
        unsafe impl$(<const $d: u8, const $n: u8>)? crate::timer::CompareDma<$tim, { timer_channel!($signal) }> for $stream
        $(where
            crate::dma::Dma<$d>: crate::dma::DmaInstance,
            crate::dma::Stream<$d, $n>: crate::dma::StreamInterrupt,)?
        {
            const REQUEST: u8 = $request;
        }
    };
}

macro_rules! timer_channel {
    (CH1) => { 1 };
    (CH2) => { 2 };
    (CH3) => { 3 };
    (CH4) => { 4 };
}

/// The DMA streams whose peripheral port reaches GPIO: `controller: [streams];`.
macro_rules! gpio_dma {
    ($($dma:literal: [$($stream:literal),+];)+) => {
        $($(impl crate::timer::GpioDma for crate::dma::Stream<$dma, $stream> {})+)+
    };
}

/// The vector each DMA stream raises.
macro_rules! stream_interrupts {
    ($($dma:literal: [$($stream:literal => $vector:ident),+];)+) => {
        $($(
            impl crate::dma::StreamInterrupt for crate::dma::Stream<$dma, $stream> {
                const INTERRUPT: Interrupt = Interrupt::$vector;
                const NAME: &'static str = stringify!($vector);
            }
        )+)+
    };
}

/// Whether all of `buffer` lies in one of `regions`, each (base, length).
pub(crate) fn within(buffer: &[u8], regions: &[(usize, usize)]) -> bool {
    let start = buffer.as_ptr() as usize;
    let end = start.saturating_add(buffer.len());
    regions
        .iter()
        .any(|&(base, len)| start >= base && end <= base.saturating_add(len))
}
