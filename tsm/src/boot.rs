//! Initialize once in S-mode, then return to Shadowfax with INIT_DONE.
use common::{attestation::TsmAttestationContext, tsm_abi::*};
use zeroize::Zeroize;

#[repr(C, align(16))]
struct TsmScratch {
    stack_pointer: usize,
}

pub(crate) fn initialize_and_return(boot_info: usize) -> ! {
    let (error, reason) = initialize(boot_info).err().unwrap_or((0, 0));
    unsafe {
        core::arch::asm!("ecall",
            in("a0") error, in("a1") reason,
            in("a6") SBI_SHADOWFAX_INIT_DONE,
            in("a7") SBI_SHADOWFAX_BOOT_EXT_ID,
            options(noreturn));
    }
}

/// A zero address is used only by the standalone test workload.
pub(crate) fn initialize(addr: usize) -> Result<(), (isize, isize)> {
    let mut state = crate::STATE.lock();
    if state.is_some() {
        return Err((-6, BOOT_ALREADY_INITIALIZED));
    }
    unsafe {
        let heap_start = (&raw const crate::_heap_start) as usize;
        let heap_end = (&raw const crate::_heap_end) as usize;
        crate::ALLOCATOR
            .lock()
            .init(heap_start as *mut u8, heap_end - heap_start);
    }
    let context = if addr == 0 && cfg!(feature = "standalone") {
        TsmAttestationContext::default()
    } else {
        if addr == 0 {
            return Err((-3, BOOT_INVALID_INFO));
        }
        // Shadowfax supplies an aligned record followed by the serialized context.
        let boot = unsafe { &*(addr as *const TsmBootInfo) };
        if boot.magic != TSM_BOOT_MAGIC
            || boot.abi_version != TSM_BOOT_ABI_VERSION
            || boot.struct_size as usize != size_of::<TsmBootInfo>()
            || boot.dice_context_addr != (addr + size_of::<TsmBootInfo>()) as u64
            || boot.dice_context_size > (TSM_BOOT_BUFFER_SIZE - size_of::<TsmBootInfo>()) as u64
        {
            return Err((-3, BOOT_INVALID_INFO));
        }
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                boot.dice_context_addr as *mut u8,
                boot.dice_context_size as usize,
            )
        };
        // Parsing copies the context into our heap. Erase the transfer copy even
        // when parsing fails, before returning control to Shadowfax.
        let context = TsmAttestationContext::from_slice(bytes);
        bytes.zeroize();
        context.map_err(|_| (-3, BOOT_INVALID_CONTEXT))?
    };
    *state = Some(crate::TsmState::new(context));
    let scratch =
        (addr + TSM_BOOT_BUFFER_SIZE - core::mem::size_of::<TsmScratch>()) as *mut TsmScratch;
    unsafe {
        // The erased transfer buffer becomes the runtime stack.
        (*scratch).stack_pointer = scratch as usize;
        core::arch::asm!("csrw sscratch, {}", in(reg) scratch as usize);
    }
    Ok(())
}
