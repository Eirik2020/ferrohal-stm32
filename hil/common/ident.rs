//! The bench identity block (`FERRO-ID v=1`, `hil/RIG.md`, Identity): one
//! text line of key=value pairs, NUL-padded to [`LEN`] bytes, which each
//! bench's `memory.x` places at 0x0800_0400, just after the vector table.
//! `hil/rig_status.py` reads it over the debug probe while the image runs,
//! with the die's IDCODE and UID beside it, so no firmware takes part and
//! no serial port is used.

/// The block's size, fixed: the host reads this many bytes.
pub const LEN: usize = 256;

/// `line`, NUL-padded to [`LEN`]; a line that does not fit fails the build.
pub const fn block(line: &str) -> [u8; LEN] {
    let bytes = line.as_bytes();
    assert!(bytes.len() < LEN, "the identity line does not fit its block");
    let mut out = [0u8; LEN];
    let mut i = 0;
    while i < bytes.len() {
        out[i] = bytes[i];
        i += 1;
    }
    out
}
