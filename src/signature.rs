//! The chip's factory-programmed 96-bit unique ID, and its die revision.
//!
//! Requirements: UID-001, REVISION-001

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

/// The die's revision code, `DBGMCU_IDC.REV_ID` (bits 31:16).
///
/// ES0392 Table 2 maps the RM0433 chips: 0x1003 is revision Y or W, 0x2001 is
/// X and 0x2003 is V. Other families have their own tables.
pub fn revision() -> u16 {
    crate::chip::revision()
}
