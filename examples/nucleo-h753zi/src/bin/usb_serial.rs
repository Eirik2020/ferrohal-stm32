//! USB serial: a CDC-ACM port on the user USB connector, CN13, that sends
//! back every byte it receives. Open it with any terminal (it shows up as
//! `/dev/ttyACM*` on Linux, a COM port on Windows) and type.
//!
//! Wiring: a USB cable from CN13 to the host, beside the ST-LINK's (CN1).
//! PA11 and PA12 reach CN13 through SB21 and SB22, closed as shipped
//! (UM2407, USB OTG FS).
//!
//! FerroHAL gives `usb-device` the bus (`usb::UsbBus`); the class,
//! `usbd-serial` here, is any `usb-device` class crate. Nothing is
//! interrupt-driven: the main loop polls the device, which must happen at
//! least every few milliseconds while the host talks to it. A firmware with
//! a scheduler binds `UsbFs::vectors()` and polls from there.
//!
//! The USB kernel clock is HSI48, which `UsbFs::new` turns on. It runs
//! untrimmed on this chip (`docs/CHIPS.md`, H7 limits).

#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt_rtt as _;
use ferrohal_stm32::{
    chip::Peripherals,
    gpio::Speed,
    rcc::{
        AhbPrescaler, ApbPrescaler, Config, Hse, HseMode, Pll, PllSource, Supply, Sysclk,
        VoltageScale,
    },
    time::Hertz,
    usb::{
        UsbBus, UsbFs, Wiring,
        usb_device::{
            bus::UsbBusAllocator,
            device::{StringDescriptors, UsbDeviceBuilder, UsbVidPid},
        },
    },
};
use panic_probe as _;
use usbd_serial::{SerialPort, USB_CLASS_CDC};

/// A 400 MHz core and 200 MHz buses from HSE in bypass: the ST-LINK's 8 MHz
/// MCO on PH0 (UM2407, HSE clock). 8 MHz / 1 * 100 = 800 MHz VCO, / 2 =
/// 400 MHz. `validate` checks it against the chip's limits while compiling.
const CLOCK_CONFIG: Config = Config {
    hse: Some(Hse {
        freq: Hertz::mhz(8),
        mode: HseMode::Bypass,
    }),
    pll1: Some(Pll {
        source: PllSource::Hse,
        prediv: 1,
        mul: 100,
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
const _: () = assert!(CLOCK_CONFIG.validate().is_ok());

#[entry]
fn main() -> ! {
    let Some(p) = Peripherals::take() else {
        defmt::panic!("peripherals taken twice")
    };
    let mut rcc = match p.RCC.freeze(&CLOCK_CONFIG) {
        Ok(rcc) => rcc,
        Err(refused) => defmt::panic!("clocks: {}", refused.error()),
    };
    let mut gpioa = p.GPIOA.split(&mut rcc);
    // The data pins are the chip's own; both at very high speed.
    let mut dm = gpioa.p11.into_alternate(&mut gpioa.ctrl);
    dm.set_speed(&mut gpioa.ctrl, Speed::VeryHigh);
    let mut dp = gpioa.p12.into_alternate(&mut gpioa.ctrl);
    dp.set_speed(&mut gpioa.ctrl, Speed::VeryHigh);

    let usb = match UsbFs::new(
        &mut rcc,
        Wiring {
            usb: p.USB_OTG_FS,
            dm,
            dp,
        },
    ) {
        Ok(usb) => usb,
        Err(refused) => defmt::panic!("USB clock: {}", refused.error()),
    };

    // `usb-device` wants the bus and the endpoint memory for the whole
    // program: `singleton!` hands each out once, with no unsafe here.
    let Some(memory) = cortex_m::singleton!(: [u32; 1024] = [0; 1024]) else {
        defmt::panic!("endpoint memory taken twice")
    };
    let Some(bus) = cortex_m::singleton!(: UsbBusAllocator<UsbBus> = UsbBus::new(usb, memory))
    else {
        defmt::panic!("USB bus taken twice")
    };

    let mut serial = SerialPort::new(bus);
    // 0x16c0:0x27dd is the shared test VID:PID for CDC-ACM; a product needs
    // its own.
    let Ok(builder) = UsbDeviceBuilder::new(bus, UsbVidPid(0x16c0, 0x27dd)).strings(&[
        StringDescriptors::default()
            .manufacturer("FerroHAL")
            .product("FerroHAL USB serial example")
            .serial_number("example"),
    ]) else {
        defmt::panic!("USB strings")
    };
    let mut device = builder.device_class(USB_CLASS_CDC).build();
    defmt::info!("usb-serial: open the port and type");

    // Bytes read and not yet written back: `pending[head..head + len]`.
    // Neither the read nor the write blocks, so the loop keeps polling.
    let mut pending = [0u8; 64];
    let (mut head, mut len) = (0, 0);
    loop {
        device.poll(&mut [&mut serial]);
        if len == 0
            && let Ok(count) = serial.read(&mut pending)
        {
            head = 0;
            len = count;
        }
        if len > 0
            && let Ok(count) = serial.write(&pending[head..head + len])
        {
            head += count;
            len -= count;
        }
    }
}
