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

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] = include_bytes!("../../tsm.bin");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../../publickey-pkcs1.der");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub fn init(fdt_addr: usize, next_addr: usize, next_mode: usize) -> Result<(), anyhow::Error> {
    let fdt = unsafe {
        let address = fdt_addr as *const u8;
        DevTree::from_raw_pointer(address).unwrap()
    };
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new());

    // push domain zero since it is mandated by the ISA
    state.domains.push(Domain {
        name: String::from("root"),
        id: 0,
        next_mode,
        next_arg: fdt_addr,
        next_addr,
        active: false,
        start_address: 0,
        end_address: 0,
        tsm_type: TsmType::None,
        tsm: None,
    });

    let mut node_iter = fdt.compatible_nodes("shadowfax,domain,instance");
    while let Some(node) = node_iter.next().unwrap() {
        let mut domain = Domain::from_fdt_node(&node);

        // load the correct TSM for now only the default one is supported
        match domain.tsm_type {
            // Nothing to do, TSM is not required for this domain
            TsmType::None => {}
            // Load the default TSM. This involves:
            // - verify the hash
            // - loading the TSM in memory
            // - boot into the TSM
            TsmType::Default => {
                let mut tsm = Tsm::verify_and_load(
                    DEFAULT_TSM,
                    domain.next_addr,
                    DEFAULT_TSM_SIGN,
                    DEFAULT_TSM_PUBKEY,
                )?;

                let sp = unsafe { &crate::_tee_stack_start as *const u8 as usize };
                tsm.init(sp)?;
                domain.tsm = Some(tsm);
            }
            TsmType::External => {}
        }
        state.domains.push(domain.clone());
    }

    Ok(())
}

#[allow(unused)]
#[derive(Clone)]
enum TsmType {
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
    id: usize,
    name: String,
    next_addr: usize,
    next_arg: usize,
    next_mode: usize,
    pub active: bool,
    start_address: usize,
    end_address: usize,
    tsm_type: TsmType,
    pub tsm: Option<Tsm>,
}

impl Domain {
    fn empty() -> Self {
        Self {
            id: 0,
            name: String::default(),
            next_mode: 0,
            next_arg: 0,
            next_addr: 0,
            active: false,
            start_address: 0,
            end_address: 0,
            tsm_type: TsmType::None,
            tsm: None,
        }
    }

    fn from_fdt_node(node: &DevTreeNode) -> Self {
        let mut domain = Domain::empty();
        for prop in node.props().iterator() {
            if let Ok(prop) = prop {
                let name = prop.name().unwrap_or("");
                match name {
                    "id" => domain.id = prop.u32(0).unwrap() as usize,
                    "name" => domain.name = String::from(prop.str().unwrap()),
                    "next-arg1" => domain.next_arg = prop.u64(0).unwrap() as usize,
                    "next-addr" => domain.next_addr = prop.u64(0).unwrap() as usize,
                    "next-mode" => domain.next_mode = prop.u32(0).unwrap() as usize,
                    "tsm-type" => domain.tsm_type = TsmType::from(prop.str().unwrap()),
                    "memory" => {
                        let start_addr = prop.u64(0).unwrap() as usize;
                        let end_addr = prop.u64(1).unwrap() as usize;
                        domain.start_address = start_addr;
                        domain.end_address = end_addr;
                    }
                    _ => {}
                }
            }
        }
        domain
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
    pub fn get_domains(&self) -> &Vec<Domain> {
        &self.domains
    }
}

#[derive(Clone, Debug)]
pub struct Tsm {
    start_addr: usize,
    size: usize,

    pub stack_pointer: usize,
}

impl Tsm {
    fn verify_and_load(
        bin: &[u8],
        start_addr: usize,
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<Self, anyhow::Error> {
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

        Ok(Tsm {
            start_addr,
            size: bin.len(),
            stack_pointer: 0,
        })
    }

    fn init(&mut self, sp: usize) -> Result<(), anyhow::Error> {
        let hssa = (sp
            - core::mem::size_of::<TsmSupervisorStateArea>()
            - core::mem::size_of::<HartSupervisorStateArea>())
            as *mut TsmSupervisorStateArea;

        // zero out the tsm supervisor state area
        unsafe {
            core::ptr::write_bytes(hssa, 0, 1);
        }

        // setup basic registers for first context switch
        unsafe {
            (*hssa).stvec = self.start_addr;
            (*hssa).mepc = self.start_addr;
        }

        self.stack_pointer = sp;
        Ok(())
    }
}

#[derive(Clone, Debug)]
#[repr(C, align(4))]
pub struct HartSupervisorStateArea {
    regs: [u64; 32],

    sstatus: usize,
    stvec: usize,
    sip: usize,
    scounteren: usize,
    sscratch: usize,
    satp: usize,
    senvcfg: usize,
    scontext: usize,
    mepc: usize,
}

#[derive(Clone, Debug)]
#[repr(C, align(4))]
pub struct TsmSupervisorStateArea {
    regs: [u64; 32],

    sstatus: usize,
    stvec: usize,
    sip: usize,
    scounteren: usize,
    sscratch: usize,
    satp: usize,
    senvcfg: usize,
    scontext: usize,
    mepc: usize,
    interrupted: bool,
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
