use core::cell::OnceCell;

use alloc::vec::Vec;
use fdt_rs::{base::DevTree, prelude::FallibleIterator};
use spin::mutex::Mutex;

use crate::{
    cove::TEE_SCRATCH_SIZE,
    domain::{Domain, TsmType},
};

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] = include_bytes!("../bin/tsm.bin");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../crypto/tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../crypto/publickey-pkcs1.der");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub struct State {
    pub domains: Vec<Domain>,
}

impl State {
    fn new() -> Self {
        Self {
            domains: Vec::new(),
        }
    }
}

pub fn init(fdt_addr: usize) -> Result<(), anyhow::Error> {
    let fdt = unsafe {
        let address = fdt_addr as *const u8;
        DevTree::from_raw_pointer(address).unwrap()
    };
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    let tee_stack = &raw const crate::_tee_scratch_start as *const u8 as usize;

    let mut node_iter = fdt.compatible_nodes("shadowfax,domain,instance");
    while let Some(node) = node_iter.next().unwrap() {
        let (domain, start_addr, end_addr) = Domain::from_fdt_node(&node);

        // load the correct TSM for now only the default one is supported
        match domain.tsm_type {
            // Get trusted supervisor domains
            TsmType::None => {}
            // Load the default TSM. This involves:
            // - verify the hash
            // - load the TSM into memory
            TsmType::Default => {
                Domain::verify_and_load_tsm(
                    DEFAULT_TSM,
                    start_addr,
                    DEFAULT_TSM_SIGN,
                    DEFAULT_TSM_PUBKEY,
                )?;
                let ctx_addr = tee_stack
                    - (TEE_SCRATCH_SIZE + size_of::<Context>())
                    - (domain.id + 1) * size_of::<Context>();
                let hssa = ctx_addr as *mut Context;

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
                    core::ptr::write_bytes(hssa, 0, 1);
                    (*hssa).stvec = start_addr;
                    (*hssa).mepc = start_addr;
                    (*hssa).regs[2] = 0x00;
                    (*hssa).pmpcfg = pmpcfg;
                    (*hssa).pmpaddr[0] = pmpaddr;
                }
            }
            TsmType::External => {}
        }
        state.domains.push(domain.clone());
    }

    Ok(())
}

#[derive(Clone, Debug)]
#[repr(C, align(4))]
pub struct Context {
    pub regs: [usize; 32],

    sstatus: usize,
    stvec: usize,
    sip: usize,
    scounteren: usize,
    sscratch: usize,
    satp: usize,
    senvcfg: usize,
    scontext: usize,
    pub mepc: usize,

    pub pmpcfg: usize,
    pub pmpaddr: [usize; 8],
    interrupted: usize,
}
