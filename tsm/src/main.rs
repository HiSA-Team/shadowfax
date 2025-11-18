#![no_std]
#![no_main]
#![feature(never_type)]
#![feature(fn_align)]

use core::{
    cell::OnceCell,
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};

use common::sbi::{
    SbiRet, SBI_COVH_ADD_TVM_MEASURED_PAGES, SBI_COVH_ADD_TVM_MEMORY_REGION,
    SBI_COVH_CONVERT_PAGES, SBI_COVH_CREATE_TVM, SBI_COVH_CREATE_TVM_VCPU, SBI_COVH_DESTROY_TVM,
    SBI_COVH_EXT_ID, SBI_COVH_FINALIZE_TVM, SBI_COVH_GET_TSM_INFO, SBI_COVH_RUN_TVM_VCPU,
};
use linked_list_allocator::LockedHeap;
use spin::Mutex;

use crate::{
    hyper::HypervisorState,
    state::{TsmInfo, TSM_IMPL_ID, TSM_VERSION},
};

mod h_extension;
mod hyper;
mod log;
mod state;

#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    static _stack_top: u8;
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
        stack_top = sym _stack_top,
        main = sym main,
    )
}

struct TsmState {
    info: TsmInfo,
    hypervisor: HypervisorState,
}

impl TsmState {
    fn new() -> Self {
        Self {
            info: TsmInfo {
                tsm_status: state::TsmStatus::TsmReady,
                tsm_impl_id: TSM_IMPL_ID,
                tsm_version: TSM_VERSION,
                _padding: 0,
                tsm_capabilities: 0,
                tvm_state_pages: 0,
                tvm_max_vcpus: 1,
                tvm_vcpu_state_pages: 0,
            },
            hypervisor: HypervisorState::new(),
        }
    }
}

static INIT: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<Option<TsmState>> = Mutex::new(None);

fn ensure_init() -> spin::MutexGuard<'static, Option<TsmState>> {
    let mut guard = STATE.lock();

    if !INIT.load(Ordering::Acquire) {
        let state = TsmState::new(); // heavy init allowed here
        *guard = Some(state);
        INIT.store(true, Ordering::Release);
    }

    guard
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
    assert_eq!(a7, SBI_COVH_EXT_ID);

    let mut lock = ensure_init();
    let state = lock.as_mut().unwrap();

    // fid is formated as:
    // bits[31:26]: SDID target
    // bits[15:0]: function ID
    let fid = a6 & 0xFFFF;

    let ret = match fid {
        SBI_COVH_GET_TSM_INFO => {
            assert!(a1 >= core::mem::size_of::<TsmInfo>());
            unsafe {
                core::ptr::write(a0 as *mut TsmInfo, state.info.clone());
            }
            SbiRet {
                a0: 0,
                a1: core::mem::size_of::<TsmInfo>() as isize,
            }
        }

        SBI_COVH_CONVERT_PAGES => match state.hypervisor.add_confidential_pages(a0, a1) {
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

            match state.hypervisor.create_tvm(tvm_params.0, tvm_params.1) {
                Ok(id) => SbiRet {
                    a0: 0,
                    a1: id as isize,
                },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_FINALIZE_TVM => match state.hypervisor.finalize_tvm(a0, a1, a2, a3) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_ADD_TVM_MEMORY_REGION => {
            match state.hypervisor.add_tvm_memory_region(a0, a1, a2) {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_ADD_TVM_MEASURED_PAGES => {
            match state
                .hypervisor
                .add_tvm_measured_pages(a0, a1, a2, a3, a4, a5)
            {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_CREATE_TVM_VCPU => SbiRet { a0: 0, a1: 0 },
        SBI_COVH_RUN_TVM_VCPU => match state.hypervisor.tvm_run_vcpu(a0, a1) {
            Ok(_) => unreachable!(),
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_DESTROY_TVM => match state.hypervisor.destroy_tvm() {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },
        _ => SbiRet { a0: -1, a1: 0 },
    };

    drop(lock);

    // Issue the TEERET
    unsafe {
        core::arch::asm!(
            "
            ecall
            ",
            in("a0") ret.a0,
            in("a1") ret.a1,
            in("a6") a6,
            in("a7") SBI_COVH_EXT_ID,
            options(noreturn)
        );
    };
}
