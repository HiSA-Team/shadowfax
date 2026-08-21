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
//!         order = <26>;
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
    domain::{create_confidential_domain, Domain, MemoryRegion},
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
const MAX_TICKET: usize = TICKET_MASK; // 1023
const TICKET_SLOTS: usize = MAX_TICKET + 1; // includes unused slot 0

#[link_section = ".rodata"]
static DICE_PLATFORM_PUBLIC_KEY: &[u8; 32] = include_bytes!("../keys/root_of_trust_pub.bin");

pub static STATE: Mutex<OnceCell<State>> = Mutex::new(OnceCell::new());

pub enum BorrowKind {
    Convert {
        base_address: usize,
        num_pages: usize,
        owner_id: usize,
        tsm_id: usize,
    },
    Reclaim {
        base_address: usize,
        num_pages: usize,
        owner_id: usize,
        tsm_id: usize,
    },
}

impl BorrowKind {
    pub fn new_alloc(
        owner_id: usize,
        tsm_id: usize,
        base_address: usize,
        num_pages: usize,
    ) -> Self {
        Self::Convert {
            base_address,
            num_pages,
            owner_id,
            tsm_id,
        }
    }
    pub fn new_reclaim(
        owner_id: usize,
        tsm_id: usize,
        base_address: usize,
        num_pages: usize,
    ) -> Self {
        Self::Reclaim {
            base_address,
            num_pages,
            owner_id,
            tsm_id,
        }
    }

    fn belongs_to(&self, owner_id: usize, tsm_id: usize) -> bool {
        match self {
            Self::Convert {
                owner_id: expected_owner_id,
                tsm_id: expected_tsm_id,
                ..
            }
            | Self::Reclaim {
                owner_id: expected_owner_id,
                tsm_id: expected_tsm_id,
                ..
            } => owner_id == *expected_owner_id && tsm_id == *expected_tsm_id,
        }
    }
}

#[derive(Clone)]
pub struct Allocation {
    pub base_address: usize,
    pub num_pages: usize,
    pub owner_id: usize,
    pub tsm_id: usize,
}

#[derive(Clone, Copy)]
struct BootstrapGrant {
    owner_id: usize,
    tsm_id: usize,
}

pub struct State {
    pub domains: Vec<Domain, MAX_SUPERVISOR_DOMAINS>,
    pub boot_domain_id: usize,
    pub attestation_context: PlatformAttestationContext,
    /* Pending trusted memory: base_address, num_pages, original owner */
    pending_memory_allocations: [Option<BorrowKind>; TICKET_SLOTS],
    next_ticket: usize,
    /* Real memory allocations */
    memory_allocations: Vec<Allocation, MAX_TICKET>,
    bootstrap_grant: Option<BootstrapGrant>,
}

impl State {
    fn new(attestation_context: PlatformAttestationContext, boot_domain_id: usize) -> Self {
        Self {
            domains: Vec::new(),
            boot_domain_id,
            attestation_context,
            pending_memory_allocations: core::array::from_fn(|_| None),
            next_ticket: 1,
            memory_allocations: Vec::new(),
            bootstrap_grant: None,
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

    /* This does not start borrowing, but returns a ticket to the transaction */
    pub fn request_borrow(&mut self, req: BorrowKind) -> anyhow::Result<usize> {
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
     * pair that created it. The caller identities come from the active
     * OpenSBI domains, not TSM-controlled registers.
     */
    pub fn take_borrow(
        &mut self,
        ticket: usize,
        owner_id: usize,
        tsm_id: usize,
    ) -> anyhow::Result<Allocation> {
        if !(1..=MAX_TICKET).contains(&ticket) {
            return Err(anyhow::anyhow!("invalid ticket"));
        }

        let borrow = self.pending_memory_allocations[ticket]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("invalid ticket"))?;
        let (base_address, num_pages) = match borrow {
            BorrowKind::Convert {
                base_address,
                num_pages,
                owner_id: _,
                tsm_id: _,
            }
            | BorrowKind::Reclaim {
                base_address,
                num_pages,
                owner_id: _,
                tsm_id: _,
            } => (*base_address, *num_pages),
        };

        if !borrow.belongs_to(owner_id, tsm_id) {
            return Err(anyhow::anyhow!(
                "borrow returned by an unexpected domain pair"
            ));
        }

        match borrow {
            BorrowKind::Convert { .. } => {
                if self.memory_allocations.len() == MAX_TICKET {
                    return Err(anyhow::anyhow!("cannot confirm borrow"));
                }

                let allocation = Allocation {
                    base_address,
                    num_pages,
                    owner_id,
                    tsm_id,
                };
                self.memory_allocations
                    .push(allocation.clone())
                    .map_err(|_| anyhow::anyhow!("cannot confirm borrow"))?;
                self.pending_memory_allocations[ticket] = None;
                Ok(allocation)
            }
            BorrowKind::Reclaim { .. } => {
                let idx = self
                    .memory_allocations
                    .iter()
                    .enumerate()
                    .position(|(_, allocation)| {
                        allocation.base_address == base_address
                            && allocation.num_pages == num_pages
                            && allocation.owner_id == owner_id
                            && allocation.tsm_id == tsm_id
                    })
                    .ok_or_else(|| anyhow::anyhow!("No matching memory block"))?;

                let allocation = self.memory_allocations.remove(idx);
                self.pending_memory_allocations[ticket] = None;
                Ok(allocation)
            }
        }
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
        if !borrow.belongs_to(owner_id, tsm_id) {
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
pub fn init(fdt_addr: usize) -> Result<usize, anyhow::Error> {
    let platform = PlatformConfig::from_addr(fdt_addr)?;

    // First, get the security context
    let attestation_context = PlatformAttestationContext::init_from_addr(platform.dice_input_addr);
    // Verify the signature
    attestation_context.verify_with_pubkey(DICE_PLATFORM_PUBLIC_KEY)?;

    // Lock the state and init the data structure
    let mut state = STATE.lock();
    let state = state.get_mut_or_init(|| State::new(attestation_context, platform.boot_domain_id));

    let tee_stack = &raw const crate::_tee_stack_top as *const u8 as usize;

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
        boot_hart: config.boot_hart,
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
