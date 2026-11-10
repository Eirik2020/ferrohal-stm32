//! The panic handler the compile-fail probes share. Each probe links this
//! so that a probe which wrongly compiles fails for no other reason.

#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
