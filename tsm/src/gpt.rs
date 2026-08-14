use common::sbi::PAGE_SIZE;

pub const PTE_SIZE: usize = 8;
pub const PTE_V: u64 = 1 << 0;
pub const PTE_R: u64 = 1 << 1;
pub const PTE_W: u64 = 1 << 2;
pub const PTE_X: u64 = 1 << 3;
const PTE_U: u64 = 1 << 4;
pub const PTE_A: u64 = 1 << 6;
pub const PTE_D: u64 = 1 << 7;

#[derive(Clone, Copy, Debug)]
pub struct MemoryRegion {
    pub guest_gpa_base: usize,
    pub num_pages: usize,
}

/// Return the 3 VPN indices [vpn2, vpn1, vpn0] for SV39.
#[inline(always)]
fn make_vpn_sv39(gpa: usize) -> [usize; 3] {
    [
        (gpa >> 30) & 0x1FF, // VPN[2]
        (gpa >> 21) & 0x1FF, // VPN[1]
        (gpa >> 12) & 0x1FF, // VPN[0]
    ]
}

#[inline(always)]
fn pa_to_ppn(pa: usize) -> u64 {
    (pa as u64) >> 12
}

#[inline(always)]
fn ppn_to_pa(ppn: u64) -> usize {
    (ppn << 12) as usize
}

/// Map a single 4 KiB page in SV39 page tables.
/// Dynamically allocates page tables within the region supplied by the host.
///
/// Memory layout:
///   root_pt + 0x0000: L2 table (root)
///   root_pt + 0x1000: L1 table (shared for all VPN[2]=0)
///   root_pt + 0x2000 + VPN[1] * 0x1000: L0 tables
///
/// Note: This assumes all mappings use VPN[2]=0 (addresses < 1GB)
pub fn map_4k_leaf(root_pt: usize, page_table_size: usize, gpa: usize, pa: usize, perms: u64) {
    assert_eq!(gpa % PAGE_SIZE, 0, "GPA must be page-aligned");
    assert_eq!(pa % PAGE_SIZE, 0, "PA must be page-aligned");

    let [vpn2, vpn1, vpn0] = make_vpn_sv39(gpa);

    // Level 2 -> Level 1
    let pte2_addr = root_pt + vpn2 * PTE_SIZE;
    let pte2 = unsafe { core::ptr::read_volatile(pte2_addr as *const u64) };

    let l1_base = if pte2 & PTE_V == 0 {
        // L1 table doesn't exist, create it
        let l1_base = root_pt + 0x4000;
        let pte = (pa_to_ppn(l1_base) << 10) | PTE_V;
        unsafe {
            core::ptr::write_volatile(pte2_addr as *mut u64, pte);
        }
        l1_base
    } else {
        // L1 already exists, extract its address
        ppn_to_pa(pte2 >> 10)
    };

    // Level 1 -> Level 0
    let pte1_addr = l1_base + vpn1 * PTE_SIZE;
    let pte1 = unsafe { core::ptr::read_volatile(pte1_addr as *const u64) };

    let l0_base = if pte1 & PTE_V == 0 {
        // L0 table doesn't exist, allocate it
        // Allocate one L0 table for each populated VPN[1].
        let l0_base = root_pt + 0x2000 + (vpn1 * PAGE_SIZE);

        // Check that the host supplied enough space for another L0 table.
        assert!(
            l0_base + PAGE_SIZE <= root_pt + page_table_size,
            "Insufficient space for L0 table at VPN[1]={}",
            vpn1
        );

        let pte = (pa_to_ppn(l0_base) << 10) | PTE_V | PTE_U;
        unsafe {
            core::ptr::write_volatile(pte1_addr as *mut u64, pte);
        }
        l0_base
    } else {
        // L0 already exists
        ppn_to_pa(pte1 >> 10)
    };

    // Level 0 (leaf)
    let pte0_addr = l0_base + vpn0 * PTE_SIZE;
    let leaf = (pa_to_ppn(pa) << 10) | perms | PTE_V | PTE_U;
    unsafe {
        core::ptr::write_volatile(pte0_addr as *mut u64, leaf);
    }
}

/// Translates a Guest Physical Address (GPA) to a Host Physical Address (PA)
/// by walking the SV39 page table structure starting at `root_pt`.
/// Returns `None` if the address is not mapped.
pub fn translate_gpa_to_pa(root_pt: usize, gpa: usize) -> Option<usize> {
    let [vpn2, vpn1, vpn0] = make_vpn_sv39(gpa);

    // --- Level 2 (Root) ---
    // Calculate address of the PTE in the L2 table
    let pte2_addr = root_pt + (vpn2 * 8);
    let pte2 = unsafe { core::ptr::read_volatile(pte2_addr as *const u64) };

    // 1. Check Valid bit
    if (pte2 & PTE_V) == 0 {
        return None; // Page fault (not mapped)
    }

    // 2. Check for Leaf (Huge Page 1GB)
    // If R, W, or X is set, this is a leaf node, not a pointer to the next level.
    if (pte2 & (PTE_R | PTE_W | PTE_X)) != 0 {
        // PPN holds the 1GB aligned base address
        let ppn = (pte2 >> 10) & 0x003F_FFFF_FFFF_FFFF;
        // PA = (PPN << 12) | Offset within 1GB (30 bits)
        return Some(ppn_to_pa(ppn) | (gpa & 0x3FFF_FFFF));
    }

    // --- Level 1 ---
    // pte2 was a pointer to the L1 table
    let l1_base = ppn_to_pa(pte2 >> 10);
    let pte1_addr = l1_base + (vpn1 * 8);
    let pte1 = unsafe { core::ptr::read_volatile(pte1_addr as *const u64) };

    if (pte1 & PTE_V) == 0 {
        return None;
    }

    // Check for Leaf (Huge Page 2MB)
    if (pte1 & (PTE_R | PTE_W | PTE_X)) != 0 {
        let ppn = (pte1 >> 10) & 0x003F_FFFF_FFFF_FFFF;
        // PA = (PPN << 12) | Offset within 2MB (21 bits)
        return Some(ppn_to_pa(ppn) | (gpa & 0x1F_FFFF));
    }

    // --- Level 0 (4KB Page) ---
    // pte1 was a pointer to the L0 table
    let l0_base = ppn_to_pa(pte1 >> 10);
    let pte0_addr = l0_base + (vpn0 * 8);
    let pte0 = unsafe { core::ptr::read_volatile(pte0_addr as *const u64) };

    if (pte0 & PTE_V) == 0 {
        return None;
    }

    // This must be a leaf (standard 4KB page)
    if (pte0 & (PTE_R | PTE_W | PTE_X)) == 0 {
        return None; // Invalid format: L0 PTE must be a leaf
    }

    let ppn = (pte0 >> 10) & 0x003F_FFFF_FFFF_FFFF;
    // PA = (PPN << 12) | Offset within 4KB (12 bits)
    Some(ppn_to_pa(ppn) | (gpa & 0xFFF))
}

/// Map a contiguous region of memory (multiple 4KB pages).
pub fn map_region(
    root_pt: usize,
    page_table_size: usize,
    gpa_base: usize,
    pa_base: usize,
    num_pages: usize,
    perms: u64,
) {
    for i in 0..num_pages {
        // TODO align GPA to PAGE
        let gpa = gpa_base + i * PAGE_SIZE;
        let pa = pa_base + i * PAGE_SIZE;
        map_4k_leaf(root_pt, page_table_size, gpa, pa, perms);
    }
}

/// Reads `len` bytes from Guest Physical Address `gpa` into `buf`.
/// Returns error if translation fails or crosses page boundary.
pub fn read_guest_memory(root_pt: usize, gpa: usize, buf: &mut [u8]) -> Result<(), ()> {
    // 1. Check for page crossing (Simplified: fail if it crosses)
    if (gpa & 0xFFF) + buf.len() > 4096 {
        return Err(());
    }

    // 2. Translate
    let host_pa = translate_gpa_to_pa(root_pt, gpa).ok_or(())?;

    // 3. Copy
    unsafe {
        let src = host_pa as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), buf.len());
    }
    Ok(())
}

/// Writes `data` to Guest Physical Address `gpa`.
pub fn write_guest_memory(root_pt: usize, gpa: usize, data: &[u8]) -> Result<(), ()> {
    if (gpa & 0xFFF) + data.len() > 4096 {
        return Err(());
    }

    let host_pa = translate_gpa_to_pa(root_pt, gpa).ok_or(())?;

    unsafe {
        let dst = host_pa as *mut u8;
        core::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
    }
    Ok(())
}
