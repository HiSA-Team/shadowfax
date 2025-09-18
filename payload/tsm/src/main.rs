#![no_std]
#![no_main]
#![feature(fn_align)]

use core::panic::PanicInfo;

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
const SBI_COVH_CREATE_TVM: usize = 5;

macro_rules! cove_unpack_fid {
    ($fid:expr) => {
        (($fid >> 26) & 0x3F, $fid & 0xFFFF)
    };
}

#[no_mangle]
#[unsafe(naked)]
#[link_section = "._start"]
extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        // setup up the stack
        // a0-a4 contains TEECALL parameters. We must preserve them
        r#"
        .attribute arch, "rv64imac"
        add a5, zero, zero
        li t0, {stack_size_per_hart}
        mul t1, a5, t0
        la sp, {stack_top}
        sub sp, sp, t1

        call {main}
        "#,

        stack_size_per_hart = const STACK_SIZE_PER_HART,
        stack_top = sym _top_b_stack,
        main = sym main,
    )
}

struct Guest {
    vcpu_state: [u64; 32],
}
struct State {
    info: TsmInfo,
    guest: Option<Guest>,
}

impl State {
    fn new() -> Self {
        Self {
            info: TsmInfo {
                tsm_state: TsmState::TsmReady,
                tsm_impl_id: 69,
                tsm_version: 0,
                tsm_capabilities: 0,
                tvm_state_pages: 0,
                tvm_max_vcpus: 1,
                tvm_vcpu_state_pages: 0,
            },
            guest: None,
        }
    }
}

struct SbiRet {
    a0: isize,
    a1: isize,
}

// Since this is a TSM with non reentrant model, an ECALL should be a TEERET
fn main(a0: usize, a1: usize, a2: usize, a3: usize, a4: usize) -> ! {
    let mut a6: usize;
    unsafe { core::arch::asm!("add {}, a6, zero", out(reg) a6, options(nomem)) };
    let (_sdid, fid) = cove_unpack_fid!(a6);
    let state = State::new();
    let ret = match fid {
        SBI_COVH_GET_TSM_INFO => {
            let tsm_info_ptr = a0 as *mut TsmInfo;
            let info = state.info.clone();

            assert_eq!(a1, core::mem::size_of::<TsmInfo>());
            unsafe {
                tsm_info_ptr.write(info);
            }
            SbiRet {
                a0: 0,
                a1: core::mem::size_of::<TsmInfo>() as isize,
            }
        }
        SBI_COVH_CREATE_TVM => SbiRet { a0: 0, a1: 1 },
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
            in("a7") 0x434F5648,
            options(noreturn)
        );
    };
}

#[repr(C)]
#[derive(Clone, Debug)]
pub struct TsmInfo {
    /*
     * The current state of the TSM (see `tsm_state` enum above).
     * If the state is not `TSM_READY`, the remaining fields are invalid and
     * will be initialized to `0`.
     */
    pub tsm_state: TsmState,
    /*
     * Identifier of the TSM implementation, see `Reserved TSM Implementation IDs`
     * table below. This identifier is intended to distinguish among different TSM
     * implementations, potentially managed by different organizations, that might
     * target different deployment models and, thus, implement subset of CoVE spec.
     */
    pub tsm_impl_id: u32,
    /*
     * Version number of the running TSM.
     */
    pub tsm_version: u32,
    /*
     * A bitmask of CoVE features supported by the running TSM, see `TSM Capabilities`
     * table below. Every bit in this field corresponds to a capability defined by
     * `COVE_TSM_CAP_*` constants. Presence of bit `i` indicates that both the TSM
     * and hardware support the corresponding capability.
     */
    pub tsm_capabilities: usize,
    /*
     * The number of 4KB pages which must be donated to the TSM for storing TVM
     * state in sbi_covh_create_tvm_vcpu(). `0` if the TSM does not support the
     * dynamic memory allocation capability.
     */
    pub tvm_state_pages: usize,
    /*
     * The maximum number of vCPUs a TVM can support.
     */
    pub tvm_max_vcpus: usize,
    /*
     * The number of 4KB pages which must be donated to the TSM when creating
     * a new vCPU. `0` if the TSM does not support the dynamic memory allocation
     * capability.
     */
    pub tvm_vcpu_state_pages: usize,
}
/*
 * TsmPageType is an enumeration that defines the types of memory pages supported by the TSM.
 * It includes options for 4 KiB, 2 MiB, 1 GiB, and 512 GiB pages, allowing for flexible memory
 * management and allocation.
 */
pub enum TsmPageType {
    /* 4 KiB */
    Page4k = 0,
    /* 2 MiB */
    Page2mb = 1,
    /* 1 GiB */
    Page1gb = 2,
    /* 512 GiB */
    Page512gb = 3,
}

/*
 * TvmState is an enumeration that represents the state of a Trusted Virtual Machine (TVM).
 * It indicates whether the TVM is in the process of initialization or is ready to run.
 */
#[derive(Clone)]
pub enum TvmState {
    /* The TVM has been created, but isn't yet ready to run */
    TvmInitializing = 0,
    /* The TVM is in a runnable state */
    TvmRunnable = 1,
}

/*
 * TsmState is an enumeration that describes the current state of the Trusted Software Module (TSM).
 * It provides information on whether the TSM is not loaded, loaded but not initialized, or fully
 * initialized and ready to accept ECALLs (environment calls).
 */
#[derive(Clone, Debug)]
pub enum TsmState {
    /* TSM has not been loaded on this platform. */
    TsmNotLoaded = 0,
    /* TSM has been loaded, but has not yet been initialized. */
    TsmLoaded = 1,
    /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
    TsmReady = 2,
}
