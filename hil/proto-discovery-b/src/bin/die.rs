//! Candidate B, the die's glue probe: run A's six die checks (no wiring),
//! under RTIC, each a stage the host can pick.

#![no_std]
#![no_main]

#[path = "../../../stm32f4-discovery/src/board.rs"]
mod board;
#[path = "../harness.rs"]
mod harness;

use defmt_rtt as _;
use panic_probe as _;

#[derive(defmt::Format)]
pub struct Mismatch {
    expected: u32,
    got: u32,
}

/// Survives a reset: `.uninit` is never zeroed by the runtime.
#[unsafe(link_section = ".uninit.proto_reset")]
static mut RESET_MARK: u32 = 0;
const MAGIC: u32 = 0x5245_5345;

fn expect(expected: u32, got: u32) -> Result<(), Mismatch> {
    if expected == got { Ok(()) } else { Err(Mismatch { expected, got }) }
}

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {
    use super::{
        expect,
        harness::{self, Selection, check, stage},
    };
    use crate::board;
    use ferrohal_stm32::{chip::check_dma_buffer, dma::BufferError, rcc::Rcc};

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        rcc: Rcc,
        select: Selection,
    }

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        let select = Selection::read();
        let rcc = stage("clocks", 2, || {
            #[cfg(feature = "hang-clocks")]
            harness::hang();
            match cx.device.RCC.freeze(&board::CLOCK_CONFIG) {
                Ok(rcc) => rcc,
                Err(refused) => defmt::panic!("clocks: {}", refused.error()),
            }
        });
        checks::spawn().ok();
        (Shared {}, Local { rcc, select })
    }

    #[task(priority = 1, local = [rcc, select])]
    async fn checks(cx: checks::Context) {
        let (rcc, select) = (cx.local.rcc, cx.local.select);
        #[cfg(feature = "reset-check")]
        check(select, "S17.reset", 2, || -> Result<(), super::Mismatch> {
            // Passes on the boot after its own reset: a word in RAM the
            // reset leaves alone. (RCC's reset flags cannot tell: probe-rs's
            // own reset after the flash is a software reset too.)
            let mark = &raw mut super::RESET_MARK;
            // SAFETY: one core, no interrupt touches the word; volatile so
            // the write is done before the reset.
            unsafe {
                if mark.read_volatile() == super::MAGIC {
                    mark.write_volatile(0);
                    return Ok(());
                }
                mark.write_volatile(super::MAGIC);
            }
            cortex_m::peripheral::SCB::sys_reset()
        });
        #[cfg(feature = "fault-check")]
        check(select, "S17.fault", 1, || -> Result<(), super::Mismatch> {
            cortex_m::asm::udf()
        });
        check(select, "GLUE-001.dev-id", 1, || {
            expect(0x413, stm32_metapac::DBGMCU.idcode().read().dev_id().into())
        });
        check(select, "RCC-001.clocks", 1, || {
            expect(1, u32::from(rcc.clocks() == board::CLOCKS))
        });
        check(select, "RCC-003.vos", 1, || {
            #[cfg(feature = "hang-check")]
            harness::hang();
            let vos = stm32_metapac::PWR.cr1().read().vos().to_bits();
            let ready = stm32_metapac::PWR.csr1().read().vosrdy();
            expect(1, u32::from(vos & 1 == 1 && ready))
        });
        check(select, "RCC-001.wait-states", 1, || {
            expect(5, stm32_metapac::FLASH.acr().read().latency().to_bits().into())
        });
        check(select, "GLUE-004.ccm", 1, || {
            // SAFETY: CCM RAM's first 64 bytes, looked at by address only.
            let ccm: &[u8] = unsafe { core::slice::from_raw_parts(0x1000_0000 as *const u8, 64) };
            expect(1, u32::from(check_dma_buffer(ccm) == Err(BufferError::Unreachable)))
        });
        check(select, "GLUE-004.sram", 1, || {
            let sram = [0u8; 64];
            expect(1, u32::from(check_dma_buffer(&sram[..]).is_ok()))
        });
        harness::finish(board::CLOCKS.cpu.0)
    }
}
