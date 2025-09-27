#![no_std]
#![no_main]

pub mod tsm {
    use heapless::Vec;

    const MAX_PAGE_BLOCKS: usize = 4;
    const PAGE_SIZE: usize = 4096;
    const PAGE_DIRECTORY_SIZE: usize = 16 * 1024;
    const MAX_MEMORY_REGIONS: usize = 8; // per-TVM simple limit

    // Small helper types
    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct MemoryRegion {
        pub guest_gpa_base: usize,
        pub num_pages: usize,
    }

    #[repr(C)]
    pub struct State {
        pub info: TsmInfo,
        pub tvm: Option<Tvm>,
        confidential_memory: Vec<(usize, usize, Option<usize>), MAX_PAGE_BLOCKS>,
    }

    impl State {
        pub fn new() -> Self {
            Self {
                info: TsmInfo {
                    tsm_state: TsmState::TsmReady,
                    tsm_impl_id: 69,
                    tsm_version: 69,
                    tsm_capabilities: 0,
                    tvm_state_pages: 1,
                    tvm_max_vcpus: 1,
                    tvm_vcpu_state_pages: 0,
                },
                tvm: None,
                confidential_memory: Vec::new(),
            }
        }
        pub fn add_confidential_pages(
            &mut self,
            base_page_addr: usize,
            num_pages: usize,
        ) -> anyhow::Result<()> {
            self.confidential_memory
                .push((base_page_addr, num_pages, None))
                .map_err(|c| {
                    anyhow::anyhow!(
                        "out of confidential memory; failed to insert {} pages from {}",
                        c.1,
                        c.0
                    )
                })?;

            Ok(())
        }

        pub fn create_tvm(
            &mut self,
            page_table_addr: usize,
            state_addr: usize,
        ) -> anyhow::Result<usize> {
            if self.tvm.is_some() {
                anyhow::bail!("already created tvm");
            }

            // Sanity: page_table_addr must be aligned and confidential
            if page_table_addr % PAGE_DIRECTORY_SIZE != 0 {
                anyhow::bail!("page table addr must be 16KB-aligned");
            }

            // Sanity: the state address must not be into the TVM page table
            assert!(
                state_addr < page_table_addr
                    || state_addr > (page_table_addr + PAGE_DIRECTORY_SIZE - 8)
            );

            // Check that page_directory region is within some donated confidential block
            let pd_block_idx = self
                .find_confidential_block_idx_covering(page_table_addr, PAGE_DIRECTORY_SIZE)
                .ok_or_else(|| anyhow::anyhow!("page directory addr not in confidential memory"))?;

            // For the state area we will accept at least one page (or tvm_state_pages if non-zero)
            // your harness has tvm_state_pages==0, so accept a single page slot for now.
            let state_block_idx = self
                .find_confidential_block_idx_covering(state_addr, PAGE_SIZE)
                .ok_or_else(|| anyhow::anyhow!("state addr not in confidential memory"))?;

            // mark those blocks as owned by tvm id=1 (we only support single tvm)
            // update the Option<usize> owner inside confidential_memory
            {
                let (base, npages, owner) = self
                    .confidential_memory
                    .get_mut(pd_block_idx)
                    .ok_or_else(|| anyhow::anyhow!("invalid pd block idx"))?;
                *owner = Some(1);
            }
            {
                let (base, npages, owner) = self
                    .confidential_memory
                    .get_mut(state_block_idx)
                    .ok_or_else(|| anyhow::anyhow!("invalid state block idx"))?;
                *owner = Some(1);
            }
            // Zero the page-directory region in-place
            unsafe {
                let ptr = page_table_addr as *mut u8;
                // safety: test harness writes to this memory in GDB; for a real platform you'd convert mapping
                core::ptr::write_bytes(ptr, 0, PAGE_DIRECTORY_SIZE);
            }

            // TODO: init page table and init tvm state
            let tvm = Tvm::new(page_table_addr, state_addr);
            let tvm_id = tvm.id;
            self.tvm = Some(tvm);

            Ok(tvm_id)
        }

        pub fn destroy_tvm(&mut self) -> anyhow::Result<()> {
            if let Some(tvm) = &self.tvm {
                //TODO:
                unsafe {
                    let ptr = tvm.page_table_addr as *mut u8;
                    core::ptr::write_bytes(ptr, 0, PAGE_DIRECTORY_SIZE);
                }
            }
            self.tvm = None;
            Ok(())
        }

        // Add a simple API to register a guest memory region for the TVM
        pub fn add_tvm_memory_region(
            &mut self,
            tvm_id: usize,
            guest_gpa_base: usize,
            num_pages: usize,
        ) -> anyhow::Result<()> {
            if self.tvm.is_none() {
                anyhow::bail!("no tvm present");
            }
            let t = self.tvm.as_mut().unwrap();
            if t.id != tvm_id {
                anyhow::bail!("tvm id mismatch");
            }
            // avoid overlap check for simplicity or add simple overlap check:
            for r in t.memory_regions.iter() {
                let a1 = r.guest_gpa_base;
                let b1 = a1 + r.num_pages * PAGE_SIZE;
                let a2 = guest_gpa_base;
                let b2 = a2 + num_pages * PAGE_SIZE;
                // overlapping if ranges intersect
                if !(b2 <= a1 || b1 <= a2) {
                    anyhow::bail!("region overlap with existing region");
                }
            }

            t.memory_regions
                .push(MemoryRegion {
                    guest_gpa_base,
                    num_pages,
                })
                .map_err(|_| anyhow::anyhow!("too many memory regions"))?;

            Ok(())
        }

        fn find_confidential_block_idx_covering(&self, addr: usize, size: usize) -> Option<usize> {
            for (i, &(base, npages, _owner)) in self.confidential_memory.iter().enumerate() {
                let region_bytes = npages * PAGE_SIZE;
                if addr >= base && (addr + size) <= (base + region_bytes) {
                    return Some(i);
                }
            }
            None
        }
    }

    #[repr(C)]
    pub struct Tvm {
        pub id: usize,
        pub vcpu_state: [u64; 32],
        pub page_table_addr: usize,
        pub state_addr: usize,
        pub memory_regions: heapless::Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
        pub state_enum: TvmState,
    }

    impl Tvm {
        fn new(page_table_addr: usize, state_addr: usize) -> Self {
            Self {
                id: 1,
                vcpu_state: [0; 32],
                page_table_addr,
                state_addr,
                memory_regions: Vec::new(),
                state_enum: TvmState::TvmInitializing,
            }
        }
    }

    #[repr(C)]
    #[derive(Clone, Debug)]
    pub struct TsmInfo {
        /*
         * The current state of the TSM (see `tsm_state` enum above).
         * If the state is not `TSM_READY`, the remaining fields are invalid and
         * will be initialized to `0`.
         */
        pub tsm_state: TsmState,
        /*
         * Identifier of the TSM implementation, see `Reserved TSM Implementation IDs`
         * table below. This identifier is intended to distinguish among different TSM
         * implementations, potentially managed by different organizations, that might
         * target different deployment models and, thus, implement subset of CoVE spec.
         */
        pub tsm_impl_id: u32,
        /*
         * Version number of the running TSM.
         */
        pub tsm_version: u32,
        /*
         * A bitmask of CoVE features supported by the running TSM, see `TSM Capabilities`
         * table below. Every bit in this field corresponds to a capability defined by
         * `COVE_TSM_CAP_*` constants. Presence of bit `i` indicates that both the TSM
         * and hardware support the corresponding capability.
         */
        pub tsm_capabilities: usize,
        /*
         * The number of 4KB pages which must be donated to the TSM for storing TVM
         * state in sbi_covh_create_tvm_vcpu(). `0` if the TSM does not support the
         * dynamic memory allocation capability.
         */
        pub tvm_state_pages: usize,
        /*
         * The maximum number of vCPUs a TVM can support.
         */
        pub tvm_max_vcpus: usize,
        /*
         * The number of 4KB pages which must be donated to the TSM when creating
         * a new vCPU. `0` if the TSM does not support the dynamic memory allocation
         * capability.
         */
        pub tvm_vcpu_state_pages: usize,
    }
    /*
     * TsmPageType is an enumeration that defines the types of memory pages supported by the TSM.
     * It includes options for 4 KiB, 2 MiB, 1 GiB, and 512 GiB pages, allowing for flexible memory
     * management and allocation.
     */
    pub enum TsmPageType {
        /* 4 KiB */
        Page4k = 0,
        /* 2 MiB */
        Page2mb = 1,
        /* 1 GiB */
        Page1gb = 2,
        /* 512 GiB */
        Page512gb = 3,
    }

    /*
     * TvmState is an enumeration that represents the state of a Trusted Virtual Machine (TVM).
     * It indicates whether the TVM is in the process of initialization or is ready to run.
     */
    #[derive(Clone)]
    pub enum TvmState {
        /* The TVM has been created, but isn't yet ready to run */
        TvmInitializing = 0,
        /* The TVM is in a runnable state */
        TvmRunnable = 1,
    }

    /*
     * TsmState is an enumeration that describes the current state of the Trusted Software Module (TSM).
     * It provides information on whether the TSM is not loaded, loaded but not initialized, or fully
     * initialized and ready to accept ECALLs (environment calls).
     */
    #[derive(Clone, Debug)]
    pub enum TsmState {
        /* TSM has not been loaded on this platform. */
        TsmNotLoaded = 0,
        /* TSM has been loaded, but has not yet been initialized. */
        TsmLoaded = 1,
        /* TSM has been loaded & initialized, and is ready to accept ECALLs.*/
        TsmReady = 2,
    }
}
