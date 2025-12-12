#![no_std]
#![no_main]
#![feature(never_type)]
#![feature(fn_align)]

use core::panic::PanicInfo;

use common::{
    attestation::{DiceLayer, TsmAttestationContext},
    sbi::{
        SbiRet, SBI_COVH_ADD_TVM_MEASURED_PAGES, SBI_COVH_ADD_TVM_MEMORY_REGION,
        SBI_COVH_ADD_ZERO_PAGES, SBI_COVH_CONVERT_PAGES, SBI_COVH_CREATE_TVM,
        SBI_COVH_CREATE_TVM_VCPU, SBI_COVH_DESTROY_TVM, SBI_COVH_EXT_ID, SBI_COVH_FINALIZE_TVM,
        SBI_COVH_GET_TSM_INFO, SBI_COVH_RUN_TVM_VCPU,
    },
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

extern crate alloc;
#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe extern "C" {
    /// boot stack top (defined in `memory.x`)
    pub static mut _stack_top: u8;

    // Heap
    static mut _heap_start: u8;
    static _heap_end: u8;
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

// Give each hart 32K stack
const STACK_SIZE_PER_HART: usize = 1024 * 32;

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
    attestation_context: TsmAttestationContext,
}

impl TsmState {
    fn new(attestation_context: TsmAttestationContext) -> Self {
        Self {
            info: TsmInfo {
                tsm_status: state::TsmStatus::TsmReady,
                tsm_impl_id: TSM_IMPL_ID,
                tsm_version: TSM_VERSION,
                _padding: 0,
                tsm_capabilities: 0,
                tvm_state_pages: 1,
                tvm_max_vcpus: 1,
                tvm_vcpu_state_pages: 1,
            },
            hypervisor: HypervisorState::new(),
            attestation_context,
        }
    }
}

static STATE: Mutex<Option<TsmState>> = Mutex::new(None);

#[no_mangle]
#[allow(dead_code)]
#[inline(never)]
#[link_section = "._secure_init"]
/// This function will be called by the TSM-driver to initialize securely the TSM after the
/// signature has bee authenticated.
fn _secure_init(addr: usize) {
    // Initialize heap
    unsafe {
        let heap_start = (&raw const _heap_start as *const u8) as usize;
        let heap_size = ((&raw const _heap_end as *const u8) as usize) - heap_start;

        ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
    }
    let mut state = STATE.lock();

    let payload_ptr = addr as *mut TsmAttestationContext;
    let payload = unsafe { (*payload_ptr).clone() };

    *state = Some(TsmState::new(payload));

    drop(state);
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
    assert_eq!(a7, SBI_COVH_EXT_ID);

    let ret = handle_covh(a0, a1, a2, a3, a4, a5, a6);

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

fn handle_covh(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> SbiRet {
    let mut lock = STATE.lock();
    let state = lock.as_mut().unwrap();

    // fid is formated as:
    // bits[31:26]: SDID target
    // bits[15:0]: function ID
    let fid = a6 & 0xFFFF;

    match fid {
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

            let attestation_context = state.attestation_context.compute_next(&[0; 32]);

            match state
                .hypervisor
                .create_tvm(attestation_context, tvm_params.0, tvm_params.1)
            {
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

        SBI_COVH_ADD_ZERO_PAGES => match state.hypervisor.add_tvm_zero_pages(a0, a1, a2, a3, a4) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_CREATE_TVM_VCPU => match state.hypervisor.create_tvm_vcpu(a0, a1, a2) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_RUN_TVM_VCPU => match state.hypervisor.run_tvm_vcpu(a0, a1) {
            Ok(_) => unreachable!(),
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_DESTROY_TVM => match state.hypervisor.destroy_tvm() {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },
        _ => SbiRet { a0: -1, a1: 0 },
    }
}
