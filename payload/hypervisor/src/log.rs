//! Print macros for logging and panic handling on RISC-V
use core::fmt::{self, Write};
use core::panic::PanicInfo;

use crate::sbi::sbi_call;

// SBI Extension IDs
const EDBCN: i32 = 0x4442434E; // Debug Console Extension
const CONSOLE_WRITE_FID: i32 = 0x0;

/// Writer for print macros that uses SBI console
struct Writer;

impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let ptr = s.as_ptr() as usize;
        sbi_call(
            EDBCN,
            CONSOLE_WRITE_FID,
            &[
                s.len() as u64,
                (ptr & 0xffff_ffff) as u64, // Lower 32 bits of address
                ((ptr >> 32) & 0xffff_ffff) as u64, // Upper 32 bits of address
                0,
                0,
            ],
        );
        Ok(())
    }
}

/// Print function called from print macros
pub fn print_for_macro(args: fmt::Arguments) {
    let mut writer = Writer;
    let _ = writer.write_fmt(args);
}

/// Print to standard output
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::log::print_for_macro(format_args!($($arg)*))
    };
}

/// Print with newline to standard output
#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\n")
    };
    ($fmt:expr) => {
        $crate::print!(concat!($fmt, "\n"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::print!(concat!($fmt, "\n"), $($arg)*)
    };
}

/// Print error message with red color (if supported by terminal)
#[macro_export]
macro_rules! eprintln {
    () => {
        $crate::println!()
    };
    ($fmt:expr) => {
        $crate::println!(concat!("\x1b[31m", $fmt, "\x1b[0m"))
    };
    ($fmt:expr, $($arg:tt)*) => {
        $crate::println!(concat!("\x1b[31m", $fmt, "\x1b[0m"), $($arg)*)
    };
}

/// Panic handler that prints panic information and halts
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    eprintln!("PANIC: {}", info);

    loop {
        unsafe {
            core::arch::asm!("wfi");
        }
    }
}
