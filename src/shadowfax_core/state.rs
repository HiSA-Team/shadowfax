use core::{cell::OnceCell, error::Error, fmt::Display};

use alloc::{string::String, vec::Vec};
use fdt_rs::{
    base::{DevTree, DevTreeNode},
    prelude::{FallibleIterator, PropReader},
};
use riscv::register::{mepc, mstatus};
use rsa::{
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    sha2::Sha256,
    signature::Verifier,
};
use spin::mutex::Mutex;

use crate::opensbi::{pmp_disable, pmp_set};

#[link_section = ".rodata"]
static DEFAULT_TSM: &[u8] = include_bytes!("../../tsm.bin");

#[link_section = ".rodata"]
static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../tsm.bin.signature");

#[link_section = ".rodata"]
static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../../publickey-pkcs1.der");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub fn init(fdt_addr: usize, next_addr: usize, next_mode: usize) {
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
        tsm_type: TsmType::None,
        start_address: 0,
        end_address: 0,
    });

    let mut node_iter = fdt.compatible_nodes("shadowfax,domain,instance");
    while let Some(node) = node_iter.next().unwrap() {
        let domain = Domain::from_fdt_node(&node);
        state.domains.push(domain.clone());

        // load the correct TSM for now only the default one is supported
        match domain.tsm_type {
            // Nothing to do, TSM is not required for this domain
            TsmType::None => {}
            // Load the default TSM. This involves:
            // - verify the hash
            // - loading the TSM in memory
            // - boot into the TSM
            TsmType::Default => {
                let tsm = Tsm::verify_and_load(
                    DEFAULT_TSM,
                    domain.next_addr,
                    DEFAULT_TSM_SIGN,
                    DEFAULT_TSM_PUBKEY,
                )
                .unwrap();
                tsm.bootstrap();
            }
            TsmType::Custom { name: _ } => panic!("Custom TSM are not supported yet"),
        }
    }
}

#[allow(unused)]
#[derive(Clone)]
enum TsmType {
    Default,
    Custom { name: String },
    None,
}

impl From<&str> for TsmType {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_ref() {
            "default" => TsmType::Default,
            "none" => TsmType::None,
            s => TsmType::Custom {
                name: String::from(s),
            },
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
    active: bool,
    start_address: usize,
    end_address: usize,
    tsm_type: TsmType,
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
    domains: Vec<Domain>,
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

struct Tsm {
    start_addr: usize,
    size: usize,
}

impl Tsm {
    fn verify_and_load(
        bin: &[u8],
        start_addr: usize,
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<Self, rsa::Error> {
        let signature = Signature::try_from(signature).unwrap();
        let verifying_key = VerifyingKey::<Sha256>::from_pkcs1_der(&public_key)?;
        verifying_key.verify(bin, &signature).unwrap();

        unsafe {
            core::ptr::copy_nonoverlapping(bin.as_ptr(), start_addr as *mut u8, bin.len());
        }
        Ok(Tsm {
            start_addr,
            size: bin.len(),
        })
    }

    fn bootstrap(&self) -> ! {
        // configure PMP
        let log2len = self.size.next_power_of_two().trailing_zeros();
        unsafe {
            pmp_set(1, 0x3F, self.start_addr as u64, log2len as u64);
        }

        // Save current general purpose registers
        // core::arch::asm!(
        //     "addi sp, sp, -{context_size}",
        //     "csrr t0, sstatus",
        //     "sd t0, 0*8(sp)",
        //     "csrr t0, stvec",
        //     "sd t0, 1*8(sp)",
        //     "sd zero, 2*8(sp)",
        //     "sd zero, 3*8(sp)",
        //     "sd zero, 4*8(sp)",
        //     "csrr t0, satp",
        //     "sd t0, 5*8(sp)",
        //     "sd zero, 6*8(sp)",
        //     context_size = const core::mem::size_of::<HartSupervisorStateArea>(),
        // );

        // context switch
        let mut mstatus = mstatus::read();
        mstatus.set_mpie(false);
        mstatus.set_mpp(mstatus::MPP::Supervisor);
        unsafe {
            mepc::write(self.start_addr);
            core::arch::asm!(
                "add a0, zero, zero",
                "csrw stvec, {start_address}",
                "csrw stval, 0",
                "csrw sscratch, zero",
                "csrw satp, zero",
                "csrw sie, zero",
                "mret",
                start_address = in(reg) self.start_addr,
                options(noreturn)
            );
        }
    }
}

struct HartSupervisorStateArea {
    sstatus: usize,
    stvec: usize,
    sip: usize,
    scounteren: usize,
    sscratch: usize,
    satp: usize,
    senvcfg: usize,
    scontext: usize,
    mpec: usize,
}

#[derive(Debug)]
enum TsmError {
    KeyError,
    SignError,
    LoadingError,
}

impl Display for TsmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        todo!()
    }
}

impl Error for TsmError {}
