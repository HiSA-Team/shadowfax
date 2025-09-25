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
use common::tsm;
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
            core::mem::size_of::<tsm::State>() < tsm_state_size,
            "Unsufficient memory for TSM State"
        );
        // for now assume we have 1 TSM
        unsafe {
            core::ptr::write(tsm_state_addr as *mut tsm::State, tsm::State::new());
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
        let end_addr = tsm_state_addr + core::mem::size_of::<tsm::State>();
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
