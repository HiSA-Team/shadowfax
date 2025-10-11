use core::{error::Error, fmt::Display};

use alloc::vec::Vec;
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};
use fdt_rs::{
    base::DevTreeNode,
    prelude::{FallibleIterator, PropReader},
};
use rsa::{
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    signature::Verifier,
};
use sha2::Sha256;

#[derive(Clone)]
pub struct Tsm {
    pub id: usize,
    pub trust_map: usize,
    pub start_region_addr: usize,
    pub end_region_addr: usize,

    pub context_addr: usize,
    pub state_addr: usize,
    pub next_pmp_slot: usize,
}

impl Tsm {
    fn empty() -> Self {
        Self {
            id: 0,
            trust_map: 0,
            start_region_addr: 0,
            end_region_addr: 0,
            context_addr: 0,
            state_addr: 0,
            next_pmp_slot: 0,
        }
    }

    pub fn from_fdt_node(node: &DevTreeNode) -> Self {
        let mut tsm = Tsm::empty();
        for prop in node.props().iterator() {
            if let Ok(prop) = prop {
                let name = prop.name().unwrap_or("");
                match name {
                    "id" => tsm.id = prop.u32(0).unwrap() as usize,
                    "memory" => {
                        tsm.start_region_addr = prop.u64(0).unwrap() as usize;
                        tsm.end_region_addr = prop.u64(1).unwrap() as usize;
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
                        while let Ok(d) = node.u32(i) {
                            trust |= 1 << (d as usize);
                            i += 1
                        }
                        tsm.trust_map = trust;
                    }
                    _ => {}
                }
            }
        }

        tsm
    }

    pub fn verify_and_load(
        bin: &[u8],
        _start_addr: usize,
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<(), anyhow::Error> {
        // Verify the tsm signature with the provided payload using the the public key
        let public_key = str::from_utf8(public_key)?;
        let signature = Signature::try_from(signature).map_err(TsmError::SignatureError)?;
        let verifying_key = VerifyingKey::<Sha256>::from_pkcs1_pem(&public_key)
            .map_err(TsmError::RsaPublicKeyError)?;
        verifying_key
            .verify(bin, &signature)
            .map_err(TsmError::SignatureError)?;

        // load the tsm into the destination address
        let size = Self::load_elf(bin)?;

        assert!(size > 0);

        Ok(())
    }

    pub fn is_trusted(&self, dst: usize) -> bool {
        self.trust_map & (1 << dst) != 0
    }

    fn load_elf(data: &[u8]) -> anyhow::Result<usize> {
        let elf = ElfBytes::<AnyEndian>::minimal_parse(data).unwrap();

        let segments = elf
            .segments()
            .ok_or_else(|| anyhow::anyhow!("ELF has no program headers"))?;

        // Collect only loadable segments
        let load_segments: Vec<_> = segments.iter().filter(|ph| ph.p_type == PT_LOAD).collect();

        if load_segments.is_empty() {
            return Err(anyhow::anyhow!("No loadable segments found"));
        }

        let mut max_loaded_addr = 0usize;
        let mut min_loaded_addr = usize::MAX;

        // Load each PT_LOAD segment
        for ph in &load_segments {
            let p_offset = ph.p_offset as usize;
            let p_filesz = ph.p_filesz as usize;
            let p_vaddr = ph.p_vaddr as usize;
            let p_memsz = ph.p_memsz as usize;

            // Bounds check
            if p_offset + p_filesz > data.len() {
                return Err(anyhow::anyhow!("Segment data out of bounds"));
            }

            // Copy data into memory (dangerous — assumes addresses are valid)
            if p_filesz > 0 {
                let src = &data[p_offset..p_offset + p_filesz];
                unsafe {
                    core::ptr::copy_nonoverlapping(src.as_ptr(), p_vaddr as *mut u8, p_filesz);
                }
            }

            // Zero-fill .bss section
            if p_memsz > p_filesz {
                let bss_start = (p_vaddr + p_filesz) as *mut u8;
                let bss_len = p_memsz - p_filesz;
                unsafe {
                    core::ptr::write_bytes(bss_start, 0, bss_len);
                }
            }

            // Track memory range
            min_loaded_addr = min_loaded_addr.min(p_vaddr);
            max_loaded_addr = max_loaded_addr.max(p_vaddr + p_memsz);
        }

        // Return total size loaded in memory
        Ok(max_loaded_addr - min_loaded_addr)
    }
}

#[derive(Debug)]
pub enum TsmError {
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
