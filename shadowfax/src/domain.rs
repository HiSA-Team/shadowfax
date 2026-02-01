use alloc::vec::Vec;
use elf::{abi::PT_LOAD, endian::AnyEndian, ElfBytes};
use rsa::{
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    signature::Verifier,
};
use sha2::Sha256;

use crate::{context::Context, error::TsmError};

mod tsm {
    #[link_section = ".rodata"]
    pub static DEFAULT_TSM: &[u8] =
        include_bytes!("../../target/riscv64imac-unknown-none-elf/debug/tsm");

    #[link_section = ".rodata"]
    pub static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../bin/tsm.bin.signature");

    #[link_section = ".rodata"]
    pub static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../keys/publickey.pem");
}

#[derive(Clone)]
pub struct MemoryRegion {
    pub base_addr: usize,
    pub order: u32,
    pub mmio: bool,
    pub permissions: u8,
}

#[derive(Clone)]
pub struct Domain {
    pub trust_map: usize,
    pub memory_regions: Vec<MemoryRegion>,

    pub context_addr: usize,
    pub security_context: Option<[u8; 256]>,
}

impl Domain {
    pub fn empty() -> Self {
        Self {
            trust_map: 0,
            memory_regions: Vec::new(),
            context_addr: 0,
            security_context: None,
        }
    }

    /// Loads the TSM elf, verify it's signature
    pub fn verify_and_load_tsm(
        bin: &[u8],
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

pub fn create_confidential_domain(context_addr: usize) -> Domain {
    // Assume that the specified domain is a trusted domain -> need to load the TSM in it
    // TODO: parse domain from FDT
    let tsm_ctx = context_addr as *mut Context;
    let mut domain = Domain::empty();

    // Trust both root and untrusted domains
    domain.trust_map = (1 << 2) | (1 << 0);

    // Hardcoded memory regions for now
    domain.memory_regions = [
        MemoryRegion {
            base_addr: 0x8200_0000,
            order: 24,
            permissions: 0x3f,
            mmio: false,
        },
        MemoryRegion {
            base_addr: 0x1000_0000,
            order: 12,
            permissions: 0x3f,
            mmio: true,
        },
    ]
    .to_vec();

    // Save the context address and the state address
    domain.context_addr = context_addr;
    domain.security_context = Some([0; 256]);

    // Configure PMP entry for TMem
    let tmem_region = &domain.memory_regions[0];

    // zero out the tsm supervisor state area
    // setup basic registers for first context switch
    unsafe {
        // zero out memory
        core::ptr::write_bytes(tsm_ctx, 0, 1);

        // init values
        (*tsm_ctx).mepc = tmem_region.base_addr;
    }

    Domain::verify_and_load_tsm(
        tsm::DEFAULT_TSM,
        tsm::DEFAULT_TSM_SIGN,
        tsm::DEFAULT_TSM_PUBKEY,
    )
    .unwrap();

    return domain;
}
