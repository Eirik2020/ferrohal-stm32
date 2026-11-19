//! Run Q: the waveform generator both ways over one wire, as a bidirectional
//! protocol runs. Needs Z2.
//!
//! TIM3 channel 1 on PC6 and TIM8 channel 2 on PC7 are the two ends of Z2, both
//! active low with pull-ups. One sends while the other releases its pin and reads
//! port C over DMA, then they swap, and the checks recover each sample from what
//! crossed the wire. With the sender's output off, nothing crosses.
//!
//! Then four reader rounds (`docs/design/lanes.md`): TIM3's lane plays
//! a frame from lent memory and reads TIM8's reply into lent memory, first on
//! its own stream, then turning to read in the run that sees the frame end,
//! then through a reader paced by TIM5 (a timer no lane uses) on DMA2 stream
//! 7, then through a reader armed ahead on TIM3's update on DMA2 stream 6.

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/pwm.rs"]
mod pwm;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [CEC])]
mod app {
    use ferrohal_stm32::{
        time::Hertz,
        timer::{Awg, Pwm},
    };
    use stm32_metapac as pac;

    use crate::{
        board, pwm,
        tests::{self, Fail, Report},
    };

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        run: Option<board::RunQ>,
        near: &'static mut [u32],
        far: &'static mut [u32],
        near_reads: &'static mut [u16],
        far_reads: &'static mut [u16],
        frame: &'static mut [u32],
        lent: &'static mut [u16],
    }

    #[init(local = [
        near_buf: [u32; 20] = [0; 20],
        far_buf: [u32; 20] = [0; 20],
        near_reads_buf: [u16; 1024] = [0; 1024],
        far_reads_buf: [u16; 1024] = [0; 1024],
        frame_buf: [u32; 9] = [0; 9],
        lent_buf: [u16; 800] = [0; 800],
    ])]
    fn init(cx: init::Context) -> (Shared, Local) {
        let mut core = cx.core;
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        let run = match board::run_q(cx.device) {
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {}", error),
        };
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        checks::spawn().ok();
        (
            Shared {},
            Local {
                run: Some(run),
                near: cx.local.near_buf,
                far: cx.local.far_buf,
                near_reads: cx.local.near_reads_buf,
                far_reads: cx.local.far_reads_buf,
                frame: cx.local.frame_buf,
                lent: cx.local.lent_buf,
            },
        )
    }

    /// TIM3's run state, compare 1 and CCER, for the reader rounds.
    fn tim3_regs() -> pwm::TimerRegs {
        pwm::TimerRegs {
            enabled: pac::TIM3.cr1().read().cen(),
            compare: pac::TIM3.ccr(0).read().0,
            ccer: pac::TIM3.ccer().read().0,
        }
    }

    /// Reports one reader round: the reply came back whole, with the
    /// requirement `form` names for the reader form used.
    fn reader_round<N, const NC: u8, WN, F, const FC: u8, WF>(
        report: &mut Report,
        name: &str,
        form: &str,
        near: &mut Awg<N, N, (ferrohal_stm32::timer::CompareLane<N, NC, WN>,), ()>,
        far: &mut Awg<F, F, (ferrohal_stm32::timer::CompareLane<F, FC, WF>,), ()>,
        frame: &mut Option<&'static mut [u32]>,
        lent: &mut Option<&'static mut [u16]>,
        at_end: bool,
        cpu: Hertz,
    ) -> Option<pwm::ReaderRound>
    where
        N: ferrohal_stm32::timer::ChannelTimer,
        F: ferrohal_stm32::timer::ChannelTimer,
    {
        let seen = pwm::awg_reader_round(near, 6, far, frame, lent, at_end, &tim3_regs, cpu);
        let kept = seen.as_ref().ok().copied();
        report.check(name, form, seen.map(|_| ()));
        kept
    }

    /// A pass, or a count of what was wrong.
    fn holds(ok: bool) -> Result<(), Fail> {
        if ok {
            Ok(())
        } else {
            Err(Fail::Count { expected: 1, got: 0 })
        }
    }

    #[task(priority = 1, local = [run, near, far, near_reads, far_reads, frame, lent])]
    async fn checks(cx: checks::Context) {
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
        let cpu = board::CLOCKS.cpu;
        let Some(mut run) = cx.local.run.take() else {
            defmt::unreachable!()
        };
        let (Ok(tim3), Ok(tim8)) = (
            Pwm::new(&mut run.rcc, run.tim3, Hertz::khz(150)),
            Pwm::new(&mut run.rcc, run.tim8, Hertz::khz(150)),
        ) else {
            defmt::panic!("TIM3 or TIM8 PWM")
        };
        let near = ferrohal_stm32::timer::LaneWiring::<_, 1, _, _>::new(run.pc6, run.near_lane).assemble_two_way::<'C'>(&tim3, core::mem::take(cx.local.near), core::mem::take(cx.local.near_reads));
        let far = ferrohal_stm32::timer::LaneWiring::<_, 2, _, _>::new(run.pc7, run.far_lane).assemble_two_way::<'C'>(&tim8, core::mem::take(cx.local.far), core::mem::take(cx.local.far_reads));
        let (Ok(near), Ok(far)) = (near, far) else {
            defmt::panic!("the lanes")
        };
        let mut near = Awg::single(tim3, (near,));
        let mut far = Awg::single(tim8, (far,));
        pwm::awg_two_way_check(
            &mut report,
            "TIM3 CH1 PC6 <-> Z2 <-> TIM8 CH2 PC7",
            &mut near,
            6,
            &mut far,
            7,
            cpu,
        );

        let name = "TIM3 CH1 PC6 reads TIM8 CH2 PC7 over Z2";
        let mut frame = Some(core::mem::take(cx.local.frame));
        let mut lent = Some(core::mem::take(cx.local.lent));
        // 1. The lane's own stream, turning to read after the frame's end.
        if let Some(seen) = reader_round(&mut report, name,
            "the reply read whole on the lane's own stream [AWG-016]",
            &mut near, &mut far, &mut frame, &mut lent, false, cpu)
        {
            report.check(name, "a frame plays from lent memory and comes back; the reply fills lent memory, which comes back [AWG-012]",
                holds(seen.read_lent && seen.frame_back && seen.samples_back));
            report.check(name, "prepare_listen's rate is the one the read ran at, and `prepared` names it [AWG-013]",
                holds(seen.prepared_names_it));
            report.check(name, "at the frame's end the timer runs on with the lane's compare at 0 [AWG-018]",
                holds(seen.ran_on && seen.duty_zero));
            report.check(name, "the listen turns the output off with CCxP and CCxNP clear; speak turns it back on, active low [AWG-021]",
                holds(seen.on_before && seen.released_clear && seen.restored));
        }
        // 2. Turning to read in the run that sees the frame end.
        if let Some(seen) = reader_round(&mut report, name,
            "the reply read whole after a listen at the frame's end [AWG-019]",
            &mut near, &mut far, &mut frame, &mut lent, true, cpu)
        {
            report.check(name, "the run that saw the frame end had turned to read [AWG-019]",
                holds(seen.listening_at_end && seen.released_clear));
        }
        // 3. A reader paced by TIM5, a timer no lane uses.
        let pacer = Pwm::new(&mut run.rcc, run.tim5, Hertz::mhz(6));
        match pacer {
            Ok(pacer) => {
                let registered = near.read_ahead_paced::<_, _, 'C'>(&pacer, run.paced);
                pacer.start();
                report.check(name, "a reader paced by TIM5 registers [AWG-017]", holds(registered.is_ok()));
                if let Some(seen) = reader_round(&mut report, name,
                    "the reply read whole by a reader paced by TIM5 [AWG-017]",
                    &mut near, &mut far, &mut frame, &mut lent, true, cpu)
                {
                    report.check(name, "the paced reader reads at its pacer's rate [AWG-017]",
                        holds(seen.rate == pacer.rate()));
                }
                pacer.stop();
            }
            Err(error) => report.check(name, "the TIM5 pacer [AWG-017]", Err(Fail::Timer(error.error()))),
        }
        // 4. A reader armed ahead on TIM3's update, on a generator rebuilt
        // without the paced reader.
        let (tim3, _, lanes, ()) = near.free();
        let mut near = Awg::single(tim3, lanes);
        near.set_active_low(true);
        near.set_outputs(true);
        let registered = near.read_ahead_first_update::<_, 'C'>(run.ahead);
        report.check(name, "a reader armed ahead on TIM3's update registers [AWG-015]", holds(registered.is_ok()));
        reader_round(&mut report, name,
            "the reply read whole by a reader armed ahead on TIM3's update [AWG-015]",
            &mut near, &mut far, &mut frame, &mut lent, true, cpu);
        tests::finish(&report, || {});
    }
}
