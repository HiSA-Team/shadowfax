#![no_std]
#![no_main]

pub mod sbi {
    pub const COVH_DEFAULT_PAGE_SIZE: usize = 4096;
    // CoVH constants
    pub const SBI_COVH_EXT_ID: usize = 0x434F5648;

    pub const SBI_COVH_GET_TSM_INFO: usize = 0;
    pub const SBI_COVH_CONVERT_PAGES: usize = 1;
    pub const SBI_COVH_CREATE_TVM: usize = 5;
    pub const SBI_COVH_FINALIZE_TVM: usize = 6;
    pub const SBI_COVH_DESTROY_TVM: usize = 8;
    pub const SBI_COVH_ADD_TVM_MEMORY_REGION: usize = 9;
    pub const SBI_COVH_ADD_TVM_MEASURED_PAGES: usize = 11;
    pub const SBI_COVH_CREATE_TVM_VCPU: usize = 14;
    pub const SBI_COVH_RUN_TVM_VCPU: usize = 15;

    // SUPD constants
    pub const SBI_SUPD_EXT_ID: usize = 0x53555044;
    pub const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: usize = 0;

    #[repr(C)]
    pub struct SbiRet {
        pub a0: isize,
        pub a1: isize,
    }

    pub fn sbi_call(extid: usize, fid: usize, args: &[usize; 5]) -> SbiRet {
        let (a0, a1);
        unsafe {
            core::arch::asm!(
                "ecall",
                in("a7") extid,
                in("a6") fid,
                inlateout("a0") args[0] => a0,
                inlateout("a1") args[1] => a1,
                in("a2") args[2],
                in("a3") args[3],
                in("a4") args[4],
            );
        }
        SbiRet { a0, a1 }
    }
}

pub mod security {
    use coset::{CborSerializable, CoseSign1};
    use ed25519_dalek::{Signature, VerifyingKey};

    pub struct SecurityContext {
        cdi_id: [u8; 20],
        sign1: CoseSign1,
    }

    impl SecurityContext {
        /// Parses the DICE input formatted as follows:
        /// [0..20] -> CDI id
        /// [21..] -> CoseSign1
        /// verifying_key is a raw 32 byte ED25519 public key
        pub fn from_slice(data: &[u8], verifying_key: &[u8; 32]) -> Self {
            assert!(data.len() >= 20, "input too short");

            let mut cdi_id = [0u8; 20];
            cdi_id.copy_from_slice(&data[..20]);

            let sign1 = CoseSign1::from_slice(&data[20..]).unwrap();
            let verifiying_key = VerifyingKey::from_bytes(verifying_key).unwrap();

            sign1
                .verify_signature(b"", |sig, data| {
                    let signature = Signature::from_slice(sig).unwrap();
                    verifiying_key.verify_strict(data, &signature)
                })
                .unwrap();

            Self { cdi_id, sign1 }
        }
    }
}
