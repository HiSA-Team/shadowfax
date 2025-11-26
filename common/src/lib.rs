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
    extern crate alloc;
    use alloc::vec::Vec;
    use coset::{CborSerializable, CoseSign1};
    use ed25519_compact::{KeyPair, Seed, Signature};
    use sha2::Sha512;

    const CDI_LENGTH: usize = 32;

    #[repr(C)]
    #[derive(Clone)]
    struct Cdi(Vec<u8>);

    #[repr(C)]
    #[derive(Clone)]
    pub struct AttestationPayload {
        cdi: Cdi,
        token: CoseSign1,
    }

    impl From<*const u8> for AttestationPayload {
        fn from(ptr: *const u8) -> Self {
            Self::from_raw_bytes(ptr)
        }
    }

    impl AttestationPayload {
        /// Parses the Payload input formatted as follows:
        /// |--------|-----------------|--------|-----------------|
        /// | 4byte  |      CDILEN     | 4byte  |      EATLEN     |
        /// |--------|-----------------|--------|-----------------|
        /// | CDILEN |       CDI       | EATLEN |       EAT       |
        /// |--------|-----------------|--------|-----------------|
        fn from_raw_bytes(ptr: *const u8) -> Self {
            let mut offset = 0;

            let read_u32 = |offset: &mut usize| -> u32 {
                let mut buf = [0; 4];
                for i in 0..4 {
                    buf[i] = unsafe { core::ptr::read(ptr.add(*offset + i)) };
                }
                *offset += 4;
                u32::from_le_bytes(buf)
            };

            // Read CDI len and CDI
            let len = read_u32(&mut offset) as usize;

            // Read CDI
            let cdi = {
                let slice = unsafe { core::slice::from_raw_parts(ptr.add(offset), len) };
                offset += len;
                Vec::from(slice)
            };

            let len = read_u32(&mut offset) as usize;

            // Read CoseSign1
            let token = {
                let slice = unsafe { core::slice::from_raw_parts(ptr.add(offset), len) };
                CoseSign1::from_slice(slice).unwrap()
            };

            Self {
                cdi: Cdi(cdi),
                token,
            }
        }
    }

    pub enum AttestationContext {
        None,
        Platform { payload: AttestationPayload },
        Tsm { payload: AttestationPayload },
        Tvm { payload: AttestationPayload },
    }

    impl AttestationContext {
        pub fn init_from_addr(addr: usize) -> Self {
            let ptr = addr as *const u8;
            let payload = AttestationPayload::from(ptr);
            Self::Platform { payload }
        }

        pub fn compute_next(&self, next_layer_data: &[u8]) -> AttestationContext {
            match self {
                Self::Platform { payload } => {
                    // Build the attestation context for the TSM
                    let token = generate_tsm_token(&payload.cdi);
                    let cdi = payload.cdi.generate_next(&[0; 64]);
                    Self::Tsm {
                        payload: AttestationPayload { cdi, token },
                    }
                }
                _ => panic!("invalid attestation context"),
            }
        }

        pub fn verify(
            &self,
            verifying_key: &[u8; ed25519_compact::PublicKey::BYTES],
        ) -> Result<(), ed25519_compact::Error> {
            let verifiying_key = ed25519_compact::PublicKey::from_slice(verifying_key).unwrap();

            let sign1 = match self {
                Self::Platform { payload } => &payload.token,
                Self::Tsm { payload } => &payload.token,
                Self::Tvm { payload } => &payload.token,
                _ => panic!("invalid attestation context"),
            };

            sign1.verify_signature(b"", |sig, data| {
                let signature = Signature::from_slice(sig).unwrap();
                verifiying_key.verify(data, &signature)
            })?;

            Ok(())
        }

        pub fn get_payload(&self) -> AttestationPayload {
            match self {
                Self::Platform { payload } => payload.clone(),
                Self::Tsm { payload } => payload.clone(),
                Self::Tvm { payload } => payload.clone(),
                _ => panic!("invalid attestation context"),
            }
        }
    }

    impl Cdi {
        fn generate_keys(&self) -> KeyPair {
            let mut seed = [0; CDI_LENGTH];
            hkdf::Hkdf::<Sha512>::new(None, self.0.as_slice())
                .expand(b"Key Pair", &mut seed)
                .expect("32 byte should be enough");
            let seed = Seed::from_slice(&seed).unwrap();
            KeyPair::from_seed(seed)
        }
        fn generate_next(&self, tcb_hash: &[u8]) -> Self {
            let mut okm = [0; CDI_LENGTH];
            hkdf::Hkdf::<Sha512>::new(None, self.0.as_slice())
                .expand(b"CDI_Attest", &mut okm)
                .expect("32 byte should be enough");

            Self(okm.to_vec())
        }
    }

    fn generate_tsm_token(cdi: &Cdi) -> coset::CoseSign1 {
        let keys = cdi.generate_keys();
        CoseSign1::default()
    }
}
