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
    perf::{read_cycle, read_instret, read_time},
    state::{TsmInfo, TSM_IMPL_ID, TSM_VERSION},
};

mod h_extension;
mod hyper;
mod log;
mod perf;
mod sbi;
mod state;

#[link_section = ".rodata"]
pub static GUEST_ELF: &[u8] =
    include_bytes!("../../guests/riscv-tests/benchmarks/guests/median.riscv");

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
        main = sym test_tvm_bootstrap,
    )
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

pub static STATE: Mutex<Option<TsmState>> = Mutex::new(None);

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

/// Test function to bypass SBI and jump straight into a TVM
fn test_tvm_bootstrap() -> ! {
    println!("[OLORIN] Starting Mapping TVM from ELF");
    // 1. Initialize the TSM state manually (if _secure_init wasn't called by a driver)
    // We'll simulate a dummy attestation context for testing.
    let dummy_context = TsmAttestationContext::default();
    _secure_init(&dummy_context as *const _ as usize);

    let mut lock = STATE.lock();
    let state = lock.as_mut().expect("State not initialized");

    // 2. Define Memory Layout for Testing (Adjust based on your QEMU RAM)
    // Assuming TSM is at 0x80200000, let's put TVM structures higher up.
    let tvm_page_table_addr = 0x80800000; // Must be 16KB aligned
    let tvm_state_addr = 0x80810000;
    let tvm_confidential_pool = 0x80900000; // Where guest RAM actually sits
    let pool_size_pages = 512; // 2MB test pool

    // 3. Convert pages to confidential
    state
        .hypervisor
        .add_confidential_pages(tvm_page_table_addr, 4)
        .unwrap(); // 16KB
    state
        .hypervisor
        .add_confidential_pages(tvm_state_addr, 1)
        .unwrap();
    state
        .hypervisor
        .add_confidential_pages(tvm_confidential_pool, pool_size_pages)
        .unwrap();

    // 4. Use the ELF loading procedure
    // This helper parses GUEST_ELF and maps it into the TVM
    let tvm_id = hyper::bootstrap_load_elf_lazy(
        state,
        GUEST_ELF,
        tvm_page_table_addr,
        tvm_state_addr,
        tvm_confidential_pool,
    )
    .expect("Failed to load ELF");

    // 5. Create VCPU (ID 0)
    state
        .hypervisor
        .create_tvm_vcpu(tvm_id, 0, 0)
        .expect("Failed to create VCPU");

    println!("[OLORIN] Bootstrap complete. Entering Guest...");

    // 6. Run it!
    state
        .hypervisor
        .run_tvm_vcpu(tvm_id, 0)
        .expect("Failed to run VCPU");
}

fn test_tvm_bootstrap_perf() -> ! {
    println!("[OLORIN] Starting Mapping TVM from ELF");
    // 1. Initialize the TSM state manually (if _secure_init wasn't called by a driver)
    // We'll simulate a dummy attestation context for testing.
    // --- Start TVM measurement ---
    let cycle_start = read_cycle();
    let instret_start = read_instret();
    let time_start = read_time();

    let dummy_context = TsmAttestationContext::default();
    _secure_init(&dummy_context as *const _ as usize);

    let mut lock = STATE.lock();
    let state = lock.as_mut().expect("State not initialized");

    // 2. Define Memory Layout for Testing (Adjust based on your QEMU RAM)
    // Assuming TSM is at 0x80200000, let's put TVM structures higher up.
    let tvm_page_table_addr = 0x80800000; // Must be 16KB aligned
    let tvm_state_addr = 0x80810000;
    let tvm_confidential_pool = 0x80900000; // Where guest RAM actually sits
    let pool_size_pages = 512; // 2MB test pool

    // 3. Convert pages to confidential
    state
        .hypervisor
        .add_confidential_pages(tvm_page_table_addr, 4)
        .unwrap(); // 16KB
    state
        .hypervisor
        .add_confidential_pages(tvm_state_addr, 1)
        .unwrap();
    state
        .hypervisor
        .add_confidential_pages(tvm_confidential_pool, pool_size_pages)
        .unwrap();

    // 4. Use the ELF loading procedure
    // This helper parses GUEST_ELF and maps it into the TVM
    let tvm_id = hyper::bootstrap_load_elf_lazy(
        state,
        GUEST_ELF,
        tvm_page_table_addr,
        tvm_state_addr,
        tvm_confidential_pool,
    )
    .expect("Failed to load ELF");

    // 5. Create VCPU (ID 0)
    state
        .hypervisor
        .create_tvm_vcpu(tvm_id, 0, 0)
        .expect("Failed to create VCPU");

    println!("[OLORIN] Bootstrap complete. Entering Guest...");
    // --- End TVM measurement ---
    let cycle_end = read_cycle();
    let instret_end = read_instret();
    let time_end = read_time();

    let delta = time_end - time_start;
    println!(
        "cycle = {}\ninstret = {}\ndelta = {}",
        cycle_end - cycle_start,
        instret_end - instret_start,
        delta
    );
    println!("[OLORIN] TVM bootstrap completed");
    loop {}
}
