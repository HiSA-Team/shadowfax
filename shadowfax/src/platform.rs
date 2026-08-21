//! Runtime platform configuration parsed from the boot-provided FDT with libfdt.

use alloc::{boxed::Box, format, string::String};
use core::slice;
use heapless::Vec;

use anyhow::{anyhow, bail, Context as _};
use libfdt_rs::{Fdt, FdtNode, Phandle, PropertyCellParser, PropertyReader};

use crate::domain::MemoryRegion;

const FDT_MAGIC: u32 = 0xd00d_feed;
const FDT_HEADER_SIZE: usize = 40;

pub const MAX_MEMORY_REGIONS: usize = 16;
pub const MAX_SUPERVISOR_DOMAINS: usize = 64;
pub const MAX_RESERVED_REGIONS: usize = 16;

#[derive(Clone, Copy)]
pub struct AddressRange {
    pub base: usize,
    pub size: usize,
}

#[derive(Clone, Copy)]
pub enum TsmSource {
    None,
    Builtin,
    External {
        image: AddressRange,
        signature: AddressRange,
    },
}

#[derive(Clone)]
pub struct DomainConfig {
    pub id: usize,
    pub name: String,
    pub memory_regions: Vec<MemoryRegion, MAX_MEMORY_REGIONS>,
    pub trust_map: usize,
    pub next_addr: usize,
    pub tsm_source: TsmSource,
    pub boot_hart: bool,
}

pub struct PlatformConfig {
    pub dice_input_addr: usize,
    pub boot_domain_id: usize,
    pub domains: Vec<DomainConfig, MAX_SUPERVISOR_DOMAINS>,
}

impl PlatformConfig {
    pub fn from_addr(fdt_addr: usize) -> anyhow::Result<Self> {
        let fdt = copy_fdt(fdt_addr)?;
        let shadowfax = fdt
            .get_node("/chosen/shadowfax")
            .map_err(|error| anyhow!("missing /chosen/shadowfax: {error:?}"))?;
        ensure_compatible(&fdt, &shadowfax, "shadowfax,platform-config")?;

        let dice_input_addr = read_u64_property(&shadowfax, "dice-input")? as usize;
        let reserved_regions = read_reserved_regions(&fdt)?;

        let domain_root = fdt
            .get_node("/chosen/opensbi-domains")
            .map_err(|error| anyhow!("missing OpenSBI domain configuration: {error:?}"))?;
        ensure_compatible(&fdt, &domain_root, "opensbi,domain,config")?;

        // OpenSBI assigns IDs by iterating instance subnodes in DT order.
        // Keep the nodes until all trust phandles have been resolved.
        let mut parsed: Vec<(FdtNode<'_>, DomainConfig), MAX_SUPERVISOR_DOMAINS> = Vec::new();
        for node in domain_root
            .subnodes_iter()
            .map_err(|error| anyhow!("cannot iterate OpenSBI domains: {error:?}"))?
        {
            if !fdt
                .is_compatible(&node, "opensbi,domain,instance")
                .unwrap_or(false)
            {
                continue;
            }

            let id = parsed.len() + 1; // OpenSBI reserves domain zero for root.
            parsed
                .push((
                    node.clone(),
                    DomainConfig {
                        id,
                        name: String::from(node.name()),
                        memory_regions: read_regions(&fdt, &node)?,
                        trust_map: 0,
                        next_addr: read_u64_property(&node, "next-addr")? as usize,
                        tsm_source: read_tsm_source(&node)?,
                        boot_hart: node.get_property("boot-hart").is_ok(),
                    },
                ))
                .map_err(|_| anyhow::anyhow!("max supervisor domain exceeded"))?;
        }

        if parsed.is_empty() {
            bail!("the OpenSBI domain configuration contains no domains");
        }

        let mut boot_domain_id = None;
        for index in 0..parsed.len() {
            parsed[index].1.trust_map = read_trust_map(&fdt, &parsed[index].0, &parsed)?;
            if parsed[index].1.boot_hart && boot_domain_id.replace(parsed[index].1.id).is_some() {
                bail!("more than one domain declares boot-hart");
            }
        }

        let boot_domain_id = boot_domain_id.context("no domain declares boot-hart")?;
        for (_, config) in &parsed {
            if let TsmSource::External { image, signature } = config.tsm_source {
                for range in [image, signature] {
                    if !reserved_regions
                        .iter()
                        .any(|reserved| contains(*reserved, range))
                    {
                        bail!("domain {} TSM staging range is not reserved", config.name);
                    }
                    for (_, other) in &parsed {
                        if other.memory_regions.iter().any(|region| {
                            let size = 1usize.checked_shl(region.order).unwrap_or(0);
                            overlaps(
                                range,
                                AddressRange {
                                    base: region.base_address,
                                    size,
                                },
                            )
                        }) {
                            bail!(
                                "domain {} TSM staging overlaps supervisor memory",
                                config.name
                            );
                        }
                    }
                }
            }
        }
        let domains = parsed.into_iter().map(|(_, config)| config).collect();
        Ok(Self {
            dice_input_addr,
            boot_domain_id,
            domains,
        })
    }
}

fn read_tsm_source(node: &FdtNode<'_>) -> anyhow::Result<TsmSource> {
    let is_tsm = node.get_property("shadowfax,tsm").is_ok();
    let builtin = node.get_property("shadowfax,load-tsm").is_ok();
    let image = read_optional_range_property(node, "shadowfax,tsm-image")?;
    let signature = read_optional_range_property(node, "shadowfax,tsm-signature")?;

    if builtin && (image.is_some() || signature.is_some()) {
        bail!("domain {} selects multiple TSM sources", node.name());
    }
    if image.is_some() != signature.is_some() {
        bail!(
            "domain {} has an incomplete external TSM source",
            node.name()
        );
    }
    if !is_tsm && image.is_some() {
        bail!(
            "domain {} supplies a TSM image without shadowfax,tsm",
            node.name()
        );
    }
    if builtin {
        return Ok(TsmSource::Builtin);
    }
    if let (Some(image), Some(signature)) = (image, signature) {
        if image.size == 0 || signature.size != 64 {
            bail!("domain {} has invalid external TSM sizes", node.name());
        }
        return Ok(TsmSource::External { image, signature });
    }
    if is_tsm {
        bail!("domain {} has no TSM source", node.name());
    }
    Ok(TsmSource::None)
}

fn read_reserved_regions(fdt: &Fdt) -> anyhow::Result<Vec<AddressRange, MAX_RESERVED_REGIONS>> {
    let root = fdt
        .get_node("/reserved-memory")
        .map_err(|error| anyhow!("missing /reserved-memory: {error:?}"))?;
    let mut ranges = Vec::new();
    for node in root
        .subnodes_iter()
        .map_err(|error| anyhow!("cannot iterate reserved memory: {error:?}"))?
    {
        if node.get_property("no-map").is_err() {
            continue;
        }
        if let Some(range) = read_optional_range_property(&node, "reg")? {
            ranges
                .push(range)
                .map_err(|_| anyhow!("too many reserved-memory ranges"))?;
        }
    }
    Ok(ranges)
}

fn read_optional_range_property(
    node: &FdtNode<'_>,
    name: &str,
) -> anyhow::Result<Option<AddressRange>> {
    let property = match node.get_property(name) {
        Ok(property) => property,
        Err(libfdt_rs::Error::NotFound) => return Ok(None),
        Err(error) => bail!("cannot read {}/{name}: {error:?}", node.name()),
    };
    let mut reader = PropertyReader::from(&property);
    let mut read_u64 = || -> anyhow::Result<u64> {
        let high = unsafe { reader.read::<PropertyCellParser>() }
            .with_context(|| format!("{}/{name} has a truncated value", node.name()))?;
        let low = unsafe { reader.read::<PropertyCellParser>() }
            .with_context(|| format!("{}/{name} has a truncated value", node.name()))?;
        Ok(((high as u64) << 32) | low as u64)
    };
    let base = read_u64()?;
    let size = read_u64()?;
    if base > usize::MAX as u64 || size > usize::MAX as u64 {
        bail!(
            "{}/{name} does not fit the platform address size",
            node.name()
        );
    }
    Ok(Some(AddressRange {
        base: base as usize,
        size: size as usize,
    }))
}

fn contains(outer: AddressRange, inner: AddressRange) -> bool {
    match (
        outer.base.checked_add(outer.size),
        inner.base.checked_add(inner.size),
    ) {
        (Some(outer_end), Some(inner_end)) => inner.base >= outer.base && inner_end <= outer_end,
        _ => false,
    }
}

fn overlaps(left: AddressRange, right: AddressRange) -> bool {
    match (
        left.base.checked_add(left.size),
        right.base.checked_add(right.size),
    ) {
        (Some(left_end), Some(right_end)) => left.base < right_end && right.base < left_end,
        _ => true,
    }
}

pub fn relocate_fdt(fdt_addr: usize) -> anyhow::Result<usize> {
    let fdt = copy_fdt(fdt_addr)?;
    let shadowfax = fdt
        .get_node("/chosen/shadowfax")
        .map_err(|error| anyhow!("missing /chosen/shadowfax: {error:?}"))?;
    ensure_compatible(&fdt, &shadowfax, "shadowfax,platform-config")?;
    let destination = read_u64_property(&shadowfax, "host-fdt")? as usize;
    if destination == fdt_addr {
        return Ok(fdt_addr);
    }

    let size = fdt_total_size(fdt_addr)?;
    unsafe {
        core::ptr::copy(fdt_addr as *const u8, destination as *mut u8, size);
    }
    Ok(destination)
}

fn fdt_total_size(fdt_addr: usize) -> anyhow::Result<usize> {
    let total_size =
        u32::from_be(unsafe { ((fdt_addr + size_of::<u32>()) as *const u32).read_unaligned() })
            as usize;
    if total_size < FDT_HEADER_SIZE {
        bail!("invalid FDT size {total_size}");
    }
    Ok(total_size)
}

fn copy_fdt(fdt_addr: usize) -> anyhow::Result<Fdt> {
    let magic = u32::from_be(unsafe { (fdt_addr as *const u32).read_unaligned() });
    if magic != FDT_MAGIC {
        bail!("invalid FDT magic {magic:#x} at {fdt_addr:#x}");
    }

    let total_size = fdt_total_size(fdt_addr)?;

    let bytes = unsafe { slice::from_raw_parts(fdt_addr as *const u8, total_size) };
    let owned: Box<[u8]> = bytes.to_vec().into_boxed_slice();
    Fdt::new(owned).map_err(|error| anyhow!("cannot parse platform DTB: {error:?}"))
}

fn ensure_compatible(fdt: &Fdt, node: &FdtNode<'_>, compatible: &str) -> anyhow::Result<()> {
    if !fdt
        .is_compatible(node, compatible)
        .map_err(|error| anyhow!("cannot inspect {} compatible: {error:?}", node.name()))?
    {
        bail!("{} is not compatible with {compatible}", node.name());
    }
    Ok(())
}

fn read_u64_property(node: &FdtNode<'_>, name: &str) -> anyhow::Result<u64> {
    let property = node
        .get_property(name)
        .map_err(|error| anyhow!("missing {} property {name}: {error:?}", node.name()))?;
    let mut reader = PropertyReader::from(&property);
    let high = unsafe { reader.read::<PropertyCellParser>() }
        .with_context(|| format!("{}/{name} has no high cell", node.name()))?;
    let low = unsafe { reader.read::<PropertyCellParser>() }
        .with_context(|| format!("{}/{name} has no low cell", node.name()))?;
    Ok(((high as u64) << 32) | low as u64)
}

fn read_u32_property(node: &FdtNode<'_>, name: &str) -> anyhow::Result<u32> {
    let property = node
        .get_property(name)
        .map_err(|error| anyhow!("missing {} property {name}: {error:?}", node.name()))?;
    let mut reader = PropertyReader::from(&property);
    unsafe { reader.read::<PropertyCellParser>() }
        .with_context(|| format!("{}/{name} has no value", node.name()))
}

fn read_regions(
    fdt: &Fdt,
    domain: &FdtNode<'_>,
) -> anyhow::Result<Vec<MemoryRegion, MAX_MEMORY_REGIONS>> {
    let property = domain
        .get_property("regions")
        .map_err(|error| anyhow!("domain {} has no regions: {error:?}", domain.name()))?;
    let mut reader = PropertyReader::from(&property);
    let mut regions = Vec::new();

    while let Some(raw_phandle) = unsafe { reader.read::<PropertyCellParser>() } {
        let permissions = unsafe { reader.read::<PropertyCellParser>() }
            .with_context(|| format!("domain {} has a truncated regions entry", domain.name()))?
            as u8;
        let phandle = Phandle::try_from(raw_phandle)
            .map_err(|error| anyhow!("invalid region phandle {raw_phandle:#x}: {error:?}"))?;
        let region = fdt
            .get_node_by_phandle(&phandle)
            .map_err(|error| anyhow!("unknown region phandle {raw_phandle:#x}: {error:?}"))?;
        ensure_compatible(fdt, &region, "opensbi,domain,memregion")?;

        let order = read_u32_property(&region, "order")?;
        if order >= usize::BITS {
            bail!(
                "memory region {} has unsupported order {order}",
                region.name()
            );
        }
        regions
            .push(MemoryRegion {
                base_address: read_u64_property(&region, "base")? as usize,
                order,
                mmio: region.get_property("mmio").is_ok(),
                permissions: permissions & 0x3f,
            })
            .map_err(|_| anyhow::anyhow!("cannot push memory region"))?;
    }

    if regions.is_empty() {
        bail!("domain {} contains no memory regions", domain.name());
    }
    Ok(regions)
}

fn read_trust_map(
    fdt: &Fdt,
    domain: &FdtNode<'_>,
    domains: &[(FdtNode<'_>, DomainConfig)],
) -> anyhow::Result<usize> {
    let property = match domain.get_property("shadowfax,trusts") {
        Ok(property) => property,
        Err(libfdt_rs::Error::NotFound) => return Ok(0),
        Err(error) => bail!("cannot read trust map for {}: {error:?}", domain.name()),
    };

    let mut reader = PropertyReader::from(&property);
    let mut trust_map = 0usize;
    while let Some(raw_phandle) = unsafe { reader.read::<PropertyCellParser>() } {
        let phandle = Phandle::try_from(raw_phandle)
            .map_err(|error| anyhow!("invalid trust phandle {raw_phandle:#x}: {error:?}"))?;
        let trusted = fdt
            .get_node_by_phandle(&phandle)
            .map_err(|error| anyhow!("unknown trust phandle {raw_phandle:#x}: {error:?}"))?;
        let target = domains
            .iter()
            .position(|(node, _)| *node == trusted)
            .with_context(|| format!("{} trusts a non-domain node", domain.name()))?;
        trust_map |= 1usize << domains[target].1.id;
    }
    Ok(trust_map)
}
