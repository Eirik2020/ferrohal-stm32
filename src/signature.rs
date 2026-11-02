//! The chip's factory-programmed 96-bit unique ID.

/// The unique ID's twelve bytes, in memory order.
pub fn device_uid() -> [u8; 12] {
    let mut bytes = [0; 12];
    for (offset, byte) in bytes.iter_mut().enumerate() {
        // SAFETY: inside the chip's read-only unique ID block, one byte at a
        // time (RM0090 and RM0368 39.1, RM0433 61.1 allow any access width).
        *byte = unsafe { ((crate::chip::UID_BASE + offset) as *const u8).read_volatile() };
    }
    bytes
}
