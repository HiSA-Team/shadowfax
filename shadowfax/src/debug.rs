// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
#![allow(unused)]
use crate::opensbi;
use core::convert::TryInto;
use core::fmt::{Error, Write};

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $error:expr) => {
        if !$cond {
            Err($error)
        } else {
            Ok(())
        }
    };
}

#[macro_export]
macro_rules! ensure_not {
    ($cond:expr, $error:expr) => {
        if $cond {
            Err($error)
        } else {
            Ok(())
        }
    };
}

fn read_memory(address: usize) -> u64 {
    let ptr = (address) as *mut u64;
    unsafe { ptr.read_volatile() }
}

macro_rules! _debug {
	($($args:tt)+) => ({
		use core::fmt::Write;
		if let Err(_) = write!(crate::debug::Console::new(), $($args)+) {
            // we can safely ignore
        }
	});
}

macro_rules! debug {
	() => ({
        _debug!("\r\n")
    });
	($fmt:expr) => ({
		_debug!(concat!("[SHADOWFAX]: ", $fmt, "\r\n"))
    });
	($fmt:expr, $($args:tt)+) => ({
		_debug!(concat!("[SHADOWFAX]: ", $fmt, "\r\n"), $($args)+)
    });
}

pub(crate) use {_debug, debug};

pub struct Console {}

impl Console {
    pub fn put(c: u8) {
        unsafe {
            opensbi::sbi_putc(c);
        }
    }
}

impl Write for Console {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for i in s.bytes() {
            Self::put(i);
        }

        Ok(())
    }
}

impl Console {
    pub fn new() -> Self {
        Console {}
    }
}

pub mod raw {
    use core::fmt::{self, Write};
    use core::ptr::{read_volatile, write_volatile};

    /// Default QEMU virt ns16550 UART base. Change if your platform uses a different UART.
    const UART0_BASE: usize = 0x1000_0000;

    /// ns16550 register offsets (accessed as bytes)
    const REG_THR: usize = 0x00; // transmit holding register (write)
    const REG_LSR: usize = 0x05; // line status register (read)
    const LSR_THRE: u8 = 0x20; // Transmitter Holding Register Empty

    /// Low-level UART writer that uses MMIO (volatile accesses).
    pub struct RawConsole {
        base: usize,
    }

    impl RawConsole {
        /// Create with default UART base (adjust if needed).
        pub const fn new() -> Self {
            RawConsole { base: UART0_BASE }
        }

        /// write a single byte to UART (busy-wait until THR empty)
        pub fn putc(&self, c: u8) {
            unsafe {
                let lsr = (self.base + REG_LSR) as *const u8;
                let thr = (self.base + REG_THR) as *mut u8;

                // wait for THR empty
                while (read_volatile(lsr) & LSR_THRE) == 0 {}

                write_volatile(thr, c);
            }
        }
    }

    /// Implement `core::fmt::Write` so `write!()` / `format_args!()` work with RawConsole.
    impl Write for RawConsole {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for &b in s.as_bytes() {
                self.putc(b);
            }
            Ok(())
        }
    }

    /// Public helper that accepts `format_args!()` (no heap) and prints to UART.
    pub fn print_raw(args: core::fmt::Arguments) {
        let mut con = RawConsole::new();
        // ignore errors — nothing to do on failure here
        let _ = con.write_fmt(args);
    }

    /// Convenience macro to mirror `println!` / `print!` style:
    #[macro_export]
    macro_rules! print_raw {
    ($($arg:tt)*) => ({
        $crate::debug::raw::print_raw(core::format_args!($($arg)*));
    });
}

    /// hex helper (if you prefer manual printing of addresses)
    pub fn write_usize_hex<W: Write>(w: &mut W, mut v: usize, digits: usize) -> fmt::Result {
        // print fixed-width hex (digits = number of hex digits, e.g., 16 for 64-bit 0-padding)
        const HEX: &[u8; 16] = b"0123456789abcdef";
        // buffer digits in reverse order then output
        let mut buf = [b'0'; 32];
        for i in 0..digits {
            let nibble = (v & 0xF) as usize;
            buf[digits - 1 - i] = HEX[nibble];
            v >>= 4;
        }
        w.write_str("0x")?;
        w.write_str(core::str::from_utf8(&buf[..digits]).unwrap())?;
        Ok(())
    }
}
