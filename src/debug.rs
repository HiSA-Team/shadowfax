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
