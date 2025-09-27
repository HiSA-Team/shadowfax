#![no_std]
#![no_main]
#![feature(fn_align)]

use core::{panic::PanicInfo, ptr::NonNull};

use common::tsm::{State, TsmInfo, Tvm};

use linked_list_allocator::LockedHeap;

#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[repr(C)]
struct SbiRet {
    a0: isize,
    a1: isize,
}

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _top_b_stack: u8;
}

/*
 * This is needed for rust bare metal programs
 */
#[inline(never)]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// Give each hart 8K stack
const STACK_SIZE_PER_HART: usize = 1024 * 8;

const SBI_COVH_GET_TSM_INFO: usize = 0;
const SBI_COVH_CONVERT_PAGES: usize = 1;
const SBI_COVH_CREATE_TVM: usize = 5;
const SBI_COVH_DESTROY_TVM: usize = 8;
const EID_COVH: usize = 0x434F5648;

#[no_mangle]
#[unsafe(naked)]
#[link_section = "._start"]
extern "C" fn _start() -> ! {
    /*
     * TSM entry point. The TSM acts as "trap handler for CoVE" so we must preserve a0-a7 registers
     * as they contains ECALL parameters.
     *
     * We define a custom "ABI". TSM Expects the State Address (initialized by the TSM Driver) in
     * t0: it must not be clobbered.
     */
    core::arch::naked_asm!(
        r#"
        .attribute arch, "rv64imac"

        // setup up the stack
        li t1, {stack_size_per_hart}
        la sp, {stack_top}
        sub sp, sp, t1

        call {main}
        "#,

        stack_size_per_hart = const STACK_SIZE_PER_HART,
        stack_top = sym _top_b_stack,
        main = sym main,
    )
}

// Since this is a TSM with non reentrant model, an ECALL should be a TEERET
fn main(
    a0: usize,
    a1: usize,
    _a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    a6: usize,
    a7: usize,
) -> ! {
    let mut state_addr: usize;
    unsafe {
        core::arch::asm!("add {}, t0, zero", out(reg) state_addr, options(readonly, nostack))
    };

    // The TSM should be called only for CoVH
    assert_eq!(a7, EID_COVH);

    let fid = a6 & 0xFF;
    let mut state = unsafe { FirmwareState::from_addr(state_addr).expect("Invalid state address") };

    let ret = match fid {
        SBI_COVH_GET_TSM_INFO => {
            let info = state.info_clone();

            assert_eq!(a1, core::mem::size_of::<TsmInfo>());
            unsafe {
                core::ptr::write(a0 as *mut TsmInfo, info);
            }
            SbiRet {
                a0: 0,
                a1: core::mem::size_of::<TsmInfo>() as isize,
            }
        }
        SBI_COVH_CONVERT_PAGES => {
            let state = unsafe { state.as_mut() };

            match state.add_confidential_pages(a0, a1) {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }
        SBI_COVH_CREATE_TVM => {
            let state = unsafe { state.as_mut() };
            match state.create_tvm(a0, a1) {
                Ok(id) => SbiRet {
                    a0: 0,
                    a1: id as isize,
                },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }
        SBI_COVH_DESTROY_TVM => {
            let state = unsafe { state.as_mut() };
            match state.destroy_tvm() {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }
        _ => SbiRet { a0: -1, a1: 0 },
    };

    // Issue the TEERET
    unsafe {
        core::arch::asm!(
            "
            ecall
            ",
            in("a0") ret.a0,
            in("a1") ret.a1,
            in("a6") a6,
            in("a7") EID_COVH,
            options(noreturn)
        );
    };
}
/// Small typed wrapper around the firmware-provided pointer.
///
/// All `unsafe` pointer casting happens in `from_a5()`; the rest of the code
/// uses safe methods where possible.
pub struct FirmwareState {
    ptr: NonNull<State>,
}

impl FirmwareState {
    /// Try to build `FirmwareState` from the register `a5`.
    ///
    /// This performs basic checks (non-null and alignment). It's `unsafe` because
    /// we cannot guarantee the pointed memory has the correct provenance or lifetime.
    pub unsafe fn from_addr(addr: usize) -> Option<Self> {
        // read a5 into a usize

        if addr == 0 {
            return None;
        }

        if addr % align_of::<State>() != 0 {
            return None;
        }

        // Convert to NonNull; this is safe because addr != 0.
        Some(Self {
            ptr: NonNull::new_unchecked(addr as *mut State),
        })
    }

    /// Obtain an immutable reference. This is safe only if no other party mutates
    /// the structure concurrently. If the region can be mutated concurrently,
    /// prefer `read_volatile` or explicit atomics.
    pub fn as_ref(&self) -> &State {
        unsafe { &*self.ptr.as_ptr() }
    }

    /// Obtain a mutable reference. This is `unsafe` because aliasing and concurrency
    /// rules can be violated if another reference exists.
    pub unsafe fn as_mut(&mut self) -> &mut State {
        &mut *self.ptr.as_ptr()
    }

    /// Convenience: clone `info` field in a safe manner.
    pub fn info_clone(&self) -> TsmInfo
    where
        TsmInfo: Clone,
    {
        // if info is not concurrently mutated, this is fine. Otherwise user must ensure sync.
        self.as_ref().info.clone()
    }
}
