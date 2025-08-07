use alloc::vec::Vec;
use fdt_rs::{base::DevTree, prelude::FallibleIterator};
use serde::{Deserialize, Serialize};
use spin::{mutex::Mutex, MutexGuard};

use crate::{
    context::Context,
    cove::{MAX_DOMAINS, TEE_SCRATCH_SIZE},
    domain::{Domain, TsmType},
};

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] = include_bytes!("../bin/tsm.bin");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../bin/crypto/tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../bin/crypto/publickey-pkcs1.der");

pub struct Locked;
pub struct Unlocked;

static STATE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

pub struct GlobalState<S = Locked> {
    guard: MutexGuard<'static, Vec<u8>>,
    state: Option<State>,

    _phantom: core::marker::PhantomData<S>,
}

impl GlobalState<Locked> {
    pub fn unlock() -> GlobalState<Unlocked> {
        let mut guard = STATE.lock();
        // array is empty. It is the first initialization
        if guard.is_empty() {
            *guard = encrypt_state(&State::new());
        }

        let decrypted = decrypt_state(&guard);

        GlobalState {
            guard,
            state: Some(decrypted),
            _phantom: core::marker::PhantomData,
        }
    }
}

impl GlobalState<Unlocked> {
    pub fn state_mut(&mut self) -> &mut State {
        self.state.as_mut().unwrap()
    }
    pub fn state(&self) -> &State {
        self.state.as_ref().unwrap()
    }
    pub fn lock(mut self) {
        let state = self.state.take().unwrap();
        let new_blob = encrypt_state(&state);
        *self.guard = new_blob;
    }
}

#[derive(Serialize, Deserialize)]
pub struct State {
    pub domains: heapless::Vec<Domain, MAX_DOMAINS>,
}

impl State {
    fn new() -> Self {
        Self {
            domains: heapless::Vec::new(),
        }
    }
}

pub fn init(fdt_addr: usize) -> Result<(), anyhow::Error> {
    let fdt = unsafe {
        let address = fdt_addr as *const u8;
        DevTree::from_raw_pointer(address).unwrap()
    };

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
        with_state(|s| unsafe {
            s.domains.push_unchecked(domain.clone());
        })
    }

    Ok(())
}

impl<Locked> Drop for GlobalState<Locked> {
    fn drop(&mut self) {
        let state = self.state.take().unwrap();

        let new_blob = encrypt_state(&state);
        *self.guard = new_blob;
    }
}

fn encrypt_state(_s: &State) -> Vec<u8> {
    Vec::new()
}
fn decrypt_state(_v: &Vec<u8>) -> State {
    State::new()
}

pub fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut sm = GlobalState::<Locked>::unlock();
    let res = f(sm.state_mut());
    // sm.lock();
    res
}
