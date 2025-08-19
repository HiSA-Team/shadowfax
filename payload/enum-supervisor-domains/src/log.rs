//! Print macros for logging

use core::fmt::{self, Write};

#[repr(C)]
struct SbiRet {
    error: isize,
    value: isize,
}

fn sbi_call(extid: usize, fid: usize, args: &[usize; 5]) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") extid,
            in("a6") fid,
            inlateout("a0") args[0] => error,
            inlateout("a1") args[1] => value,
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
        );
    }
    SbiRet { error, value }
}

const EDBCN: usize = 0x4442434E;
const CONSOLE_WRITE_FID: usize = 0x0;

/// Writer for print macro.
struct Writer;
impl core::fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        sbi_call(
            EDBCN,
            CONSOLE_WRITE_FID,
            &[
                s.len(),
                s.as_ptr() as usize & 0xffff_ffff,
                (s.as_ptr() as usize >> 32) & 0xffff_ffff,
                0,
                0,
            ],
        );
        Ok(())
    }
}

/// Print function calling from print macro
pub fn print_for_macro(args: fmt::Arguments) {
    let mut writer = Writer;
    writer.write_fmt(args).unwrap();
}

/// Print debug message to standard output.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug_log")]
        $crate::log::print_for_macro(format_args!($($arg)*))
    };
}

/// Print debug message with linebreak to standard output.
#[macro_export]
macro_rules! debugln {
    ($fmt:expr) => ($crate::debug!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::debug!(concat!($fmt, "\n"), $($arg)*));
}
/// Print to standard output.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::log::print_for_macro(format_args!($($arg)*)));
}

/// Print with linebreak to standard output.
#[macro_export]
macro_rules! println {
    ($fmt:expr) => ($crate::print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print!(concat!($fmt, "\n"), $($arg)*));
}
