use core::{cell::OnceCell, error::Error, fmt::Display};

use alloc::{string::String, vec::Vec};
use fdt_rs::{
    base::{DevTree, DevTreeNode},
    prelude::{FallibleIterator, PropReader},
};
use rsa::{
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    sha2::Sha256,
    signature::Verifier,
};
use spin::mutex::Mutex;

use crate::trap::TEE_SCRATCH_SIZE;

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] = include_bytes!("../../bin/tsm.bin");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../crypto/tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../../crypto/publickey-pkcs1.der");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

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

#[allow(unused)]
#[derive(Clone)]
pub enum TsmType {
    Default,
    External,
    None,
}

impl From<&str> for TsmType {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_ref() {
            "default" => TsmType::Default,
            "none" => TsmType::None,
            "external" => TsmType::External,
            _ => panic!("unknown tsm type"),
        }
    }
}

#[derive(Clone)]
pub struct Domain {
    pub id: usize,
    name: String,
    pub active: usize,
    pub tsm_type: TsmType,
    pub trust_map: usize,
}

impl Domain {
    fn empty() -> Self {
        Self {
            id: 0,
            name: String::default(),
            active: 0,
            tsm_type: TsmType::None,
            trust_map: 0,
        }
    }

    fn from_fdt_node(node: &DevTreeNode) -> (Self, usize, usize) {
        let mut domain = Domain::empty();
        let mut start_addr = 0;
        let mut end_addr = 0;
        for prop in node.props().iterator() {
            if let Ok(prop) = prop {
                let name = prop.name().unwrap_or("");
                match name {
                    "id" => domain.id = prop.u32(0).unwrap() as usize,
                    "name" => domain.name = String::from(prop.str().unwrap()),
                    "tsm-type" => domain.tsm_type = TsmType::from(prop.str().unwrap()),
                    "memory" => {
                        start_addr = prop.u64(0).unwrap() as usize;
                        end_addr = prop.u64(1).unwrap() as usize;
                    }
                    "trust" => {
                        let node = node
                            .props()
                            .iterator()
                            .find(|c| c.as_ref().unwrap().name().unwrap_or("") == "trust")
                            .unwrap()
                            .unwrap();

                        let mut i = 0;
                        let mut trust = 0;
                        loop {
                            if let Ok(d) = node.u32(i) {
                                trust |= 1 << (d as usize);
                                i += 1
                            } else {
                                break;
                            }
                        }
                        domain.trust_map = trust;
                    }
                    _ => {}
                }
            }
        }
        domain.active = if domain.id == 0 { 1 } else { 0 };
        (domain, start_addr, end_addr)
    }
    fn verify_and_load_tsm(
        bin: &[u8],
        start_addr: usize,
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<(), anyhow::Error> {
        // Verify the tsm signature with the provided payload using the the public key
        let signature = Signature::try_from(signature).map_err(TsmError::SignatureError)?;
        let verifying_key = VerifyingKey::<Sha256>::from_pkcs1_der(&public_key)
            .map_err(TsmError::RsaPublicKeyError)?;
        verifying_key
            .verify(bin, &signature)
            .map_err(TsmError::SignatureError)?;

        // load the tsm into the destination address
        unsafe {
            core::ptr::copy_nonoverlapping(bin.as_ptr(), start_addr as *mut u8, bin.len());
        }

        Ok(())
    }

    pub fn is_trusted(&self, dst: usize) -> bool {
        self.trust_map & (1 << dst) != 0
    }
}

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

#[derive(Debug)]
enum TsmError {
    RsaPublicKeyError(rsa::pkcs1::Error),
    SignatureError(rsa::signature::Error),
}

impl Display for TsmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RsaPublicKeyError(err) => write!(f, "verification error: {}", err),
            Self::SignatureError(err) => write!(f, "signature error: {}", err),
        }
    }
}

impl Error for TsmError {}
