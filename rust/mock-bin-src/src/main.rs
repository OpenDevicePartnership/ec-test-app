//! Simple mock binary that when built produces an ELF with a `.defmt` section containing the log strings below
//! If this changes, mock.rs will likely need changing since it depends on an specific version of the ELF
#![no_std]
#![no_main]

// Need this to satisfy defmt but don't actually care about logging anywhere
#[defmt::global_logger]
struct MockLogger;

unsafe impl defmt::Logger for MockLogger {
    fn acquire() {}
    unsafe fn write(_bytes: &[u8]) {}
    unsafe fn release() {}
    unsafe fn flush() {}
}

defmt::timestamp!("{=u64:us}", 0);

#[cortex_m_rt::entry]
fn main() -> ! {
    defmt::trace!("This is a trace defmt log");
    defmt::debug!("This is a debug defmt log");
    defmt::info!(
        "This is a really long log message. Really really really long. Its length should be measured in light-years. Not characters. It will wrap around on all monitors not of cosmic scale. Who needs to log something this long anyway? Who knows. But someone will. Therefore we must be prepared."
    );
    defmt::info!("This is a log message with a newline.\nSee? I'm on a newline now!");
    defmt::warn!("This is a warn defmt log");
    defmt::error!("This is a error defmt log");
    loop {}
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
