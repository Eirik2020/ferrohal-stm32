//! `embedded-io` on the UART: `Read` and `ReadReady` on every UART, `Write`
//! and `WriteReady` on a full one. `write_all` and `read_exact` are
//! FerroHAL's own: the trait's defaults panic on a `write` that returns
//! `Ok(0)` and slice by the count a `read` returns.
//!
//! Requirements: UART-002, API-001

use embedded_io::ReadExactError;

use super::{Instance, Parts, RxDma, RxPin, TxDma, TxPin, Uart};
use crate::uart::{Error, Wiring};

impl<W: Parts> embedded_io::ErrorType for Uart<W> {
    type Error = Error;
}

impl<W: Parts> embedded_io::Read for Uart<W> {
    /// Blocks until at least one byte has arrived, as the trait requires.
    /// [`Uart::read_within`] is the bounded form.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        loop {
            let len = self.try_read(buf)?;
            if len > 0 || buf.is_empty() {
                return Ok(len);
            }
        }
    }

    /// Blocks until `buf` is full, as the trait requires; the first error
    /// ends it. Never panics.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), ReadExactError<Error>> {
        let mut filled = 0;
        while let Some(rest) = buf.get_mut(filled..) {
            if rest.is_empty() {
                return Ok(());
            }
            match embedded_io::Read::read(self, rest) {
                Ok(0) => return Err(ReadExactError::UnexpectedEof),
                Ok(len) => filled = filled.saturating_add(len),
                Err(error) => return Err(ReadExactError::Other(error)),
            }
        }
        // A read never reports more than it was given.
        Ok(())
    }
}

impl<W: Parts> embedded_io::ReadReady for Uart<W> {
    fn read_ready(&mut self) -> Result<bool, Error> {
        Ok(self.available()? > 0)
    }
}

impl<U, TxP, RxP, TxD, RxD> embedded_io::Write for Uart<Wiring<U, TxP, RxP, TxD, RxD>>
where
    U: Instance,
    TxP: TxPin<U>,
    RxP: RxPin<U>,
    TxD: TxDma<U>,
    RxD: RxDma<U>,
{
    fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        Uart::write(self, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        Uart::flush(self)
    }

    /// Queues all of `buf`, frame by frame, waiting for room as `write`
    /// does. A `write` that takes nothing from a non-empty buffer, which
    /// this driver never does, ends it with [`Error::Halted`] instead of the
    /// trait's panic.
    fn write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        let mut sent = 0;
        while let Some(rest) = buf.get(sent..) {
            if rest.is_empty() {
                return Ok(());
            }
            match Uart::write(self, rest)? {
                0 => return Err(Error::Halted),
                len => sent = sent.saturating_add(len),
            }
        }
        Ok(())
    }
}

impl<U, TxP, RxP, TxD, RxD> embedded_io::WriteReady for Uart<Wiring<U, TxP, RxP, TxD, RxD>>
where
    U: Instance,
    TxP: TxPin<U>,
    RxP: RxPin<U>,
    TxD: TxDma<U>,
    RxD: RxDma<U>,
{
    fn write_ready(&mut self) -> Result<bool, Error> {
        Uart::write_ready(self)
    }
}
