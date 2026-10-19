//! Global runtime state for the TSM driver.
//!
//! [`init`] builds this state from the platform FDT supplied by the previous
//! boot stage in register `a1`. The FDT contains an
//! `opensbi,domain,config` node under `/chosen`; its
//! `opensbi,domain,instance` subnodes define the supervisor domains, while
//! referenced `opensbi,domain,memregion` nodes define their memory access.
//!
//! OpenSBI reserves domain ID 0 for its root domain. Shadowfax mirrors that
//! root at index 0 and assigns IDs 1 onward to domain-instance nodes in device
//! tree order, matching OpenSBI's enumeration. Consequently, a domain's ID is
//! also its index in [`State::domains`]. Exactly one instance must contain the
//! standard `boot-hart` property; its `next-addr` is the next-stage entry point.
//!
//! Shadowfax adds two properties to the OpenSBI domain description:
//!
//! - `shadowfax,load-tsm` marks a domain in which Shadowfax must verify and
//!   load the embedded TSM.
//! - `shadowfax,trusts` contains domain phandles accepted by that TSM. The
//!   parser resolves those phandles into the runtime trust bitmap.
//!
//! A minimal two-domain configuration therefore has this form:
//!
//! ```text
//! opensbi-domains {
//!     compatible = "opensbi,domain,config";
//!
//!     tmem: trusted-memory {
//!         compatible = "opensbi,domain,memregion";
//!         base = <0x0 0x90000000>;
//!         order = <25>;
//!     };
//!
//!     umem: untrusted-memory {
//!         compatible = "opensbi,domain,memregion";
//!         base = <0x0 0x8a000000>;
//!         order = <25>;
//!     };
//!
//!     trusted-domain {
//!         compatible = "opensbi,domain,instance";
//!         possible-harts = <&cpu0>;
//!         regions = <&tmem 0x3f>;
//!         next-addr = <0x0 0x90000000>;
//!         next-mode = <0x1>;
//!         shadowfax,load-tsm;
//!         shadowfax,trusts = <&udomain>;
//!     };
//!
//!     udomain: untrusted-domain {
//!         compatible = "opensbi,domain,instance";
//!         possible-harts = <&cpu0>;
//!         boot-hart = <&cpu0>;
//!         regions = <&umem 0x3f>;
//!         next-addr = <0x0 0x8a000000>;
//!         next-mode = <0x1>;
//!     };
//! };
//! ```
//!
//! Author: Giuseppe Capasso <capassog97@gmail.com>

use core::cell::OnceCell;

use alloc::string::String;
use common::attestation::{DiceLayer, PlatformAttestationContext};
use heapless::Vec;
use spin::mutex::Mutex;

use crate::{
    context::Context,
    cove::TEE_SCRATCH_SIZE,
    domain::{create_confidential_domain, initialize_default_tsm, Domain, MemoryRegion},
    platform::{DomainConfig, PlatformConfig, MAX_SUPERVISOR_DOMAINS},
    print_raw,
};

/* This comes from exploting CoVE FID (register a6) format. The idea is to store the ticket in
 * bits[25.16]
31          26 25              16 15                           0
+-------------+------------------+------------------------------+
|    SDID     |     Reserved     |             FID              |
+-------------+------------------+------------------------------+
 * */
const TICKET_BITS: usize = 10;
const TICKET_MASK: usize = (1 << TICKET_BITS) - 1; // 0x3ff
                                                   /*
                                                    * Fixed limits sized for small platforms (a few supervisor domains). They can
                                                    * later be derived from the device tree and allocated on the heap.
                                                    */
const MAX_TICKET: usize = 63; // tickets 1..=63 in flight; must fit in TICKET_BITS
const TICKET_SLOTS: usize = MAX_TICKET + 1; // includes unused slot 0
const _: () = assert!(MAX_TICKET <= TICKET_MASK);
/*
 * TEE stack layout, top down: the handlers' call stack (TEE_SCRATCH_SIZE), the
 * scratch context, then one saved Context per domain id (see init()).
 */
const TEE_STACK_SIZE: usize = 32 * 1024;
const _: () = assert!(
    TEE_STACK_SIZE >= TEE_SCRATCH_SIZE + (MAX_SUPERVISOR_DOMAINS + 1) * size_of::<Context>()
);
const MAX_ALLOCATIONS: usize = 64; // committed confidential allocations
const MAX_TVMS_PER_TSM: usize = 2; // TVM ids a TSM may hand out: 0..MAX_TVMS_PER_TSM
const MAX_HARTS: usize = 2;

#[link_section = ".rodata"]
static DICE_PLATFORM_PUBLIC_KEY: &[u8; 32] = include_bytes!("../keys/root_of_trust_pub.bin");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

#[no_mangle]
pub(crate) static mut TEE_STACK_TOP: usize = 0;

pub fn tee_stack_top() -> usize {
    unsafe { TEE_STACK_TOP }
}

#[derive(Clone)]
pub struct PhysicalMemory {
    pub base_address: usize,
    pub num_pages: usize,
    pub owner_id: usize,
    pub tsm_id: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BorrowKind {
    Convert,
    Reclaim,
}

/* A pending transaction: what is being borrowed, and how. */
pub struct Borrow {
    pub kind: BorrowKind,
    pub block: PhysicalMemory,
}

impl PhysicalMemory {
    fn belongs_to(&self, owner_id: usize, tsm_id: usize) -> bool {
        self.owner_id == owner_id && self.tsm_id == tsm_id
    }
    fn range(&self) -> (usize, usize) {
        (self.base_address, self.num_pages)
    }
}

#[derive(Clone, Copy)]
struct BootstrapGrant {
    owner_id: usize,
    tsm_id: usize,
}

#[repr(C, align(4096))]
pub struct State {
    tee_stack: [u8; TEE_STACK_SIZE],
    pub domains: Vec<Domain, MAX_SUPERVISOR_DOMAINS>,
    pub boot_domain_id: usize,
    pub attestation_context: PlatformAttestationContext,
    /* Pending trusted memory: base_address, num_pages, original owner */
    pending_memory_allocations: [Option<Borrow>; TICKET_SLOTS],
    next_ticket: usize,
    /* Real memory allocations */
    memory_allocations: Vec<PhysicalMemory, MAX_ALLOCATIONS>,
    bootstrap_grant: Option<BootstrapGrant>,
    /* TVM ownership: tvm_owner[tsm_id][tvm_id] = owner domain */
    tvm_owner: [[Option<u8>; MAX_TVMS_PER_TSM]; MAX_SUPERVISOR_DOMAINS],
    /* Keep track of long running calls */
    pub outstanding_calls: [Option<usize>; MAX_HARTS],
}

impl State {
    fn new(attestation_context: PlatformAttestationContext, boot_domain_id: usize) -> Self {
        Self {
            tee_stack: [0; TEE_STACK_SIZE],
            domains: Vec::new(),
            boot_domain_id,
            attestation_context,
            pending_memory_allocations: core::array::from_fn(|_| None),
            next_ticket: 1,
            memory_allocations: Vec::new(),
            bootstrap_grant: None,
            tvm_owner: [[None; MAX_TVMS_PER_TSM]; MAX_SUPERVISOR_DOMAINS],
            outstanding_calls: [None; MAX_HARTS],
        }
    }

    pub fn start_bootstrap(&mut self, owner_id: usize, tsm_id: usize) -> anyhow::Result<()> {
        if self.bootstrap_grant.is_some() {
            return Err(anyhow::anyhow!("a bootstrap grant is already active"));
        }

        self.bootstrap_grant = Some(BootstrapGrant { owner_id, tsm_id });
        Ok(())
    }

    pub fn finish_bootstrap(&mut self, owner_id: usize, tsm_id: usize) -> anyhow::Result<()> {
        match self.bootstrap_grant {
            Some(grant) if grant.owner_id == owner_id && grant.tsm_id == tsm_id => {
                self.bootstrap_grant = None;
                Ok(())
            }
            _ => Err(anyhow::anyhow!("invalid bootstrap grant completion")),
        }
    }

    pub fn bootstrap_owner_for(&self, tsm_id: usize) -> Option<usize> {
        match self.bootstrap_grant {
            Some(grant) if grant.tsm_id == tsm_id => Some(grant.owner_id),
            _ => None,
        }
    }

    pub fn validate_convert_range(
        &self,
        owner_id: usize,
        base_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        let size = num_pages
            .checked_mul(common::sbi::PAGE_SIZE)
            .ok_or_else(|| anyhow::anyhow!("page range overflow"))?;
        if size == 0 || !size.is_power_of_two() || base_address % size != 0 {
            return Err(anyhow::anyhow!(
                "range must be a non-zero, naturally aligned power of two"
            ));
        }
        let end = base_address
            .checked_add(size)
            .ok_or_else(|| anyhow::anyhow!("page range overflow"))?;
        if !self.domain_owns_ram_range(owner_id, base_address, size) {
            return Err(anyhow::anyhow!("range is outside caller RAM"));
        }

        let overlaps = |other_base: usize, other_pages: usize| {
            let other_end =
                other_base.saturating_add(other_pages.saturating_mul(common::sbi::PAGE_SIZE));
            base_address < other_end && other_base < end
        };
        if self
            .memory_allocations
            .iter()
            .any(|allocation| overlaps(allocation.base_address, allocation.num_pages))
            || self
                .pending_memory_allocations
                .iter()
                .flatten()
                .any(|pending| {
                    let (pending_base, pending_pages) = pending.block.range();
                    overlaps(pending_base, pending_pages)
                })
        {
            return Err(anyhow::anyhow!("range overlaps confidential memory"));
        }
        Ok(())
    }

    pub fn domain_owns_ram_range(&self, owner_id: usize, base_address: usize, size: usize) -> bool {
        let Some(end) = base_address.checked_add(size) else {
            return false;
        };
        let Some(domain) = self.domains.get(owner_id) else {
            return false;
        };
        if size == 0 {
            return false;
        }
        /*
         * Converting pages splits a domain's RAM into power-of-two regions, so a
         * buffer the domain owns may span several adjacent ones. Walk them: each
         * step must start inside a region and moves to that region's end.
         */
        let mut cursor = base_address;
        for _ in 0..=domain.memory_regions.len() {
            if cursor >= end {
                return true;
            }
            let next = domain.memory_regions.iter().find_map(|region| {
                if region.mmio || region.order >= usize::BITS {
                    return None;
                }
                let region_end = region.base_address.saturating_add(1usize << region.order);
                (cursor >= region.base_address && cursor < region_end).then_some(region_end)
            });
            match next {
                Some(region_end) => cursor = region_end,
                None => return false,
            }
        }
        cursor >= end
    }

    pub fn validate_reclaim_range(
        &self,
        owner_id: usize,
        tsm_id: usize,
        base_address: usize,
        num_pages: usize,
    ) -> anyhow::Result<()> {
        self.memory_allocations
            .iter()
            .any(|allocation| {
                allocation.base_address == base_address
                    && allocation.num_pages == num_pages
                    && allocation.owner_id == owner_id
                    && allocation.tsm_id == tsm_id
            })
            .then_some(())
            .ok_or_else(|| anyhow::anyhow!("no matching confidential allocation"))
    }

    /* True if [base, base + num_pages) lies inside one allocation that owner_id converted for tsm_id. */
    pub fn owns_confidential_range(
        &self,
        owner_id: usize,
        tsm_id: usize,
        base: usize,
        num_pages: usize,
    ) -> bool {
        let Some(end) = num_pages
            .checked_mul(common::sbi::PAGE_SIZE)
            .and_then(|size| base.checked_add(size))
        else {
            return false;
        };
        num_pages != 0
            && self.memory_allocations.iter().any(|a| {
                a.owner_id == owner_id
                    && a.tsm_id == tsm_id
                    && base >= a.base_address
                    && end <= a.base_address + a.num_pages * common::sbi::PAGE_SIZE
            })
    }

    /* The TVM id comes from the TSM, so bound both indices before use. */
    fn tvm_slot(&mut self, tsm_id: usize, tvm_id: usize) -> anyhow::Result<&mut Option<u8>> {
        self.tvm_owner
            .get_mut(tsm_id)
            .and_then(|tvms| tvms.get_mut(tvm_id))
            .ok_or_else(|| anyhow::anyhow!("bad tsm_id {} or tvm_id {}", tsm_id, tvm_id))
    }

    /* Record the creator of a TVM. A TSM must not hand out an id already in use. */
    pub fn assign_tvm(
        &mut self,
        tsm_id: usize,
        tvm_id: usize,
        owner_id: usize,
    ) -> anyhow::Result<()> {
        /* Domain ids index a MAX_SUPERVISOR_DOMAINS (64) table, so they fit in a u8. */
        let owner = u8::try_from(owner_id)?;
        let slot = self.tvm_slot(tsm_id, tvm_id)?;
        if let Some(owner) = *slot {
            anyhow::bail!(
                "tvm {} of tsm {} already owned by domain {}",
                tvm_id,
                tsm_id,
                owner
            );
        }
        *slot = Some(owner);
        Ok(())
    }

    /* Forget a destroyed TVM; only its owner can have destroyed it. */
    pub fn remove_tvm(
        &mut self,
        tsm_id: usize,
        tvm_id: usize,
        owner_id: usize,
    ) -> anyhow::Result<()> {
        let slot = self.tvm_slot(tsm_id, tvm_id)?;
        if *slot != u8::try_from(owner_id).ok() {
            anyhow::bail!(
                "tvm {} of tsm {} is not owned by domain {}",
                tvm_id,
                tsm_id,
                owner_id
            );
        }
        *slot = None;
        Ok(())
    }

    pub fn is_tvm_owned_by(&self, tsm_id: usize, tvm_id: usize, owner_id: usize) -> bool {
        self.tvm_owner
            .get(tsm_id)
            .and_then(|tvms| tvms.get(tvm_id))
            .is_some_and(|owner| owner.is_some_and(|owner| usize::from(owner) == owner_id))
    }
}

/* Impl for memory tracking system */
impl State {
    /* This does not start borrowing, but returns a ticket to the transaction */
    pub fn request_borrow(&mut self, req: Borrow) -> anyhow::Result<usize> {
        for _ in 0..MAX_TICKET {
            self.next_ticket = if self.next_ticket == MAX_TICKET {
                1
            } else {
                self.next_ticket + 1
            };

            if self.pending_memory_allocations[self.next_ticket].is_none() {
                self.pending_memory_allocations[self.next_ticket] = Some(req);
                return Ok(self.next_ticket);
            }
        }

        Err(anyhow::anyhow!("no ticket available for memory allocation"))
    }

    /*
     * Confirm a transaction only when it is returned by the same owner/TSM
     * pair that created it, and as the same kind of operation. The caller
     * identities come from the active OpenSBI domains, not TSM-controlled
     * registers. Every check runs before any state changes.
     */
    pub fn take_borrow(
        &mut self,
        ticket: usize,
        owner_id: usize,
        tsm_id: usize,
        kind: BorrowKind,
    ) -> anyhow::Result<PhysicalMemory> {
        if !(1..=MAX_TICKET).contains(&ticket) {
            return Err(anyhow::anyhow!("invalid ticket"));
        }
        let borrow = self.pending_memory_allocations[ticket]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("invalid ticket"))?;

        if !borrow.block.belongs_to(owner_id, tsm_id) {
            return Err(anyhow::anyhow!(
                "borrow returned by an unexpected domain pair"
            ));
        }
        if borrow.kind != kind {
            return Err(anyhow::anyhow!("borrow returned by a different operation"));
        }

        let block = &borrow.block;
        let allocation = match borrow.kind {
            BorrowKind::Convert => {
                self.memory_allocations
                    .push(block.clone())
                    .map_err(|_| anyhow::anyhow!("cannot confirm borrow"))?;
                block.clone()
            }
            BorrowKind::Reclaim => {
                let idx = self
                    .memory_allocations
                    .iter()
                    .position(|allocation| {
                        allocation.base_address == block.base_address
                            && allocation.num_pages == block.num_pages
                            && allocation.owner_id == block.owner_id
                            && allocation.tsm_id == block.tsm_id
                    })
                    .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;
                self.memory_allocations.remove(idx)
            }
        };
        self.pending_memory_allocations[ticket] = None;
        Ok(allocation)
    }

    /* Delete a transaction only for its original owner/TSM pair. */
    pub fn cancel_borrow(
        &mut self,
        ticket: usize,
        owner_id: usize,
        tsm_id: usize,
    ) -> anyhow::Result<()> {
        if !(1..=MAX_TICKET).contains(&ticket) {
            return Err(anyhow::anyhow!("invalid ticket"));
        }
        let borrow = self.pending_memory_allocations[ticket]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("invalid ticket"))?;
        if !borrow.block.belongs_to(owner_id, tsm_id) {
            return Err(anyhow::anyhow!(
                "borrow returned by an unexpected domain pair"
            ));
        }
        self.pending_memory_allocations[ticket] = None;

        Ok(())
    }
}

/// This function initializes the TSM-driver:
/// - read DICE input parameters, compute the new security context and create TSM CDI_ID and
/// certificate
/// - initialize the TEE stack
/// - create every supervisor domain declared by the platform device tree;
/// - load a built-in or externally staged TSM in domains marked with `shadowfax,tsm`.
/// Assumption: the domain id matches with its position in the domain array
pub fn init(fdt_addr: usize, boot_hartid: usize) -> Result<usize, anyhow::Error> {
    let platform = PlatformConfig::from_addr(fdt_addr, boot_hartid)?;
    if let Some((image, signature)) = platform.default_tsm {
        initialize_default_tsm(image, signature);
    }

    // First, get the security context
    let attestation_context = PlatformAttestationContext::init_from_addr(platform.dice_input_addr);
    // Verify the signature
    attestation_context.verify_with_pubkey(DICE_PLATFORM_PUBLIC_KEY)?;

    // Lock the state and init the data structure
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new(attestation_context, platform.boot_domain_id));

    let tee_stack = state.tee_stack.as_ptr_range().end as usize;
    unsafe {
        TEE_STACK_TOP = tee_stack;
    }

    // Create the root domain. The root domain id is always zero, so it has to be the first
    let root_domain = Domain {
        name: String::from("root"),
        memory_regions: Vec::from_slice(&[MemoryRegion {
            base_address: 0,
            order: usize::BITS,
            mmio: false,
            permissions: 0x3f,
        }])
        .map_err(|_| anyhow::anyhow!("cannot create root memory domain region"))?,
        // The root domain should not be involved in Confidential call
        trust_map: 0,
        next_addr: 0,
        context_addr: 0,
        has_tsm: false,
        boot_hart: false,
    };
    state
        .domains
        .push(root_domain)
        .map_err(|_| anyhow::anyhow!("too many domains"))?;

    let base_context = tee_stack - (TEE_SCRATCH_SIZE + size_of::<Context>());
    for config in platform.domains {
        let context_addr = base_context - config.id * size_of::<Context>();
        let domain = if !matches!(config.tsm_source, crate::platform::TsmSource::None) {
            create_confidential_domain(config, context_addr, &state.attestation_context)?
        } else {
            domain_from_config(config, context_addr)
        };
        state
            .domains
            .push(domain)
            .map_err(|_| anyhow::anyhow!("too many domains"))?;
    }

    dump_domains(state, platform.dice_input_addr, fdt_addr);

    Ok(state.domains[state.boot_domain_id].next_addr)
}

fn domain_from_config(config: DomainConfig, context_addr: usize) -> Domain {
    Domain {
        name: config.name,
        trust_map: config.trust_map,
        memory_regions: config.memory_regions,
        next_addr: config.next_addr,
        context_addr,
        has_tsm: false,
        boot_hart: config.boot_hart.is_some(),
    }
}

fn dump_domains(state: &State, dice_input_addr: usize, fdt_addr: usize) {
    print_raw!("{:<40} : {:#010x}\n", "DICE Input Address", dice_input_addr);
    print_raw!("{:<40} : {:#010x}\n", "Boot FDT Address", fdt_addr);
    print_raw!("Supervisor domains (DT order)\n");

    for (id, domain) in state.domains.iter().enumerate() {
        print_raw!(
            "  Domain {}: {}{}{}\n",
            id,
            domain.name,
            if domain.has_tsm {
                " [trusted, load TSM]"
            } else {
                ""
            },
            if domain.boot_hart { " [boot]" } else { "" }
        );
        print_raw!(
            "    entry={:#x}, context={:#x}\n",
            domain.next_addr,
            domain.context_addr
        );

        if domain.has_tsm {
            print_raw!("    trusts=[");
            let mut separator = "";
            for trusted_id in 0..state.domains.len() {
                if domain.is_trusted(trusted_id) {
                    print_raw!(
                        "{}{}:{}",
                        separator,
                        trusted_id,
                        state.domains[trusted_id].name
                    );
                    separator = ", ";
                }
            }
            print_raw!("]\n");
        }

        for (region_id, region) in domain.memory_regions.iter().enumerate() {
            if region.order == usize::BITS {
                print_raw!(
                    "    region {}: all memory, P:{:#04x}\n",
                    region_id,
                    region.permissions
                );
                continue;
            }
            let end = region.base_address + (1usize << region.order);
            print_raw!(
                "    region {}: {:#x}-{:#x} {}, P:{:#04x}\n",
                region_id,
                region.base_address,
                end,
                if region.mmio { "MMIO" } else { "RAM" },
                region.permissions
            );
        }
    }
}
