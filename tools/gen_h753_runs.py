"""Writes the Nucleo-H753ZI HIL run binaries from one template.

    python3 -I tools/gen_h753_runs.py

The four runs (hil/nucleo-h753zi-loopback/src/bin/run_[a-d].rs) differ only
in which UARTs they open, their interrupt vectors, their frames and run A's
cache phase, so they are generated from `RUNS` below rather than kept in
step by hand. Edit `RUNS` (and the board's wiring in board.rs), rerun this,
and commit the generated files with the change.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "hil" / "nucleo-h753zi-loopback" / "src" / "bin"

# run, summary lines, uarts: (field, label, line vector, rx dma vector, tx dma vector, config expr)
RUNS = {
    "a": (
        [
            "Run A: the TBS Lucid H7's UARTs at its rates, on its DMA streams,",
            "under its clock tree. USART6 (CRSF) is on the Lucid's pins, PC6 and",
            "PC7; USART1 (SBUS) and USART3 (MSP) are on pins the Zio headers carry.",
            "USART2 is a control at 115200.",
            "",
            "First with the data cache off, the Lucid's policy: the full check",
            "list on each UART, then all four streaming at once. Then with both",
            "caches on: buffers in cached RAM and DTCM must be refused, and the",
            "four must stream at once again.",
        ],
        [
            ("usart6", "USART6 PC6->PC7, CRSF 420000", "USART6", "DMA1_STREAM0", "DMA1_STREAM6", "CRSF"),
            ("usart1", "USART1 PB6->PB7, SBUS 100000 8E2", "USART1", "DMA2_STREAM4", "DMA2_STREAM5", "SBUS"),
            ("usart3", "USART3 PB10->PB11, MSP 115200", "USART3", "DMA1_STREAM1", "DMA1_STREAM3", "MSP"),
            ("usart2", "USART2 PD5->PD6, control", "USART2", "DMA1_STREAM2", "DMA1_STREAM4", "MSP"),
        ],
    ),
    "b": (
        ["Run B: UART4, UART5, UART7 and USART3 on their first pin pairs, and",
         "the DMA streams run A leaves free. The full check list on each, then",
         "all four streaming at once."],
        [
            ("uart4", "UART4 PB9->PB8", "UART4", "DMA1_STREAM5", "DMA1_STREAM7", "PLAIN"),
            ("uart5", "UART5 PC12->PD2", "UART5", "DMA2_STREAM0", "DMA2_STREAM1", "PLAIN"),
            ("uart7", "UART7 PF7->PF6", "UART7", "DMA2_STREAM2", "DMA2_STREAM3", "PLAIN"),
            ("usart3", "USART3 PC10->PC11", "USART3", "DMA2_STREAM6", "DMA2_STREAM7", "PLAIN"),
        ],
    ),
    "c": (
        ["Run C: UART4 and UART7 on alternate pins, UART7 with its streams on",
         "different controllers. The full check list on each, then both",
         "streaming at once. (UART5's PB13 would belong here, but this board",
         "does not route it to the Zio header.)"],
        [
            ("uart4", "UART4 PD1->PD0", "UART4", "DMA2_STREAM7", "DMA2_STREAM0", "PLAIN"),
            ("uart7", "UART7 PE8->PE7", "UART7", "DMA1_STREAM5", "DMA2_STREAM5", "PLAIN"),
        ],
    ),
    "d": (
        ["Run D: UART4 on the PC10/PC11 link USART3 uses in run B, and UART7 on",
         "its JTAG pins, PA15 and PB3. The full check list on each, then both",
         "streaming at once."],
        [
            ("uart4", "UART4 PC10->PC11", "UART4", "DMA2_STREAM1", "DMA1_STREAM2", "PLAIN"),
            ("uart7", "UART7 PA15->PB3", "UART7", "DMA1_STREAM3", "DMA2_STREAM6", "PLAIN"),
        ],
    ),
}

for run, (summary, uarts) in RUNS.items():
    doc = "\n".join(f"//! {l}".rstrip() for l in summary)
    table = "\n".join(
        f"//! | `{line}`, `{dma}`, `{txdma}` | {label.split(',')[0]}'s `UartIrq` | 2 |"
        for _, label, line, dma, txdma, _ in uarts
    )
    shared = "\n".join(f"        #[lock_free]\n        irq_{f}: UartIrq," for f, *_ in uarts)
    local_uarts = "\n".join(f"        {f}: Uart," for f, *_ in uarts)
    opens = "\n".join(
        f"        let (uart_{f}, irq_{f}) =\n            board::open(&mut run.rcc, \"{label}\", run.{f}, &{cfg}, buffers.next().unwrap());"
        for f, label, _, _, _, cfg in uarts
    )
    shared_init = ", ".join(f"irq_{f}" for f, *_ in uarts)
    local_init = "\n".join(f"                {f}: uart_{f}," for f, *_ in uarts)
    tasks = "\n\n".join(
        f"""    #[task(binds = {line}, priority = 2, shared = [irq_{f}])]
    fn {f}_line(cx: {f}_line::Context) {{
        cx.shared.irq_{f}.on_interrupt();
    }}

    #[task(binds = {dma}, priority = 2, shared = [irq_{f}])]
    fn {f}_rx(cx: {f}_rx::Context) {{
        cx.shared.irq_{f}.on_interrupt();
    }}

    #[task(binds = {txdma}, priority = 2, shared = [irq_{f}])]
    fn {f}_tx(cx: {f}_tx::Context) {{
        cx.shared.irq_{f}.on_interrupt();
    }}"""
        for f, label, line, dma, txdma, cfg in uarts
    )
    fields = ", ".join(f for f, *_ in uarts)
    suites = "\n".join(
        f"""        tests::full_suite(
            &mut report,
            &mut Loop {{
                name: "{label}",
                uart: &mut *{f},
            }},
            &{cfg},
            RX_RING,
            TX_BUFFER,
            &mut tests::nvic_mask(Interrupt::{dma}),
            &mut tests::nvic_mask_all([Interrupt::{line}, Interrupt::{dma}, Interrupt::{txdma}]),
        );"""
        for f, label, line, dma, txdma, cfg in uarts
    )
    together_name = " + ".join(label.split(" ")[0] for _, label, *_ in uarts)
    links = "\n".join(
        f"""                &mut Loop {{
                    name: "{label}",
                    uart: &mut *{f},
                }},"""
        for f, label, *_ in uarts
    )
    streams = f"""        report.check(
            "{together_name}",
            "8 KiB each, all at once",
            tests::streams(
                &mut [
{links}
                ],
                8 * 1024,
            ),
        );"""
    bounds = "\n".join(
        f"""        report.check(
            "{label}",
            "counted and clocked waits within their bounds, caches on",
            tests::wait_bounds(
                &mut Loop {{
                    name: "{label}",
                    uart: &mut *{f},
                }},
                board::CLOCKS.cpu.0,
                {cfg}.baudrate,
            ),
        );"""
        for f, label, line, dma, txdma, cfg in uarts
    )
    if run == "a":
        local_extra = "        scb: SCB,\n        cpuid: CPUID,\n"
        local_extra_init = "                scb: core.SCB,\n                cpuid: core.CPUID,\n"
        task_locals = f"{fields}, led, scb, cpuid"
        cache_phase = f"""
        // The same load with both caches on.
        board::caches_on(cx.local.scb, cx.local.cpuid);
        defmt::info!(
            "I-cache {{=bool}}, D-cache {{=bool}}",
            SCB::icache_enabled(),
            SCB::dcache_enabled()
        );
        // The stack is ordinary, cached AXI SRAM.
        let cached = core::hint::black_box([0u8; 32]);
        report.check(
            "D-cache on",
            "buffer in cached RAM refused",
            tests::expect_refusal(check_dma_buffer(&cached), ConfigError::BufferCacheable),
        );
        // SAFETY: never read; only its address is checked.
        let dtcm = unsafe {{ core::slice::from_raw_parts(0x2000_0000 as *const u8, 32) }};
        report.check(
            "D-cache on",
            "buffer in DTCM refused",
            tests::expect_refusal(check_dma_buffer(dtcm), ConfigError::BufferUnreachable),
        );
{streams.replace('"8 KiB each, all at once"', '"8 KiB each, all at once, caches on"')}
{bounds}
"""
        extra_uses = "        chip::check_dma_buffer,\n"
        extra_uses2 = "        uart::{Config, ConfigError, Parity, StopBits, Uart, UartIrq},\n"
        core_uses = "    use cortex_m::peripheral::{CPUID, SCB};\n"
        consts = """
    /// The Lucid's frames: CRSF to the receiver, SBUS from the DJI
    /// connector (without the inversion FerroHAL cannot set yet), MSP to
    /// the DisplayPort.
    const CRSF: Config = Config::new(420_000);
    const SBUS: Config = Config {
        baudrate: 100_000,
        parity: Parity::Even,
        stop_bits: StopBits::Two,
    };
    const MSP: Config = Config::new(115_200);
"""
    else:
        local_extra = ""
        local_extra_init = ""
        task_locals = f"{fields}, led"
        cache_phase = ""
        extra_uses = ""
        extra_uses2 = "        uart::{Config, Uart, UartIrq},\n"
        core_uses = ""
        consts = "\n    const PLAIN: Config = Config::new(115_200);\n"
    rebind = "\n".join(f"        let {f} = cx.local.{f};" for f, *_ in uarts)
    src = f"""// Generated by tools/gen_h753_runs.py: edit RUNS there, not this file.

{doc}
//!
//! RTIC owns every interrupt; FerroHAL only enables each UART's and its
//! receive stream's own interrupt bits. Each vector in use, and what serves
//! it:
//!
//! | Vector | Owner | Priority |
//! | --- | --- | --- |
{table}
//! | `SPI1` | RTIC, dispatching the priority 1 `checks` task | 1 |

#![no_std]
#![no_main]

#[path = "../board.rs"]
mod board;
#[path = "../../../common/tests.rs"]
mod tests;

use defmt_rtt as _;
use panic_probe as _;

#[rtic::app(device = ferrohal_stm32::device, peripherals = true, dispatchers = [SPI1])]
mod app {{
{core_uses}    use ferrohal_stm32::{{
{extra_uses}        device::Interrupt,
{extra_uses2}    }};

    use crate::{{
        board::{{self, RX_RING, TX_BUFFER}},
        tests::{{self, Loop, Report}},
    }};
{consts}
    #[shared]
    struct Shared {{
        // Each handle's two tasks run at one priority, so neither preempts
        // the other and the handle needs no lock.
{shared}
    }}

    #[local]
    struct Local {{
{local_uarts}
        led: board::Led,
{local_extra}    }}

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {{
        let mut core = cx.core;
        // The cycle counter, for the wait-bound check.
        core.DCB.enable_trace();
        core.DWT.enable_cycle_counter();
        // The buffers' block is non-cacheable before anything could cache it.
        board::dma_region(&mut core);

        let mut run = match board::run_{run}(cx.device) {{
            Ok(run) => run,
            Err(error) => defmt::panic!("clocks: {{}}", error),
        }};
        // `freeze` must land on the tree worked out while compiling.
        defmt::assert_eq!(run.rcc.clocks(), board::CLOCKS);
        defmt::info!("run {run.upper()}, clocks: {{}}", board::CLOCKS);

        let mut buffers = board::dma_buffers().into_iter();
{opens}

        checks::spawn().ok();
        (
            Shared {{ {shared_init} }},
            Local {{
{local_init}
                led: run.led,
{local_extra_init}            }},
        )
    }}

{tasks}

    #[task(priority = 1, local = [{task_locals}])]
    async fn checks(cx: checks::Context) {{
        tests::set_cpu_hz(board::CLOCKS.cpu.0);
        let mut report = Report::new();
{rebind}

{suites}

{streams}
{cache_phase}
        let led = cx.local.led;
        tests::finish(&report, || led.set_high());
    }}
}}
"""
    (out / f"run_{run}.rs").write_text(src)
print("written")
