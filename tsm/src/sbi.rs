use common::{
    attestation::DiceLayer,
    sbi::{SbiRet, COVG_GET_EVIDENCE},
};

use crate::{
    hyper::{read_guest_memory, write_guest_memory},
    println, ATTESTATION_CONTEXT, MEASUREMENT,
};

pub fn handle_covg(_eid: usize, fid: usize, args: &[usize; 6]) -> SbiRet {
    match fid {
        COVG_GET_EVIDENCE => {
            println!("[OLORIN] Requested attestation certificate");
            handle_covg_get_evidence(args[0], args[1], args[2], args[3], args[4], args[5])
        }
        _ => SbiRet { a0: -1, a1: 0 },
    }
}

fn handle_covg_get_evidence(
    _pub_key_addr: usize,
    _pub_key_size: usize,
    challenge_addr: usize,
    _cert_format: usize,
    cert_addr_out: usize,
    cert_size: usize,
) -> SbiRet {
    // A. SETUP: Get Page Table
    let hgatp_val = crate::h_extension::csrs::hgatp::read().bits();
    let root_pt = ((hgatp_val & 0xFF_FFFF_FFFF_F) << 12) as usize;

    // B. INPUT: Read Challenge from Guest
    let mut challenge = [0u8; 64];
    if read_guest_memory(root_pt, challenge_addr, &mut challenge).is_err() {
        return SbiRet { a0: -1, a1: 0 }; // Fault or Boundary Error
    }

    // C. LOGIC: Generate Evidence (Holds Locks)
    let encoded_evidence = {
        // We assume Measurement is also available here or passed in
        // For this example, let's say it's in TSM or separate lock
        let measure_lock = MEASUREMENT.lock();
        let measurement = match measure_lock.as_ref() {
            Some(m) => m,
            None => return SbiRet { a0: -1, a1: 0 },
        };

        let attetstation_lock = ATTESTATION_CONTEXT.lock();
        let tvm_attestation_ctx = match attetstation_lock.as_ref() {
            Some(att) => att,
            None => return SbiRet { a0: -1, a1: 0 },
        }
        .compute_next(measurement);
        let evidence = tvm_attestation_ctx.get_evidence(&measurement, &challenge);
        match evidence.to_bytes() {
            Ok(e) => e,
            Err(e) => {
                println!("[OLORIN] Error during evidence encoding {}", e);
                return SbiRet { a0: -1, a1: 0 };
            }
        }
    };
    // D. VALIDATION: Check Size
    if encoded_evidence.len() > cert_size {
        return SbiRet { a0: -1, a1: 0 }; // Buffer too small
    }

    if write_guest_memory(root_pt, cert_addr_out, &encoded_evidence).is_err() {
        return SbiRet { a0: -1, a1: 0 };
    }

    // Success
    SbiRet {
        a0: 0,
        a1: encoded_evidence.len() as isize,
    }
}
