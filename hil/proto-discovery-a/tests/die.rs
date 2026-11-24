//! Run A's die checks (hil/stm32f4-discovery/src/bin/die_0x413.rs) as
//! embedded-test cases: DEV_ID, the clock tree at 168 MHz, VOS, wait states,
//! and the CCM/SRAM DMA reachability glue. The UART routes are left out.
//! PROTOTYPE, candidate A (audit step 6).

#![no_std]
#![no_main]

#[path = "../../stm32f4-discovery/src/board.rs"]
mod board;

use defmt_rtt as _;

/// A failed comparison, printed by embedded-test through defmt.
#[derive(defmt::Format)]
pub struct Mismatch {
    pub expected: u32,
    pub got: u32,
}

pub fn expect(expected: u32, got: u32) -> Result<(), Mismatch> {
    if expected == got { Ok(()) } else { Err(Mismatch { expected, got }) }
}

#[embedded_test::tests(default_timeout = 3)]
mod tests {
    use super::{Mismatch, expect};
    use crate::board;
    use ferrohal_stm32::{
        chip::{Peripherals, check_dma_buffer},
        dma::BufferError,
        rcc::Rcc,
    };

    pub struct State {
        rcc: Rcc,
    }

    /// Runs before every test, after the host's reset: the clock tree.
    #[init]
    fn init() -> State {
        #[cfg(feature = "hang-clocks")]
        loop {
            cortex_m::asm::nop();
        }
        #[allow(unreachable_code)]
        let Some(p) = Peripherals::take() else { defmt::panic!("peripherals taken") };
        match p.RCC.freeze(&board::CLOCK_CONFIG) {
            Ok(rcc) => State { rcc },
            Err(refused) => defmt::panic!("clocks: {}", refused.error()),
        }
    }

    /// GLUE-001
    #[test]
    fn dev_id_is_0x413() -> Result<(), Mismatch> {
        expect(0x413, u32::from(stm32_metapac::DBGMCU.idcode().read().dev_id()))
    }

    /// RCC-001
    #[test]
    fn freeze_gives_the_solved_clocks(state: State) -> Result<(), Mismatch> {
        expect(1, u32::from(state.rcc.clocks() == board::CLOCKS))
    }

    /// RCC-003
    #[test]
    fn voltage_scale_1_ready(_state: State) -> Result<(), Mismatch> {
        #[cfg(feature = "hang-check")]
        loop {
            cortex_m::asm::nop();
        }
        #[allow(unreachable_code)]
        let vos = stm32_metapac::PWR.cr1().read().vos().to_bits();
        let ready = stm32_metapac::PWR.csr1().read().vosrdy();
        expect(1, u32::from(vos & 0b01 == 0b01 && ready))
    }

    /// RCC-001
    #[test]
    fn five_wait_states_at_168_mhz(_state: State) -> Result<(), Mismatch> {
        expect(5, u32::from(stm32_metapac::FLASH.acr().read().latency().to_bits()))
    }

    /// GLUE-004
    #[test]
    fn ccm_buffer_refused() -> Result<(), Mismatch> {
        // SAFETY: CCM RAM's first 64 bytes, looked at by address only.
        let ccm: &[u8] = unsafe { core::slice::from_raw_parts(0x1000_0000 as *const u8, 64) };
        expect(1, u32::from(check_dma_buffer(ccm) == Err(BufferError::Unreachable)))
    }

    /// GLUE-004
    #[test]
    fn sram_buffer_accepted() -> Result<(), Mismatch> {
        let sram = cortex_m::singleton!(: [u8; 64] = [0; 64]).unwrap();
        expect(1, u32::from(check_dma_buffer(&sram[..]).is_ok()))
    }

    /// Section 17: can a test span a reset? Asks for one and expects to
    /// see the next boot pass this test.
    #[cfg(feature = "reset-check")]
    #[test]
    fn a_reset_in_a_test() {
        cortex_m::peripheral::SCB::sys_reset()
    }

    /// Section 17: a HardFault in one test, then do the later tests run?
    #[cfg(feature = "fault-check")]
    #[test]
    fn a_fault_in_a_test() {
        cortex_m::asm::udf()
    }
}
