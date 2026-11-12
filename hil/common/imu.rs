//! A flight controller's IMU, on the SPI and EXTI routes its flight code
//! uses. WHO_AM_I for any of the parts FerroWasp knows; the whole data path
//! for the ICM-42688-P.

use core::convert::Infallible;

use embedded_hal::digital::OutputPin;
use ferrohal_stm32::{
    exti::ExtiHandle,
    spi::{self, Instance, Spi},
};

use crate::usb_bench::{Link, Report};

const READ: u8 = 0x80;
const WHO_AM_I: u8 = 0x75;
const ICM42688P: u8 = 0x47;
const MPU6500: u8 = 0x70;
const MPU6000: u8 = 0x68;

/// The IMU's bus, chip select and data-ready line.
pub struct Imu<'a, U, Cs> {
    pub spi: &'a mut Spi<U>,
    pub cs: &'a mut Cs,
    /// Routed and listening in EXTI, with its NVIC vector off, so the flag
    /// latches.
    pub ready: ExtiHandle,
    /// The register speed every part takes.
    pub slow: spi::Config,
    /// The speed the ICM-42688-P runs at; the MPUs' registers stop at 1 MHz.
    pub fast: spi::Config,
}

impl<U: Instance, Cs: OutputPin<Error = Infallible>> Imu<'_, U, Cs> {
    /// One read of WHO_AM_I over DMA, finished by polling.
    fn who_am_i(&mut self, link: &mut Link) -> Result<u8, spi::Error> {
        let _ = self.cs.set_low();
        let mut result = self.spi.start(&[WHO_AM_I | READ, 0]);
        if result.is_ok() {
            let start = link.now();
            result = loop {
                link.service();
                if let Some(done) = self.spi.poll() {
                    break done.map(|_| ());
                }
                if link.now().wrapping_sub(start) > 10_000 {
                    self.spi.abort();
                    break Err(spi::Error::Timeout);
                }
            };
        }
        let _ = self.cs.set_high();
        result?;
        Ok(self.spi.received().get(1).copied().unwrap_or(0))
    }

    fn write(&mut self, register: u8, value: u8) -> bool {
        let _ = self.cs.set_low();
        let ok = self.spi.transfer(&[register, value]).is_ok();
        let _ = self.cs.set_high();
        ok
    }

    fn read(&mut self, register: u8) -> Option<u8> {
        let _ = self.cs.set_low();
        let value = self
            .spi
            .transfer(&[register | READ, 0])
            .ok()
            .and_then(|rx| rx.get(1).copied());
        let _ = self.cs.set_high();
        value
    }

    pub fn suite(&mut self, report: &mut Report, link: &mut Link) {
        let id = self.who_am_i(link);
        match id {
            Ok(byte) => check!(
                report,
                link,
                matches!(byte, ICM42688P | MPU6500 | MPU6000),
                "imu: WHO_AM_I over DMA is {:#04x} at {} Hz",
                byte,
                self.spi.frequency().0
            ),
            Err(error) => check!(report, link, false, "imu: WHO_AM_I over DMA: {:?}", error),
        }
        let mut same = true;
        for _ in 0..100 {
            same &= self.who_am_i(link) == id;
        }
        check!(report, link, same, "imu: 100 more DMA reads say the same");

        let blocking = self.read(WHO_AM_I);
        check!(
            report,
            link,
            blocking == id.ok(),
            "imu: the blocking path reads {:?}",
            blocking
        );

        if id == Ok(ICM42688P) {
            let changed = self.spi.set_config(&self.fast).is_ok();
            let mut same = changed;
            for _ in 0..100 {
                same &= self.who_am_i(link) == id;
            }
            check!(
                report,
                link,
                same,
                "imu: 100 DMA reads at {} Hz",
                self.spi.frequency().0
            );
            self.data_suite(report, link);
            let _ = self.spi.set_config(&self.slow);
        } else {
            link.line(format_args!(
                "INFO imu: not an ICM-42688-P, data path not run"
            ));
        }
        let health = self.spi.health();
        check!(
            report,
            link,
            health.dma_errors == 0
                && health.overruns == 0
                && health.mode_faults == 0
                && health.timeouts == 0,
            "imu: {:?}",
            health
        );
    }

    /// The flight path on the real sensor, set up as FerroWasp sets it up:
    /// the data-ready pulse sets the EXTI line, which starts a DMA read of
    /// temperature, accelerometer and gyroscope. Polled here: the pending flag
    /// and the driver's completion are what a handler would see. Ends with a
    /// soft reset, leaving the sensor as power-on did.
    fn data_suite(&mut self, report: &mut Report, link: &mut Link) {
        const DEVICE_CONFIG: u8 = 0x11;
        const INT_CONFIG: u8 = 0x14;
        const TEMP_DATA1: u8 = 0x1D;
        const INT_STATUS: u8 = 0x2D;
        const INTF_CONFIG0: u8 = 0x4C;
        const PWR_MGMT0: u8 = 0x4E;
        const GYRO_CONFIG0: u8 = 0x4F;
        const ACCEL_CONFIG0: u8 = 0x50;
        const GYRO_ACCEL_CONFIG0: u8 = 0x52;
        const INT_CONFIG1: u8 = 0x64;
        const INT_SOURCE0: u8 = 0x65;
        /// ±16 g, the full scale `ACCEL_CONFIG0` below picks with 2 kHz.
        const LSB_PER_G: u64 = 2048;
        const RATE_HZ: u32 = 2000;
        const SAMPLES: u32 = 400;

        let mut written = self.write(DEVICE_CONFIG, 1);
        link.settle(2_000);
        let status = self.read(INT_STATUS);
        check!(
            report,
            link,
            written && matches!(status, Some(status) if status & (1 << 4) != 0),
            "imu: soft reset done, status {:?}",
            status
        );

        written = self.write(INTF_CONFIG0, 0x33);
        written &= self.write(GYRO_CONFIG0, 0x05);
        written &= self.write(ACCEL_CONFIG0, 0x05);
        written &= self.write(GYRO_ACCEL_CONFIG0, 0x11);
        // The last of the configuration: no writes for 200 us after it.
        written &= self.write(PWR_MGMT0, 0x0F);
        link.settle(1_000);
        written &= self.write(INT_CONFIG, 0x03);
        written &= self.write(INT_CONFIG1, 0);
        written &= self.write(INT_SOURCE0, 1 << 3);
        link.settle(49_000);
        check!(
            report,
            link,
            written,
            "imu: configured for 2 kHz data ready on INT1"
        );

        // The rate, with nothing else in the way.
        self.ready.clear();
        let start = link.now();
        let mut pulses = 0u32;
        while link.now().wrapping_sub(start) < 200_000 {
            link.service();
            if self.ready.is_pending() {
                self.ready.clear();
                pulses += 1;
            }
        }
        check!(
            report,
            link,
            pulses.abs_diff(RATE_HZ / 5) <= RATE_HZ / 100,
            "imu: {} data-ready edges in 200 ms on EXTI line {}, want {} within 5%",
            pulses,
            self.ready.line(),
            RATE_HZ / 5
        );

        // Each pulse starts a burst, as in flight.
        let mut burst = [0u8; 15];
        burst[0] = TEMP_DATA1 | READ;
        let mut done = 0u32;
        let mut failures = 0u32;
        let mut slowest_us = 0;
        let mut magnitude_squared = 0u64;
        let mut temperature = 0i32;
        let start = link.now();
        self.ready.clear();
        while done < SAMPLES && link.now().wrapping_sub(start) < 1_000_000 {
            link.service();
            if !self.ready.is_pending() {
                continue;
            }
            self.ready.clear();
            let began = link.now();
            let _ = self.cs.set_low();
            let mut result = self.spi.start(&burst);
            if result.is_ok() {
                result = loop {
                    if let Some(finished) = self.spi.poll() {
                        break finished.map(|_| ());
                    }
                    if link.now().wrapping_sub(began) > 1_000 {
                        self.spi.abort();
                        break Err(spi::Error::Timeout);
                    }
                };
            }
            let _ = self.cs.set_high();
            slowest_us = slowest_us.max(link.now().wrapping_sub(began));
            let rx = self.spi.received();
            if result.is_err() || rx.len() != burst.len() {
                failures += 1;
                continue;
            }
            let word = |at: usize| i64::from(i16::from_be_bytes([rx[at], rx[at + 1]]));
            temperature = word(1) as i32;
            let (x, y, z) = (word(3), word(5), word(7));
            magnitude_squared += (x * x + y * y + z * z) as u64;
            done += 1;
        }
        let elapsed_us = link.now().wrapping_sub(start);
        check!(
            report,
            link,
            done == SAMPLES && failures == 0,
            "imu: {} bursts of 15 bytes at {} Hz, {} failed, slowest {} us",
            done,
            self.spi.frequency().0,
            failures,
            slowest_us
        );
        let want_us = SAMPLES * 1_000_000 / RATE_HZ;
        check!(
            report,
            link,
            elapsed_us.abs_diff(want_us) <= want_us / 20,
            "imu: they took {} us, want {} within 5%: none missed",
            elapsed_us,
            want_us
        );
        let milli_g = (magnitude_squared / u64::from(done.max(1))).isqrt() * 1000 / LSB_PER_G;
        check!(
            report,
            link,
            (800..=1200).contains(&milli_g),
            "imu: accelerometer reads {} mg: gravity, if the board isn't being moved",
            milli_g
        );
        // The datasheet: 25 C + raw / 132.48.
        let celsius = 25 + temperature * 100 / 13248;
        check!(
            report,
            link,
            (0..=90).contains(&celsius),
            "imu: {} C inside",
            celsius
        );

        let _ = self.write(DEVICE_CONFIG, 1);
        link.settle(2_000);
    }
}
