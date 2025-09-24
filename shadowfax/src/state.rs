/*
* Global State of the TSM Driver. For now this state is an array of supervisor domains. The state
* is initialized by the init function which populates the array from the device tree. The device
* tree must declare supervisor domains with `compatible = "shadowfax,domain,config";`. Each
* supervisor domain must declare an id and a `compatible = "shadowfax,domain,instance";`.
* Other fields may be:
* - trust: a list of id of other supervisor domains that are trusted;
* - tsm-type:
*  - "none": don't load anything not a confidential supervisor domains;
*  - "default": confidential supervisor domain. Load the default TSM;
*  - "external": confidential supervisor domain. Don't load the TSN;
* - memory: an entry for the memory range used to program the PMP of the domain
*
* Note: since we use the OpenSBI implementation, the domain with id=0x0 is initialized by OpenSBI
* sbi_scratch_init() function.
*
* Examples:
* `
       shadowfax-domains {
           compatible = "shadowfax,domain,config";

           untrusted-domain {
               compatible = "shadowfax,domain,instance";
               id = <0x0>
               trust = <0x1>;
               tsm-type = "none";
           };

           trusted-domain {
               compatible = "shadowfax,domain,instance";
               id = <0x1>;
               memory = <0x0 0x81000000 0x0 0x82000000>;
               tsm-type = "default";
           };
       };
* `
* Author: Giuseppe Capasso <capassog97@gmail.com>
*/

use core::cell::OnceCell;

use alloc::vec::Vec;
use fdt_rs::{base::DevTree, prelude::FallibleIterator};
use spin::mutex::Mutex;

use crate::{context::Context, cove::TEE_SCRATCH_SIZE, tsm::Tsm};

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] =
    include_bytes!("../../target/riscv64imac-unknown-none-elf/debug/shadowfax_tsm");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../bin/tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../keys/publickey.pem");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub tsms: Vec<Tsm>,
    pub current_id: usize,
}

impl State {
    fn new() -> Self {
        Self {
            tsms: Vec::new(),
            current_id: 0,
        }
    }
}

pub fn init(
    fdt_addr: usize,
    tsm_state_addr: usize,
    tsm_state_size: usize,
) -> Result<(), anyhow::Error> {
    let fdt = unsafe {
        let address = fdt_addr as *const u8;
        DevTree::from_raw_pointer(address).unwrap()
    };
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    let tee_stack = &raw const crate::_tee_scratch_start as *const u8 as usize;

    let mut node_iter = fdt.compatible_nodes("shadowfax,domain,instance");
    while let Some(node) = node_iter.next().unwrap() {
        let mut tsm = Tsm::from_fdt_node(&node);

        Tsm::verify_and_load(
            DEFAULT_TSM,
            tsm.start_region_addr,
            DEFAULT_TSM_SIGN,
            DEFAULT_TSM_PUBKEY,
        )?;
        let context_addr = tee_stack
            - (TEE_SCRATCH_SIZE + size_of::<Context>())
            - (tsm.id + 1) * size_of::<Context>();
        let tsm_ctx = context_addr as *mut Context;

        // Save the context address and the state address
        tsm.context_addr = context_addr;
        tsm.state_addr = tsm_state_addr;

        // init the TSM state
        assert!(
            core::mem::size_of::<TsmInternalState>() < tsm_state_size,
            "Unsufficient memory for TSM State"
        );
        // for now assume we have 1 TSM
        unsafe {
            core::ptr::write(
                tsm_state_addr as *mut TsmInternalState,
                TsmInternalState::new(),
            );
        }

        // Setup PMP entries for the memory region
        let start_addr = tsm.start_region_addr;
        let end_addr = tsm.end_region_addr;

        let size = (end_addr - start_addr).next_power_of_two();
        let base = start_addr & !(size - 1);

        let k = size.trailing_zeros() as usize;
        let ones = (1 << (k - 3)) - 1;

        // Source: https://www.five-embeddev.com/riscv-priv-isa-manual/latest-adoc/machine.html#pmp
        let pmpaddr = ((base >> 2) as usize) | ones;
        let locked = false;
        let range = riscv::register::Range::NAPOT;
        let permission = riscv::register::Permission::RWX;
        let index = 0;
        let byte = (locked as usize) << 7 | (range as usize) << 3 | (permission as usize);
        let pmpcfg = byte << (8 * index);

        // zero out the tsm supervisor state area
        // setup basic registers for first context switch
        unsafe {
            // zero out memory
            core::ptr::write_bytes(tsm_ctx, 0, 1);

            // init values
            (*tsm_ctx).stvec = start_addr;
            (*tsm_ctx).mepc = start_addr;
            (*tsm_ctx).pmpcfg = pmpcfg;
            (*tsm_ctx).pmpaddr[0] = pmpaddr;
        }

        // Setup PMP entries for the state
        let start_addr = tsm.state_addr;
        let end_addr = tsm_state_addr + core::mem::size_of::<TsmInternalState>();
        let size = (end_addr - start_addr).next_power_of_two();
        let base = start_addr & !(size - 1);

        let k = size.trailing_zeros() as usize;
        let ones = (1 << (k - 3)) - 1;

        let pmpaddr = ((base >> 2) as usize) | ones;
        let locked = false;
        let range = riscv::register::Range::NAPOT;
        let permission = riscv::register::Permission::RW;
        let index = 1;
        let byte = (locked as usize) << 7 | (range as usize) << 3 | (permission as usize);
        let pmpcfg = byte << (8 * index);

        unsafe {
            (*tsm_ctx).pmpcfg |= pmpcfg;
            (*tsm_ctx).pmpaddr[1] = pmpaddr;
        }

        state.tsms.push(tsm);
    }

    Ok(())
}

#[repr(C, align(0x1000))]
struct TsmInternalState {
    info: TsmInfo,
    guest: Option<Guest>,
}

impl TsmInternalState {
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

struct Guest {
    vcpu_state: [u64; 32],
}

#[repr(C)]
#[derive(Clone, Debug)]
struct TsmInfo {
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
enum TsmPageType {
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
enum TvmState {
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
enum TsmState {
    /* TSM has not been loaded on this platform. */
    TsmNotLoaded = 0,
    /* TSM has been loaded, but has not yet been initialized. */
    TsmLoaded = 1,
    /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
    TsmReady = 2,
}
