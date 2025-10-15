#![no_std]
#![no_main]
#![feature(never_type)]
#![feature(fn_align)]

use core::{cell::OnceCell, panic::PanicInfo, ptr::NonNull};

use common::{
    sbi::{
        SbiRet, EID_COVH, SBI_COVH_ADD_TVM_MEASURED_PAGES, SBI_COVH_ADD_TVM_MEMORY_REGION,
        SBI_COVH_CONVERT_PAGES, SBI_COVH_CREATE_TVM, SBI_COVH_CREATE_TVM_VCPU,
        SBI_COVH_DESTROY_TVM, SBI_COVH_FINALIZE_TVM, SBI_COVH_GET_TSM_INFO, SBI_COVH_RUN_TVM_VCPU,
    },
    tsm::{TsmInfo, TsmState, TsmStateData},
};
use linked_list_allocator::LockedHeap;

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::hyper::HypervisorState;

mod h_extension;
mod hyper;
mod log;

#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _top_b_stack: u8;
}

/*
 * This is needed for rust bare metal programs
 */
#[inline(never)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

// Give each hart 8K stack
const STACK_SIZE_PER_HART: usize = 1024 * 8;

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

static mut HYPER_STATE: MaybeUninit<HypervisorState> = MaybeUninit::uninit();
static INIT_DONE: AtomicBool = AtomicBool::new(false);

fn get_or_init_state(state_addr: usize) -> &'static mut HypervisorState {
    if !INIT_DONE.load(Ordering::Acquire) {
        unsafe {
            let mut state = unsafe { State::from_addr(state_addr).expect("Invalid state address") };
            let tsm_state_data = state.as_mut();
            (*tsm_state_data).info.tsm_state = TsmState::TsmReady;

            HYPER_STATE.write(HypervisorState::new());
        }
        INIT_DONE.store(true, Ordering::Release);
    }
    unsafe { &mut *HYPER_STATE.as_mut_ptr() }
}

// Since this is a TSM with non reentrant model, an ECALL should be a TEERET
fn main(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> ! {
    // The TSM should be called only for CoVH.
    // TODO: the TSM will be invoked also for the CoVG SBI
    assert_eq!(a7, EID_COVH);
    let mut state_addr: usize;

    unsafe {
        core::arch::asm!("add {}, t0, zero", out(reg) state_addr, options(readonly, nostack))
    };
    let hypervisor_data = get_or_init_state(state_addr);

    // fid is formated as:
    // bits[31:26]: SDID target
    // bits[15:0]: function ID
    let fid = a6 & 0xFFFF;

    let ret = match fid {
        SBI_COVH_GET_TSM_INFO => {
            let state = unsafe { State::from_addr(state_addr).expect("Invalid state address") };
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

        SBI_COVH_CONVERT_PAGES => match hypervisor_data.add_confidential_pages(a0, a1) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_CREATE_TVM => {
            assert!(a1 == 16);
            let tvm_params = unsafe {
                let page_table_address = core::ptr::read(a0 as *const usize);
                let state_address = core::ptr::read((a0 + 8) as *const usize);
                (page_table_address, state_address)
            };

            match hypervisor_data.create_tvm(tvm_params.0, tvm_params.1) {
                Ok(id) => SbiRet {
                    a0: 0,
                    a1: id as isize,
                },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_FINALIZE_TVM => match hypervisor_data.finalize_tvm(a0, a1, a2, a3) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_ADD_TVM_MEMORY_REGION => match hypervisor_data.add_tvm_memory_region(a0, a1, a2) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_ADD_TVM_MEASURED_PAGES => {
            match hypervisor_data.add_tvm_measured_pages(a0, a1, a2, a3, a4, a5) {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_CREATE_TVM_VCPU => SbiRet { a0: 0, a1: 0 },
        SBI_COVH_RUN_TVM_VCPU => match hypervisor_data.tvm_run_vcpu(a0, a1) {
            Ok(_) => unreachable!(),
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_DESTROY_TVM => match hypervisor_data.destroy_tvm() {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },
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
/// All `unsafe` pointer casting happens in `from_addr()`; the rest of the code
/// uses safe methods where possible.
pub struct State {
    ptr: NonNull<TsmStateData>,
}

impl State {
    /// Try to build `FirmwareState` from the register `t0`.
    ///
    /// This performs basic checks (non-null and alignment). It's `unsafe` because
    /// we cannot guarantee the pointed memory has the correct provenance or lifetime.
    pub unsafe fn from_addr(addr: usize) -> Option<Self> {
        if addr == 0 {
            return None;
        }

        if addr % align_of::<TsmStateData>() != 0 {
            return None;
        }

        // Convert to NonNull; this is safe because addr != 0.
        Some(Self {
            ptr: NonNull::new_unchecked(addr as *mut TsmStateData),
        })
    }

    /// Obtain an immutable reference. This is safe only if no other party mutates
    /// the structure concurrently. If the region can be mutated concurrently,
    /// prefer `read_volatile` or explicit atomics.
    pub fn as_ref(&self) -> &TsmStateData {
        unsafe { &*self.ptr.as_ptr() }
    }

    /// Obtain a mutable reference. This is `unsafe` because aliasing and concurrency
    /// rules can be violated if another reference exists.
    pub unsafe fn as_mut(&mut self) -> &mut TsmStateData {
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
