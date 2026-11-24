//! Candidate B's harness: the bench keeps its own `main` and prints lines the
//! host (`tools/proto_harness_b.py`) reads: `stage <name> start <limit_s>`,
//! `stage <name> done`, and `<ID> pass|fail`. The host holds each stage to
//! the limit announced and names the stage left open when it stops a run.
//! Which stages run is the host's word in a file exposed over semihosting
//! (`select`): empty or `all` runs every stage; else the stages whose name, or
//! whose requirement ID (the name up to a `.`), is one of the words.

use core::sync::atomic::{AtomicU32, Ordering};

static FAILED: AtomicU32 = AtomicU32::new(0);

pub struct Selection {
    buf: [u8; 128],
    len: usize,
}

impl Selection {
    /// Reads the host's words. Call it once, before any stage.
    pub fn read() -> Self {
        use semihosting::{fs::File, io::Read};
        let mut s = Selection { buf: [0; 128], len: 0 };
        if let Ok(mut file) = File::open(c"select")
            && let Ok(n) = file.read(&mut s.buf)
        {
            s.len = s.buf[..n].iter().take_while(|b| b.is_ascii_graphic() || **b == b' ').count();
        }
        defmt::info!("select '{=str}'", s.words());
        s
    }

    pub fn words(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    pub fn wants(&self, stage: &str) -> bool {
        let id = stage.split('.').next().unwrap_or(stage);
        let words = self.words().trim();
        words.is_empty() || words == "all" || words.split(' ').any(|w| w == stage || w == id)
    }
}

/// Runs `body` as stage `name`, announced with the limit the host holds it to.
pub fn stage<R>(name: &str, limit_s: u32, body: impl FnOnce() -> R) -> R {
    defmt::info!("stage {=str} start {=u32}", name, limit_s);
    let out = body();
    defmt::info!("stage {=str} done", name);
    out
}

/// A check: a stage of its own, whose verdict line names its requirement.
pub fn check<E: defmt::Format>(
    select: &Selection,
    name: &str,
    limit_s: u32,
    body: impl FnOnce() -> Result<(), E>,
) {
    if !select.wants(name) {
        return;
    }
    let id = name.split('.').next().unwrap_or(name);
    match stage(name, limit_s, body) {
        Ok(()) => defmt::info!("{=str} pass ({=str})", id, name),
        Err(e) => {
            FAILED.fetch_add(1, Ordering::Relaxed);
            defmt::error!("{=str} fail ({=str}): {}", id, name, e);
        }
    }
}

/// A planted hang, for the prototype's measurements.
#[allow(dead_code)]
pub fn hang() -> ! {
    loop {
        cortex_m::asm::nop();
    }
}

/// Ends the run: semihosting exit 0 when every check passed, 1 otherwise.
pub fn finish(cpu_hz: u32) -> ! {
    defmt::info!("finished, {=u32} failed", FAILED.load(Ordering::Relaxed));
    cortex_m::asm::delay(cpu_hz / 5); // 200 ms for the host to drain RTT
    match FAILED.load(Ordering::Relaxed) {
        0 => semihosting::process::exit(0),
        _ => semihosting::process::exit(1),
    }
}
