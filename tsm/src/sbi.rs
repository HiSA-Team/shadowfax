use common::{
    attestation::DiceLayer,
    sbi::{SbiRet, COVG_GET_EVIDENCE, PAGE_SIZE},
};

use crate::{println, STATE};

pub fn handle_covg(_eid: usize, fid: usize, args: &[usize; 6]) -> SbiRet {
    match fid {
        COVG_GET_EVIDENCE => {
            println!("[OLORIN] requested attestation certificate");
            handle_covg_get_evidence(args[0], args[1], args[2], args[3], args[4], args[5])
        }
        _ => SbiRet { a0: -1, a1: 0 },
    }
}

fn handle_covg_get_evidence(
    pub_key_addr: usize,
    _pub_key_size: usize,
    challenge_addr: usize,
    _cert_format: usize,
    cert_addr_out: usize,
    cert_size: usize,
) -> SbiRet {
    let mut lock = STATE.lock();
    if let Some(tsm) = lock.as_mut() {
        let attestation_context = tsm.attestation_context.clone();
        if let Some(tvm) = &tsm.hypervisor.tvm {
            // 1. Validate pointers (Simplified: Check alignment)
            if pub_key_addr % PAGE_SIZE != 0
                || challenge_addr % PAGE_SIZE != 0
                || cert_addr_out % PAGE_SIZE != 0
            {
                return SbiRet { a0: -1, a1: 0 }; // SBI_ERR_INVALID_PARAM
            }

            // 2. Read Challenge from Guest Memory
            // Safety: We assume the guest has mapped this validly.
            // In a real TSM, verify 'challenge_addr' is in the TVM's confidential region.
            let mut challenge = [0u8; 64]; // Assuming 64-byte Nonce
            unsafe {
                let src = challenge_addr as *const u8;
                core::ptr::copy_nonoverlapping(src, challenge.as_mut_ptr(), 64);
            }

            // 3. Generate Evidence
            // The measurement is stored in tvm.measure
            // According to CoVE, the TVM context is created when it requests the evidence.
            let tvm_measure = tvm.get_measure();
            let tvm_att_context = attestation_context.compute_next(&tvm_measure);
            let evidence = tvm_att_context.get_evidence(&tvm_measure, &challenge);

            // 4. Serialize
            let encoded_evidence = match evidence.to_bytes() {
                Ok(b) => b,
                Err(_) => return SbiRet { a0: -1, a1: 0 },
            };

            if encoded_evidence.len() > cert_size {
                // Buffer too small, return required size in value
                return SbiRet { a0: -1, a1: 0 };
            }

            // 5. Write to Guest Memory
            unsafe {
                let dst = cert_addr_out as *mut u8;
                core::ptr::copy_nonoverlapping(
                    encoded_evidence.as_ptr(),
                    dst,
                    encoded_evidence.len(),
                );
            }

            return SbiRet {
                a0: 0,
                a1: encoded_evidence.len() as isize,
            };
        }
    }
    SbiRet { a0: -1, a1: 0 }
}
