//! The macros every chip's glue builds from. They carry shapes only; the
//! facts (which peripherals, pins, streams and vectors) are the rows each
//! chip file passes them, which `tools/check_glue.py` reads.
//!
//! Names in the expansions (`Pin`, `Alternate`, `Interrupt`) resolve in the
//! chip file that invokes them, so each chip brings its own IP versions.
//!
//! Requirements: GLUE-001, GLUE-002

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
                    Some(unsafe { Self::steal_unchecked() })
                }
            }

            /// The tokens, unconditionally. RTIC calls this when its app
            /// sets `peripherals = true`; the `rtic` feature is what makes
            /// the item exist.
            ///
            /// # Safety
            ///
            /// No token from an earlier `take` or `steal` may still be in
            /// use.
            #[cfg(feature = "rtic")]
            pub unsafe fn steal() -> Self {
                // SAFETY: the caller promised no token set is still in use.
                unsafe { Self::steal_unchecked() }
            }

            /// The tokens without the feature or the flag: the one path
            /// inside the crate.
            ///
            /// # Safety
            ///
            /// As `Peripherals::steal`: no token from an earlier `take` or
            /// `steal` may still be in use.
            pub(crate) unsafe fn steal_unchecked() -> Self {
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
        crate::rcc::Kernel::Pll1Q
    };
    ($bus:ident) => {
        crate::rcc::Kernel::Bus(Bus::$bus)
    };
}

/// SPI instance rows: `NAME => INFO, bus, enable bit, kernel clock;`.
macro_rules! spis {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal, $kernel:ident;)+) => {
        $(
            static $info: crate::spi::Info = crate::spi::Info::new(
                crate::spi_ip::Unit::new(pac::$name),
                RccBit::new(Bus::$bus, $bit),
            );

            impl crate::spi::Instance for $name {
                const KERNEL: crate::rcc::Kernel = spi_kernel!($kernel);
                const NAME: &'static str = stringify!($name);
                fn info() -> &'static crate::spi::Info {
                    &$info
                }
            }
        )+
    };
}

/// I2C instance rows: `NAME => INFO, bus, enable bit;`. The kernel clock
/// is the bus clock, which `freeze` writes the H7's muxes to match.
macro_rules! i2cs {
    ($($name:ident => $info:ident, $bus:ident, $bit:literal;)+) => {
        $(
            static $info: crate::i2c::Info = crate::i2c::Info::new(
                crate::i2c_ip::Unit::new(pac::$name),
                RccBit::new(Bus::$bus, $bit),
            );

            impl crate::i2c::Instance for $name {
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Bus(Bus::$bus);
                const NAME: &'static str = stringify!($name);
                fn info() -> &'static crate::i2c::Info {
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
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Timer(Bus::$bus);
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
    (CH1, $tim:ident, $pin:ty) => {
        impl crate::timer::ChannelPin<$tim, 1> for $pin {}
    };
    (CH2, $tim:ident, $pin:ty) => {
        impl crate::timer::ChannelPin<$tim, 2> for $pin {}
    };
    (CH3, $tim:ident, $pin:ty) => {
        impl crate::timer::ChannelPin<$tim, 3> for $pin {}
    };
    (CH4, $tim:ident, $pin:ty) => {
        impl crate::timer::ChannelPin<$tim, 4> for $pin {}
    };
    (CH1N, $tim:ident, $pin:ty) => {
        impl crate::timer::ComplementaryPin<$tim, 1> for $pin {}
    };
    (CH2N, $tim:ident, $pin:ty) => {
        impl crate::timer::ComplementaryPin<$tim, 2> for $pin {}
    };
    (CH3N, $tim:ident, $pin:ty) => {
        impl crate::timer::ComplementaryPin<$tim, 3> for $pin {}
    };
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
        // SAFETY: the update request ID this row names comes from
        // stm32-metapac 21's metadata, which tools/check_glue.py holds
        // against the chip file.
        unsafe impl$(<const $d: u8, const $n: u8>)? crate::timer::UpdateDma<$tim> for $stream
        $(where
            crate::dma::Dma<$d>: crate::dma::DmaInstance,
            crate::dma::Stream<$d, $n>: crate::dma::StreamInterrupt,)?
        {
            const REQUEST: u8 = $request;
        }
    };
    ($signal:ident, $tim:ident, $stream:ty, $request:literal $(, $d:ident, $n:ident)?) => {
        // SAFETY: the compare request ID this row names comes from
        // stm32-metapac 21's metadata, which tools/check_glue.py holds
        // against the chip file.
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
    (CH1) => {
        1
    };
    (CH2) => {
        2
    };
    (CH3) => {
        3
    };
    (CH4) => {
        4
    };
}

/// The DMA streams whose peripheral port reaches GPIO: `controller: [streams];`.
macro_rules! gpio_dma {
    ($($dma:literal: [$($stream:literal),+];)+) => {
        $($(impl crate::timer::GpioDma for crate::dma::Stream<$dma, $stream> {})+)+
    };
}

/// The full-speed USB port's row: `TOKEN => bus, enable bit, vector, FIFO
/// words, endpoints, core;`, on PA11 and PA12 in alternate function 10.
/// `core` is `fs_core` for a full-speed-only controller, or `hs_core` for
/// the high-speed-capable one the H7's port is, running on its internal
/// full-speed PHY.
macro_rules! usb_fs {
    ($name:ident => $bus:ident, $bit:literal, $vector:ident, $fifo:literal, $endpoints:literal, $core:ident;) => {
        #[cfg(feature = "usb")]
        pub(crate) const USB_FS_HS_CORE: bool = usb_core!($core);
        #[cfg(feature = "usb")]
        pub(crate) struct UsbFsFacts {
            pub registers: *const (),
            pub enable: RccBit,
        }

        // SAFETY: an address and an enable bit, both constant.
        #[cfg(feature = "usb")]
        unsafe impl Sync for UsbFsFacts {}

        #[cfg(feature = "usb")]
        pub(crate) const USB_FS: UsbFsFacts = UsbFsFacts {
            registers: pac::$name.as_ptr() as *const (),
            enable: RccBit::new(Bus::$bus, $bit),
        };
        #[cfg(feature = "usb")]
        pub(crate) const USB_FS_INTERRUPT: Interrupt = Interrupt::$vector;
        #[cfg(feature = "usb")]
        pub(crate) const USB_FS_FIFO_WORDS: usize = $fifo;
        #[cfg(feature = "usb")]
        pub(crate) const USB_FS_ENDPOINTS: usize = $endpoints;
    };
}

// Used only with the `usb` feature on.
#[allow(unused_macros)]
macro_rules! usb_core {
    (fs_core) => {
        false
    };
    (hs_core) => {
        true
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

/// ADC rows: `NAME => common registers, bus, enable bit, internal channels;`.
macro_rules! adcs {
    ($($name:ident => $common:ident, $bus:ident, $bit:literal, $internal:literal;)+) => {
        $(
            impl crate::adc::Instance for $name {
                const NAME: &'static str = stringify!($name);
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Bus(Bus::$bus);
                const INTERNAL: bool = $internal;
                fn unit() -> crate::adc_ip::Unit {
                    crate::adc_ip::Unit::new(pac::$name, pac::$common)
                }
            }
        )+
    };
}

/// An ADC's internal channels: `NAME: temperature sensor, VREFINT`.
macro_rules! adc_internal {
    ($name:ident: $temperature:literal, $vrefint:literal) => {
        impl crate::adc::InternalChannels for $name {
            const TEMPERATURE: u8 = $temperature;
            const VREFINT: u8 = $vrefint;
        }
    };
}

/// An ADC's pins: `NAME: (port, pin, channel), ...`.
macro_rules! adc_pins {
    ($name:ident: $(($port:literal, $pin:literal, $channel:literal)),+ $(,)?) => {
        $(impl crate::adc::AdcPin<$name> for Pin<$port, $pin, crate::gpio::Analog> {
            const CHANNEL: u8 = $channel;
        })+
    };
}

/// An ADC's DMA streams on a fixed-request DMA: `NAME: (controller, stream,
/// channel), ...`.
#[allow(unused_macros)]
macro_rules! adc_dma {
    ($name:ident: $(($dma:literal, $stream:literal, $request:literal)),+ $(,)?) => {
        $(
            // SAFETY: the DMA request table's route for this ADC, from
            // stm32-metapac's metadata.
            unsafe impl crate::adc::AdcDma<$name> for Stream<$dma, $stream> {
                const REQUEST: u8 = $request;
            }
        )+
    };
}

/// An ADC's DMAMUX1 request: any stream of DMA1 or DMA2 serves it.
#[allow(unused_macros)]
macro_rules! adc_dma_mux {
    ($name:ident: mux $request:literal) => {
        // SAFETY: the DMAMUX1 request ID for this ADC, from stm32-metapac's
        // metadata (RM0433 table 121).
        unsafe impl<const D: u8, const N: u8> crate::adc::AdcDma<$name> for Stream<D, N>
        where
            Dma<D>: DmaInstance,
            Stream<D, N>: crate::dma::StreamInterrupt,
        {
            const REQUEST: u8 = $request;
        }
    };
}

/// A timer another can start: `SLAVE: MASTER, internal trigger number`.
macro_rules! timer_trigger {
    ($slave:ident: $master:ident, $itr:literal) => {
        impl crate::timer::TriggeredBy<$master> for $slave {
            const ITR: u8 = $itr;
        }
    };
}

/// SDMMC rows: `NAME => bus, enable bit;`.
#[allow(unused_macros)]
macro_rules! sdmmcs {
    ($($name:ident => $bus:ident, $bit:literal;)+) => {
        $(
            impl crate::sdmmc::Registers for $name {
                fn regs() -> stm32_metapac::sdmmc::Sdmmc {
                    pac::$name
                }
            }

            impl crate::sdmmc::Instance for $name {
                const NAME: &'static str = stringify!($name);
                const ENABLE: RccBit = RccBit::new(Bus::$bus, $bit);
                const KERNEL: crate::rcc::Kernel = crate::rcc::Kernel::Pll1Q;
            }
        )+
    };
}
