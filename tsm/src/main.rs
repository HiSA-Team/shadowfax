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
    tsm_abi::{TsmBootInfo, TSM_BOOT_ABI_VERSION, TSM_BOOT_MAGIC},
};
use linked_list_allocator::LockedHeap;
use spin::Mutex;

use crate::{constants::TVM_MAX_VCPUS, hyper::HypervisorState};

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

// Give each hart 32K stack
const STACK_SIZE_PER_HART: usize = 1024 * 32;
// The project TSM PIE is linked at offset zero in a 32 MiB supervisor domain.
const TSM_REGION_SIZE: usize = 32 * 1024 * 1024;

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

        // _start is at PIE offset zero, so this AUIPC recovers the runtime load base
        // without relying on an absolute linker symbol or a dynamic relocation.
        auipc sp, 0
        li t1, {tsm_region_size}
        add sp, sp, t1
        li t1, {stack_size_per_hart}
        sub sp, sp, t1

        call {main}
        "#,

        stack_size_per_hart = const STACK_SIZE_PER_HART,
        tsm_region_size = const TSM_REGION_SIZE,
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
    main(a0, a1, a2, a3, a4, a5, a6, a7)
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

enum TsmPageType {
    Page4k = 0,
    Page2mb = 1,
    Page1gb = 2,
    Page512gb = 3,
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

#[no_mangle]
#[allow(dead_code)]
#[inline(never)]
#[link_section = "._secure_init"]
/// This function will be called by the TSM-driver to initialize securely the TSM after the
/// signature has bee authenticated.
extern "C" fn _secure_init(addr: usize) -> isize {
    // Initialize heap
    unsafe {
        let heap_start = (&raw const _heap_start as *const u8) as usize;
        let heap_size = ((&raw const _heap_end as *const u8) as usize) - heap_start;

        ALLOCATOR.lock().init(heap_start as *mut u8, heap_size);
    }
    // 2. Prepare the Initial Context
    // If addr is 0 (Testing), create a fresh default context ON THE HEAP.
    // If addr != 0 (Production), we assume it points to valid ROM/Flash/Pre-loaded RAM
    // that is NOT overlapping with our new Heap.
    let initial_context = if addr == 0 {
        TsmAttestationContext::default()
    } else {
        let boot = unsafe { &*(addr as *const TsmBootInfo) };
        if boot.magic != TSM_BOOT_MAGIC
            || boot.abi_version != TSM_BOOT_ABI_VERSION
            || boot.struct_size as usize != size_of::<TsmBootInfo>()
            || boot.dice_context_size > usize::MAX as u64
        {
            return -1;
        }
        let bytes = unsafe {
            core::slice::from_raw_parts(
                boot.dice_context_addr as *const u8,
                boot.dice_context_size as usize,
            )
        };
        match TsmAttestationContext::from_slice(bytes) {
            Ok(context) => context,
            Err(_) => return -1,
        }
    };

    // 3. Update Global State
    // We clone into State and Attestation Context.
    // Since heap is Init, these clones allocate safely.
    let mut state = STATE.lock();
    state.replace(TsmState::new(initial_context));

    drop(state);
    0
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
    let owner = (a6 >> 26) & 0x3F;

    if fid == SBI_COVH_RUN_TVM_VCPU {
        let prepared = state.hypervisor.prepare_tvm_vcpu(owner, a0, a1);
        drop(lock);

        match prepared {
            Ok((vcpu_addr, entry_sepc, entry_arg, resume)) => unsafe {
                HypervisorState::enter_prepared_tvm_vcpu(vcpu_addr, entry_sepc, entry_arg, resume)
            },
            Err(_) => return SbiRet { a0: -1, a1: 0 },
        }
    }

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

        SBI_COVH_CONVERT_PAGES => match state.hypervisor.add_confidential_pages(owner, a0, a1) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_RECLAIM_PAGES => match state.hypervisor.reclaim_pages(owner, a0, a1) {
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
            println!("Creating TVM for domain {}", owner);

            match state.hypervisor.create_tvm(
                owner,
                attestation_context,
                tvm_params.0,
                tvm_params.1,
            ) {
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
                .finalize_tvm(owner, a0, a1, a2, a3, &state.attestation_context)
            {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_ADD_TVM_MEMORY_REGION => {
            match state.hypervisor.add_tvm_memory_region(owner, a0, a1, a2) {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_ADD_TVM_MEASURED_PAGES => {
            match state
                .hypervisor
                .add_tvm_measured_pages(owner, a0, a1, a2, a3, a4, a5)
            {
                Ok(_) => SbiRet { a0: 0, a1: 0 },
                Err(_) => SbiRet { a0: -1, a1: 0 },
            }
        }

        SBI_COVH_ADD_ZERO_PAGES => match state
            .hypervisor
            .add_tvm_zero_pages(owner, a0, a1, a2, a3, a4)
        {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_CREATE_TVM_VCPU => match state.hypervisor.create_tvm_vcpu(owner, a0, a1, a2) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },

        SBI_COVH_DESTROY_TVM => match state.hypervisor.destroy_tvm(owner, a0) {
            Ok(_) => SbiRet { a0: 0, a1: 0 },
            Err(_) => SbiRet { a0: -1, a1: 0 },
        },
        _ => SbiRet { a0: -1, a1: 0 },
    }
}
