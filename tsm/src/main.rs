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
        SBI_COVH_GET_TSM_INFO, SBI_COVH_RECLAIM_PAGES, SBI_COVH_RUN_TVM_VCPU,
    },
};
use linked_list_allocator::LockedHeap;
use spin::Mutex;

use crate::{constants::TVM_MAX_VCPUS, hyper::HypervisorState};

mod boot;
mod constants;
mod gpt;
mod h_extension;
mod hyper;
mod log;
#[cfg(feature = "standalone")]
mod standalone;
mod trap;

extern crate alloc;
#[global_allocator]
/// Global allocator.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

unsafe extern "C" {
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

#[no_mangle]
#[unsafe(naked)]
#[link_section = "._start"]
extern "C" fn _start() -> ! {
    /*
     * TSM entry point. The TSM acts as "trap handler for CoVE" so we must preserve a0-a7 registers
     * as they contains ECALL parameters.
     *
     */
    core::arch::naked_asm!(
        r#"
        .attribute arch, "rv64imac"

        // Initialization uses the linker-reserved boot stack.
        li t0, {boot_extension}
        bne a7, t0, 1f
        li t0, {boot_function}
        bne a6, t0, 1f
        mv sp, a0
        j 2f

        // Later entries use the runtime state created during initialization.
        1:
        csrr t0, sscratch
        ld sp, 0(t0)

        2:
        call {main}
        "#,

        boot_extension = const common::tsm_abi::SBI_SHADOWFAX_BOOT_EXT_ID,
        boot_function = const common::tsm_abi::SBI_SHADOWFAX_INIT_DONE,
        main = sym tsm_entry,
    )
}

extern "C" fn tsm_entry(
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
) -> ! {
    #[cfg(feature = "standalone")]
    {
        let _ = (a0, a1, a2, a3, a4, a5, a6, a7);
        standalone::test_tvm_bootstrap()
    }

    #[cfg(not(feature = "standalone"))]
    {
        if a6 == common::tsm_abi::SBI_SHADOWFAX_INIT_DONE
            && a7 == common::tsm_abi::SBI_SHADOWFAX_BOOT_EXT_ID
        {
            boot::initialize_and_return(a0)
        }
        main(a0, a1, a2, a3, a4, a5, a6, a7)
    }
}

const TSM_IMPL_ID: u32 = 0x45;
const TSM_VERSION: u32 = 0x45;

#[repr(C)]
#[derive(Clone, Debug)]
struct TsmInfo {
    tsm_status: TsmStatus,
    tsm_impl_id: u32,
    tsm_version: u32,
    _padding: u32,
    tsm_capabilities: usize,
    tvm_state_pages: usize,
    tvm_max_vcpus: usize,
    tvm_vcpu_state_pages: usize,
}

#[derive(Clone, Debug)]
enum TsmStatus {
    TsmNotLoaded = 0,
    TsmLoaded = 1,
    TsmReady = 2,
}

pub struct TsmState {
    info: TsmInfo,
    pub hypervisor: HypervisorState,
    pub attestation_context: TsmAttestationContext,
}

impl TsmState {
    fn new(attestation_context: TsmAttestationContext) -> Self {
        Self {
            info: TsmInfo {
                tsm_status: TsmStatus::TsmReady,
                tsm_impl_id: TSM_IMPL_ID,
                tsm_version: TSM_VERSION,
                _padding: 0,
                tsm_capabilities: 0,
                tvm_state_pages: 1,
                tvm_max_vcpus: TVM_MAX_VCPUS,
                tvm_vcpu_state_pages: 1,
            },
            hypervisor: HypervisorState::new(),
            attestation_context,
        }
    }
}

static STATE: Mutex<Option<TsmState>> = Mutex::new(None);

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
            if a1 < core::mem::size_of::<TsmInfo>() || a0 % core::mem::align_of::<TsmInfo>() != 0 {
                return SbiRet { a0: -1, a1: 0 };
            }
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

        SBI_COVH_RECLAIM_PAGES => match state.hypervisor.reclaim_pages(a0, a1) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_CREATE_TVM => {
            if a1 != 2 * core::mem::size_of::<usize>() || a0 % core::mem::align_of::<usize>() != 0 {
                return SbiRet { a0: -1, a1: 0 };
            }
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

        SBI_COVH_FINALIZE_TVM => {
            match state
                .hypervisor
                .finalize_tvm(a0, a1, a2, a3, &state.attestation_context)
            {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

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

        SBI_COVH_DESTROY_TVM => match state.hypervisor.destroy_tvm(a0) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },
        SBI_COVH_RUN_TVM_VCPU => {
            let prepared = state.hypervisor.prepare_tvm_vcpu(a0, a1);
            drop(lock);

            match prepared {
                Ok((vcpu_addr, entry_sepc, entry_arg, resume)) => unsafe {
                    HypervisorState::enter_prepared_tvm_vcpu(
                        vcpu_addr, entry_sepc, entry_arg, resume,
                    )
                },
                Err(_) => return SbiRet { a0: -1, a1: 0 },
            }
        }
        _ => SbiRet { a0: -1, a1: 0 },
    }
}
