#![no_std]
#![no_main]

use cove::{SbiRet, TsmInfo};
use heapless::Vec;
use spin::mutex::SpinMutex;
mod cove;
mod log;

// make sure the panic handler is linked in
extern crate panic_halt;

const COVEH_EXT_ID: u64 = 0x434F5648;
const SBI_EXT_COVE_HOST_GET_TSM_INFO: u64 = 0x00;

const SUPD_EXT_ID: u64 = 0x53555044;
const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: u64 = 0x00;

static DOMAINS: SpinMutex<Vec<TsmInfo, 64>> = SpinMutex::new(Vec::new());

fn sbi_call(extid: usize, fid: usize, args: &[u64; 5]) -> SbiRet {
    let (error, value);
    unsafe {
        core::arch::asm!(
            "ecall",
            in("a7") extid,
            in("a6") fid,
            inlateout("a0") args[0] => error,
            inlateout("a1") args[1] => value,
            in("a2") args[2],
            in("a3") args[3],
            in("a4") args[4],
        );
    }
    SbiRet { error, value }
}
macro_rules! cove_pack_fid {
    ($sdid:expr, $fid:expr) => {
        (($sdid & 0x3F) << 26) | ($fid & 0xFFFF)
    };
}

#[riscv_rt::entry]
fn main() -> ! {
    let mut domains = DOMAINS.lock();
    println!("[SHADOWFAX-HYPERVISOR] enumerating supervisor domains");
    // get all active_domains
    let active_domains = sbi_call(
        SUPD_EXT_ID as usize,
        SBI_EXT_SUPD_GET_ACTIVE_DOMAINS as usize,
        &[0, 0, 0, 0, 0],
    );

    // register active domains in our structure
    let domain_mask = active_domains.value;
    for i in 0..64 {
        if ((domain_mask >> i) & 0x01) == 1 {
            domains
                .push(TsmInfo {
                    tsm_state: cove::TsmState::TsmLoaded,
                    tsm_impl_id: 0,
                    tsm_version: 0,
                    tsm_capabilities: 0,
                    tvm_state_pages: 0,
                    tvm_max_vcpus: 0,
                    tvm_vcpu_state_pages: 0,
                })
                .unwrap();
        }
    }

    assert_eq!(domains.len(), 2);
    println!(
        "[SHADOWFAX-HYPERVISOR] found one supervisor domain with ID {}",
        1
    );

    let domain = &domains[1];
    let fid = cove_pack_fid!(1, SBI_EXT_COVE_HOST_GET_TSM_INFO as usize);
    let mut sbi_args = [0, 0, 0, 0, 0];
    sbi_args[0] = &raw const domain as u64;
    sbi_args[1] = size_of::<TsmInfo>() as u64;

    sbi_call(COVEH_EXT_ID as usize, fid, &sbi_args);

    println!(
        "[SHADOWFAX-HYPERVISOR] TSM has impl id: {}",
        domain.tsm_impl_id
    );

    loop {
        riscv::asm::wfi();
    }
}
