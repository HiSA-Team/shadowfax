use alloc::string::String;
use common::{
    attestation::{DiceLayer, TsmAttestationContext},
    tsm_abi::{TsmBootInfo, TSM_BOOT_ABI_VERSION, TSM_BOOT_MAGIC},
};
use ed25519_compact::Signature;
use elf::{
    abi::{ET_DYN, ET_EXEC, PF_W, PF_X, PT_LOAD, R_RISCV_RELATIVE},
    endian::AnyEndian,
    ElfBytes,
};
use heapless::Vec;
use sha2::{Digest, Sha512};
use zeroize::Zeroize;

use crate::{
    context::Context,
    error::TsmError,
    platform::{DomainConfig, TsmSource, MAX_MEMORY_REGIONS},
};

mod tsm {
    #[link_section = ".rodata"]
    pub static DEFAULT_TSM: &[u8] = include_bytes!("../../bin/tsm.elf");

    #[link_section = ".rodata"]
    pub static DEFAULT_TSM_SIGN: &[u8] = include_bytes!("../../bin/tsm.bin.signature");

    #[link_section = ".rodata"]
    pub static DEFAULT_TSM_PUBKEY: &[u8] = include_bytes!("../keys/publickey.pem");
}

#[derive(Clone, Debug)]
pub struct MemoryRegion {
    pub base_address: usize,
    pub order: u32,
    pub mmio: bool,
    pub permissions: u8,
}

#[derive(Clone)]
pub struct Domain {
    pub name: String,
    pub trust_map: usize,
    pub memory_regions: Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
    pub next_addr: usize,
    pub context_addr: usize,
    pub has_tsm: bool,
    pub boot_hart: bool,
}

impl Domain {
    pub fn is_trusted(&self, dst: usize) -> bool {
        self.trust_map & (1 << dst) != 0
    }
}

pub fn create_confidential_domain(
    config: DomainConfig,
    context_addr: usize,
    platform_attestation: &common::attestation::PlatformAttestationContext,
) -> anyhow::Result<Domain> {
    let (image, signature) = match config.tsm_source {
        TsmSource::Builtin => (tsm::DEFAULT_TSM, tsm::DEFAULT_TSM_SIGN),
        TsmSource::External { image, signature } => unsafe {
            (
                core::slice::from_raw_parts(image.base as *const u8, image.size),
                core::slice::from_raw_parts(signature.base as *const u8, signature.size),
            )
        },
        TsmSource::None => return Err(anyhow::anyhow!("TSM domain has no image")),
    };
    verify_tsm(image, signature)?;
    let measurement: [u8; 64] = Sha512::digest(image).into();
    let attestation_context = platform_attestation.compute_next(&measurement);
    let tsm_ctx = context_addr as *mut Context;
    let domain = Domain {
        name: config.name,
        trust_map: config.trust_map,
        memory_regions: config.memory_regions,
        next_addr: config.next_addr,
        context_addr,
        has_tsm: true,
        boot_hart: config.boot_hart,
    };

    // zero out the tsm supervisor state area
    // setup basic registers for first context switch
    unsafe {
        // zero out memory
        core::ptr::write_bytes(tsm_ctx, 0, 1);

        // init values
        (*tsm_ctx).mepc = domain.next_addr;
    }

    let load_bias = load_tsm_elf(image, domain.next_addr, &domain.memory_regions)?;

    // Boot and initialize secure_init safely
    boot_tsm(
        image,
        load_bias,
        config.id,
        measurement,
        attestation_context,
    )?;

    Ok(domain)
}

fn verify_tsm(image: &[u8], signature: &[u8]) -> anyhow::Result<()> {
    let public_key = str::from_utf8(tsm::DEFAULT_TSM_PUBKEY)?;
    let signature = Signature::from_slice(signature).map_err(TsmError::SignatureDecode)?;
    let verifying_key = from_public_pem(public_key).map_err(TsmError::PublicKeyDecode)?;
    verifying_key
        .verify(image, &signature)
        .map_err(TsmError::SignatureVerification)?;
    Ok(())
}

fn load_tsm_elf(data: &[u8], next_addr: usize, regions: &[MemoryRegion]) -> anyhow::Result<usize> {
    let elf = ElfBytes::<AnyEndian>::minimal_parse(data)
        .map_err(|error| anyhow::anyhow!("cannot parse TSM ELF: {error:?}"))?;
    let load_bias = match elf.ehdr.e_type {
        ET_DYN => next_addr
            .checked_sub(elf.ehdr.e_entry as usize)
            .ok_or_else(|| anyhow::anyhow!("TSM entry cannot be relocated to domain entry"))?,
        ET_EXEC if elf.ehdr.e_entry as usize == next_addr => 0,
        ET_EXEC => {
            return Err(anyhow::anyhow!(
                "fixed TSM entry does not match domain entry"
            ))
        }
        _ => return Err(anyhow::anyhow!("unsupported TSM ELF type")),
    };
    let segments = elf
        .segments()
        .ok_or_else(|| anyhow::anyhow!("TSM ELF has no program headers"))?;
    let mut entry_is_executable = false;
    for ph in segments.iter().filter(|ph| ph.p_type == PT_LOAD) {
        let destination = load_bias
            .checked_add(ph.p_vaddr as usize)
            .ok_or_else(|| anyhow::anyhow!("TSM segment address overflows"))?;
        let memory_end = destination
            .checked_add(ph.p_memsz as usize)
            .ok_or_else(|| anyhow::anyhow!("TSM segment range overflows"))?;
        let file_end = (ph.p_offset as usize)
            .checked_add(ph.p_filesz as usize)
            .ok_or_else(|| anyhow::anyhow!("TSM file range overflows"))?;
        if ph.p_filesz > ph.p_memsz || file_end > data.len() {
            return Err(anyhow::anyhow!("TSM segment is outside the ELF"));
        }
        if !range_allowed(destination, memory_end, ph.p_flags as u8, regions) {
            return Err(anyhow::anyhow!(
                "TSM segment is outside its supervisor domain"
            ));
        }
        if ph.p_flags & PF_X != 0 && next_addr >= destination && next_addr < memory_end {
            entry_is_executable = true;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                data.as_ptr().add(ph.p_offset as usize),
                destination as *mut u8,
                ph.p_filesz as usize,
            );
            core::ptr::write_bytes(
                (destination + ph.p_filesz as usize) as *mut u8,
                0,
                (ph.p_memsz - ph.p_filesz) as usize,
            );
        }
    }
    if !entry_is_executable {
        return Err(anyhow::anyhow!("TSM entry is not executable"));
    }
    if elf.ehdr.e_type == ET_DYN {
        let relocations = elf
            .section_header_by_name(".rela.dyn")
            .map_err(|error| anyhow::anyhow!("cannot find TSM relocations: {error:?}"))?
            .ok_or_else(|| anyhow::anyhow!("PIE TSM has no .rela.dyn"))?;
        for relocation in elf
            .section_data_as_relas(&relocations)
            .map_err(|error| anyhow::anyhow!("cannot parse TSM relocations: {error:?}"))?
        {
            if relocation.r_type != R_RISCV_RELATIVE || relocation.r_sym != 0 {
                return Err(anyhow::anyhow!("unsupported TSM relocation"));
            }
            let destination = load_bias
                .checked_add(relocation.r_offset as usize)
                .ok_or_else(|| anyhow::anyhow!("TSM relocation address overflows"))?;
            let destination_end = destination
                .checked_add(size_of::<usize>())
                .ok_or_else(|| anyhow::anyhow!("TSM relocation range overflows"))?;
            let is_writable_segment = segments
                .iter()
                .filter(|ph| ph.p_type == PT_LOAD && ph.p_flags & PF_W != 0)
                .any(|ph| {
                    let Some(start) = load_bias.checked_add(ph.p_vaddr as usize) else {
                        return false;
                    };
                    let Some(end) = start.checked_add(ph.p_memsz as usize) else {
                        return false;
                    };
                    destination >= start && destination_end <= end
                });
            if !is_writable_segment {
                return Err(anyhow::anyhow!("TSM relocation target is not writable"));
            }
            let value = load_bias
                .checked_add_signed(relocation.r_addend as isize)
                .ok_or_else(|| anyhow::anyhow!("TSM relocation value overflows"))?;
            unsafe { (destination as *mut usize).write_unaligned(value) };
        }
    }
    Ok(load_bias)
}

fn range_allowed(start: usize, end: usize, permissions: u8, regions: &[MemoryRegion]) -> bool {
    regions.iter().any(|region| {
        let Some(size) = 1usize.checked_shl(region.order) else {
            return false;
        };
        let Some(region_end) = region.base_address.checked_add(size) else {
            return false;
        };
        !region.mmio
            && start >= region.base_address
            && end <= region_end
            && region.permissions & (permissions & 0x7) == permissions & 0x7
    })
}

/// Resolve the verified secure-init symbol and invoke its versioned C ABI.
fn boot_tsm(
    data: &[u8],
    load_bias: usize,
    domain_id: usize,
    measurement: [u8; 64],
    attestation_context: TsmAttestationContext,
) -> anyhow::Result<()> {
    let elf = ElfBytes::<AnyEndian>::minimal_parse(data)
        .map_err(|error| anyhow::anyhow!("cannot parse TSM ELF: {error:?}"))?;

    // get static symbol table instead of dynsym
    let (symtab, strtab) = elf
        .symbol_table()
        .map_err(|error| anyhow::anyhow!("cannot inspect TSM symbols: {error:?}"))?
        .ok_or_else(|| anyhow::anyhow!("TSM ELF has no symbol table"))?;

    // find symbol by iterating static symbols
    let name = b"_secure_init";

    let mut found = None;
    for sym in symtab {
        if let Ok(a) = strtab.get(sym.st_name as usize) {
            if a.as_bytes() == name {
                found = Some(sym);
                break;
            }
        }
    }

    let sym = found.ok_or_else(|| anyhow::anyhow!("cannot find _secure_init"))?;

    let mut encoded_context = attestation_context.to_raw_bytes();
    let boot_info = TsmBootInfo {
        magic: TSM_BOOT_MAGIC,
        abi_version: TSM_BOOT_ABI_VERSION,
        struct_size: size_of::<TsmBootInfo>() as u32,
        domain_id: domain_id as u64,
        load_base: load_bias as u64,
        measurement,
        dice_context_addr: encoded_context.as_ptr() as u64,
        dice_context_size: encoded_context.len() as u64,
    };
    let init_address = load_bias
        .checked_add(sym.st_value as usize)
        .ok_or_else(|| anyhow::anyhow!("secure-init address overflows"))?;
    let init_is_executable = elf
        .segments()
        .ok_or_else(|| anyhow::anyhow!("TSM ELF has no program headers"))?
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD && ph.p_flags & PF_X != 0)
        .any(|ph| {
            let Some(start) = load_bias.checked_add(ph.p_vaddr as usize) else {
                return false;
            };
            let Some(end) = start.checked_add(ph.p_memsz as usize) else {
                return false;
            };
            init_address >= start && init_address < end
        });
    if !init_is_executable {
        encoded_context.zeroize();
        return Err(anyhow::anyhow!(
            "_secure_init is not in an executable segment"
        ));
    }
    unsafe {
        let secure_init_fn =
            core::mem::transmute::<usize, extern "C" fn(usize) -> isize>(init_address);
        if secure_init_fn(&boot_info as *const TsmBootInfo as usize) != 0 {
            encoded_context.zeroize();
            return Err(anyhow::anyhow!("TSM secure initialization failed"));
        }
    }
    encoded_context.zeroize();
    Ok(())
}

/// THIS FUNCTION SHOULD NOT EXISTS. IT IS A TEMPORARY FIX SINCE THE ED25519 LIBRARY DEPENDS ON
/// STD TO PARSE THE PEM
use base64ct::Encoding;

const DER_HEADER_PK: [u8; 12] = [48, 42, 48, 5, 6, 3, 43, 101, 112, 3, 33, 0];

fn from_public_pem(pem: &str) -> Result<ed25519_compact::PublicKey, ed25519_compact::Error> {
    let mut it = pem.split("-----BEGIN PUBLIC KEY-----");
    let _ = it.next().ok_or(ed25519_compact::Error::ParseError)?;
    let inner = it.next().ok_or(ed25519_compact::Error::ParseError)?;
    let mut it = inner.split("-----END PUBLIC KEY-----");
    let b64 = it.next().ok_or(ed25519_compact::Error::ParseError)?;
    let _ = it.next().ok_or(ed25519_compact::Error::ParseError)?;

    let mut der = [0u8; DER_HEADER_PK.len() + 32]; // 32-byte public key
    let b64_clean = b64.trim();
    base64ct::Base64::decode(b64_clean.as_bytes(), &mut der)
        .map_err(|_| ed25519_compact::Error::ParseError)?;

    if der.len() != DER_HEADER_PK.len() + 32 || der[0..DER_HEADER_PK.len()] != DER_HEADER_PK {
        return Err(ed25519_compact::Error::ParseError);
    }

    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(&der[DER_HEADER_PK.len()..]);
    Ok(ed25519_compact::PublicKey::from_slice(&pk_bytes)?)
}
