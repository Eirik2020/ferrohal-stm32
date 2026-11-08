//! The Pico 1 I2C peripheral simulator for the FerroHAL benches: two DW
//! `apb_i2c` slaves, one per bench (the wiring list is `hil/WIRING.md`),
//! each speaking the register map in this crate's `lib.rs` at address
//! `0x2A`. I2C0 answers the Nucleo-F401RE's I2C3; I2C1 answers the
//! Nucleo-H753ZI's I2C4.
//!
//! The map's two scenario registers reach behind the embassy driver:
//! STRETCH delays before the response goes out — an empty TX FIFO is
//! what holds SCL, so the wait is the stretch — and NACK pokes the DW
//! block's `IC_SLV_DATA_NACK_ONLY` (offset 0x84) while the bus idles,
//! which the block allows only while it is disabled and its slave
//! inactive. Both are one-shot, spent by the master's next transfer, so
//! the firmware stays idempotent across runs.
//!
//! No transaction is ever left pending a vanished master: a read the
//! master abandons ends when its next START aborts the block, which is
//! how the hardware is designed to resync.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::i2c::InterruptHandler;
use embassy_rp::i2c_slave::{Command, Config, I2cSlave};
use embassy_rp::peripherals::{I2C0, I2C1};
use embassy_time::{with_timeout, Duration, Instant, Timer};

use pico_i2c_sim as sim;
use panic_halt as _;

bind_interrupts!(struct Irqs {
    I2C0_IRQ => InterruptHandler<I2C0>;
    I2C1_IRQ => InterruptHandler<I2C1>;
});

/// How long `listen` may wait before it is re-armed: a second of silence
/// on the bus means no master is talking, nothing to clean up. It is
/// long past any transaction's wire time, so cancelling `listen` here
/// never loses an in-flight phase.
const QUIET: Duration = Duration::from_secs(1);

/// How long a NACK arm stays set before the warden spends it. A refused
/// data byte is never delivered as a command — the DW NACKs it and drops
/// it — and while `IC_SLV_DATA_NACK_ONLY` stays set every later data
/// byte is dropped the same way, so nothing on the bus can tell the
/// simulator the refusal happened. Time is what is left: the suite sends
/// its refused write within about a millisecond of the arm and then
/// pauses three, so a disarm this much later lands after the refusal and
/// before the next read.
const SETTLE: Duration = Duration::from_millis(3);

/// When each bus's NACK arm came up, in milliseconds since boot plus one
/// (zero: not armed). The warden reads it; the serve task writes it.
static ARM_AT: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
/// A bus whose arm the warden spent: its serve task applies the refusal
/// to the map at the next command it sees.
static DISARMED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
/// When each serve task last looked at its bus, on the ARM_AT clock. The
/// warden feeds the watchdog only while both are fresh, so a wedged bus
/// task reboots the chip instead of hiding behind the other one.
static ALIVE: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];

/// Milliseconds since boot, plus one, as the ARM_AT words carry them.
fn now_word() -> u32 {
    Instant::now().as_millis() as u32 + 1
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    start_watchdog();
    let mut config = Config::default();
    config.addr = u16::from(sim::ADDR);
    // Both ends of each bench bus pull up internally (~40 k): the lines
    // idle high with no external resistor fitted.
    config.scl_pullup = true;
    config.sda_pullup = true;
    // I2C0: SDA on GP0, SCL on GP1 — the F401 bench, bus tag 1.
    let bus0 = I2cSlave::new(p.I2C0, p.PIN_1, p.PIN_0, Irqs, config);
    // I2C1: SDA on GP2, SCL on GP3 — the H753 bench, bus tag 2.
    let bus1 = I2cSlave::new(p.I2C1, p.PIN_3, p.PIN_2, Irqs, config);
    spawner.spawn(bus0_task(bus0).expect("the I2C0 task"));
    spawner.spawn(bus1_task(bus1).expect("the I2C1 task"));
    spawner.spawn(warden_task().expect("the warden task"));
}

/// Starts the hardware watchdog: eight seconds without a feed reboots the
/// chip, so a wedged bus task recovers by itself and the self-reboot
/// flash path keeps working. Only the serve loops feed it; the longest
/// thing they wait on is a stretch (tens of milliseconds).
fn start_watchdog() {
    let wdt = embassy_rp::pac::WATCHDOG;
    wdt.tick().write(|w| {
        w.set_cycles(11); // the 12 MHz reference, divided to 1 µs ticks
        w.set_enable(true);
    });
    wdt.ctrl().write(|w| {
        w.set_time(8_000_000); // 8 s of 1 µs ticks
        w.set_enable(true);
    });
    wdt.load()
        .write_value(embassy_rp::pac::watchdog::regs::Load(0));
}

/// Restarts the watchdog's count from zero.
fn feed_watchdog() {
    embassy_rp::pac::WATCHDOG
        .load()
        .write_value(embassy_rp::pac::watchdog::regs::Load(0));
}

#[embassy_executor::task]
async fn bus0_task(slave: I2cSlave<'static, I2C0>) {
    serve(slave, 1, poke_i2c0).await
}

#[embassy_executor::task]
async fn bus1_task(slave: I2cSlave<'static, I2C1>) {
    serve(slave, 2, poke_i2c1).await
}

/// Spends NACK arms that have sat for their settle. The refusal itself
/// is invisible to the bus — the DW NACKs the data byte and drops it, so
/// no command ever arrives — and the hardware arm keeps refusing every
/// later data byte, so only time can decide that the refusal has passed:
/// the warden disarms the hardware on the idle bus and lets the serve
/// task apply the map's accounting at its next command. It also feeds
/// the watchdog, and only while both serve tasks are fresh: a bus task
/// that wedges reboots the chip instead of hiding behind the other.
#[embassy_executor::task]
async fn warden_task() {
    loop {
        Timer::after(Duration::from_millis(1)).await;
        let now = now_word();
        for slot in 0..2 {
            let at = ARM_AT[slot].load(Ordering::Relaxed);
            if at != 0 && now.wrapping_sub(at) >= SETTLE.as_millis() as u32 {
                match slot {
                    0 => poke_i2c0(false),
                    _ => poke_i2c1(false),
                }
                ARM_AT[slot].store(0, Ordering::Relaxed);
                DISARMED[slot].store(true, Ordering::Relaxed);
            }
        }
        let fresh = |slot: usize| {
            let at = ALIVE[slot].load(Ordering::Relaxed);
            at != 0 && now.wrapping_sub(at) < 1_000
        };
        if fresh(0) && fresh(1) {
            feed_watchdog();
        }
    }
}

/// Serves one bus: every transaction the master makes moves the map. The
/// one-shot NACK is reconciled with the hardware where the DW block takes
/// that write without a race: inside the master's own transaction (the
/// write-read's read phase, where no new START can arrive), or after a
/// refusal, in the idle gap the suite's throwaway read covers. The
/// embassy `listen` never surfaces the refusal itself — the DW delivers
/// the refused byte and stops like any write — so the map's own arm is
/// what identifies the refused transaction.
async fn serve<T: embassy_rp::i2c::Instance>(
    mut slave: I2cSlave<'static, T>,
    tag: u8,
    poke: fn(bool),
) {
    let mut state = sim::State::new(tag);
    let mut in_buf = [0u8; 64];
    let mut out = [0u8; 64];
    let mut armed = false;
    let slot = usize::from(tag - 1);
    loop {
        ALIVE[slot].store(now_word(), Ordering::Relaxed);
        let command = match with_timeout(QUIET, slave.listen(&mut in_buf)).await {
            Ok(Ok(command)) => command,
            Ok(Err(_)) => {
                // A partial write: the buffer filled before the master
                // stopped. Drop it; the master's next START resyncs.
                continue;
            }
            Err(_) => {
                // A second of silence: the warden has spent any arm worth
                // spending, and nothing else needs cleaning up.
                continue;
            }
        };
        // The warden spent this bus's arm while the task was listening:
        // the refused write happened, unseen. Account for it before the
        // command that follows it.
        if DISARMED[slot].load(Ordering::Relaxed) {
            DISARMED[slot].store(false, Ordering::Relaxed);
            state.on_abort();
            armed = false;
            ARM_AT[slot].store(0, Ordering::Relaxed);
        }
        // Whether this transaction's write phase met the arm: the map's
        // one-shot, set by an earlier transaction, spent by this one.
        let was_armed = state.nack_armed();
        match command {
            // General call is off in the configuration; nothing to do.
            Command::GeneralCall(_) => {}
            Command::Write(len) if was_armed => {
                // The refused write: the hardware NACKed its first data
                // byte, the DW still delivered it, and the wire saw the
                // address. A refused CLEAR still clears, so a run that
                // died mid-refusal leaves the next one clean.
                state.on_abort();
                armed = false;
                poke(false);
                if len > 0 && in_buf[0] == sim::REG_CLEAR {
                    state.on_write(&in_buf[..len]);
                }
            }
            Command::Write(len) => {
                state.on_write(&in_buf[..len]);
                state.on_transaction(sim::SHAPE_WRITE);
            }
            Command::Read => {
                state.on_transaction(sim::SHAPE_READ);
                respond(&mut slave, &mut state, &mut out).await;
            }
            Command::WriteRead(len) if was_armed => {
                // The write phase's first data byte was refused, so the
                // read phase never comes; the master has walked away.
                state.on_abort();
                armed = false;
                poke(false);
                if len > 0 && in_buf[0] == sim::REG_CLEAR {
                    state.on_write(&in_buf[..len]);
                }
            }
            Command::WriteRead(len) => {
                state.on_write(&in_buf[..len]);
                state.on_transaction(sim::SHAPE_WRITE_READ);
                respond(&mut slave, &mut state, &mut out).await;
            }
        }
        // The BOOT register: the ROM's own reset, which re-enters the USB
        // bootloader. The bus is idle here, so nothing is cut mid-transfer.
        if state.take_boot() {
            reboot_to_bootsel();
        }
        // The bus idles now: make the hardware match the map's one-shot.
        // The poke's disable window is a few microseconds on a stopped bus;
        // the suite sends a throwaway read after its arm write and after
        // the refusal, so a START inside the window costs that read and
        // never the op that follows.
        let want = state.nack_armed();
        if want != armed {
            poke(want);
            armed = want;
            ARM_AT[slot].store(if want { now_word() } else { 0 }, Ordering::Relaxed);
        }
    }
}

/// Reboots the chip into its USB bootloader through the ROM's own
/// `_reset_to_usb_boot`, found through the public ROM table (RP2040
/// datasheet §2.8.3): 'M', 'u', 0x01 at 0x10, the function-table pointer
/// at 0x14, and the lookup helper at 0x18. Functions are named by
/// `(c2 << 8) | c1`, so reset-to-USB-boot is `('B' << 8) | 'U'`. Never
/// returns: the RPI-RP2 drive appears and `run-i2c-sim.sh flash` copies
/// the next image without a finger on the button.
fn reboot_to_bootsel() -> ! {
    unsafe {
        let table = core::ptr::read_volatile(0x0000_0014 as *const u16) as *const u8;
        let lookup: extern "C" fn(*const u8, u32, u16) -> *const u8 = core::mem::transmute(
            core::ptr::read_volatile(0x0000_0018 as *const u16) as *const u8,
        );
        let reset = lookup(table, ('B' as u32) << 8 | 'U' as u32, 0xffff);
        let reset: extern "C" fn(u32, u32) -> ! = core::mem::transmute(reset);
        // No activity-light pin, no disabled interfaces.
        reset(0, 0)
    }
}

/// Answers a read phase: the queued stretch first — the FIFO is empty
/// while the timer runs, and an empty FIFO is what holds SCL — then the
/// register the last write pointed at. One `respond_to_read` call is
/// enough and is the only safe count: it returns either when the master
/// NACKs the last byte or when the STOP's abort clears the leftover
/// bytes, and on that abort it reads `IC_CLR_TX_ABRT`, which is what
/// releases the DW's forced flushed state — a second call would push
/// into a bus that has already stopped. A master that walks away leaves
/// the block waiting, and the next transaction's read request aborts it,
/// which is the resync the hardware provides.
async fn respond<T: embassy_rp::i2c::Instance>(
    slave: &mut I2cSlave<'static, T>,
    state: &mut sim::State,
    out: &mut [u8; 64],
) {
    if let Some(us) = state.take_stretch() {
        Timer::after(Duration::from_micros(u64::from(us))).await;
    }
    state.fill(out);
    let _ = slave.respond_to_read(out).await;
}

/// Arms or clears the DW block's data-phase refusal for I2C0. The write
/// is allowed only while the block is disabled and its slave inactive:
/// this runs between transactions, on a stopped bus. The disable window
/// is a few microseconds; a master that STARTs inside it gets an address
/// NACK instead, which the benches' cross-checks tell apart.
fn poke_i2c0(on: bool) {
    arm(&i2c0(), on);
}

/// The same for I2C1.
fn poke_i2c1(on: bool) {
    arm(&i2c1(), on);
}

fn i2c0() -> embassy_rp::pac::i2c::I2c {
    // The RP2040's I2C blocks, at their fixed addresses in the APB
    // space: embassy owns the singletons, but the register block is the
    // same memory.
    unsafe { embassy_rp::pac::i2c::I2c::from_ptr(0x4004_4000 as *mut ()) }
}

fn i2c1() -> embassy_rp::pac::i2c::I2c {
    unsafe { embassy_rp::pac::i2c::I2c::from_ptr(0x4004_8000 as *mut ()) }
}

fn arm(i2c: &embassy_rp::pac::i2c::I2c, on: bool) {
    i2c.ic_enable().modify(|w| w.set_enable(false));
    for _ in 0..1000 {
        if !i2c.ic_enable_status().read().ic_en() {
            break;
        }
    }
    i2c.ic_slv_data_nack_only().modify(|w| w.set_nack(on));
    i2c.ic_enable().modify(|w| w.set_enable(true));
    for _ in 0..1000 {
        if i2c.ic_enable_status().read().ic_en() {
            break;
        }
    }
}

