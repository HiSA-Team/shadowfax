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
    pub const SBI_COVH_ADD_ZERO_PAGES: usize = 12;
    pub const SBI_COVH_CREATE_TVM_VCPU: usize = 14;
    pub const SBI_COVH_RUN_TVM_VCPU: usize = 15;

    // SUPD constants
    pub const SBI_SUPD_EXT_ID: usize = 0x53555044;
    pub const SBI_EXT_SUPD_GET_ACTIVE_DOMAINS: usize = 0;

    // CoVG constants
    pub const COVG_EXTENSION: usize = 0x434F5647;
    pub const COVG_GET_EVIDENCE: usize = 8;

    pub const PAGE_SIZE: usize = 4096;

    #[repr(C)]
    pub struct SbiRet {
        pub a0: isize,
        pub a1: isize,
    }

    pub fn sbi_call(extid: usize, fid: usize, args: &[usize; 6]) -> SbiRet {
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
                in("a5") args[5],
            );
        }
        SbiRet { a0, a1 }
    }
}

pub mod attestation {
    extern crate alloc;
    use alloc::vec::Vec;
    use coset::{
        AsCborValue, CborSerializable, CoseKeyBuilder, CoseSign1, CoseSign1Builder, HeaderBuilder,
        cbor::{Value, Value::Integer},
        cwt::{self, ClaimsSet},
        iana::{self, Algorithm},
    };
    use ed25519_compact::{KeyPair, PublicKey, Seed, Signature};
    use sha2::Sha512;

    const CDI_LENGTH: usize = 32;

    const ASYM_SALT: [u8; 64] = [
        0x63, 0xB6, 0xA0, 0x4D, 0x2C, 0x07, 0x7F, 0xC1, 0x0F, 0x63, 0x9F, 0x21, 0xDA, 0x79, 0x38,
        0x44, 0x35, 0x6C, 0xC2, 0xB0, 0xB4, 0x41, 0xB3, 0xA7, 0x71, 0x24, 0x03, 0x5C, 0x03, 0xF8,
        0xE1, 0xBE, 0x60, 0x35, 0xD3, 0x1F, 0x28, 0x28, 0x21, 0xA7, 0x45, 0x0A, 0x02, 0x22, 0x2A,
        0xB1, 0xB3, 0xCF, 0xF1, 0x67, 0x9B, 0x05, 0xAB, 0x1C, 0xA5, 0xD1, 0xAF, 0xFB, 0x78, 0x9C,
        0xCD, 0x2B, 0x0B, 0x3B,
    ];

    const PROFILE_LABEL: i64 = 265;
    const PLATFORM_PUBLIC_KEY_LABEL: i64 = -70_000;
    const MANUFACTURER_ID_LABEL: i64 = -70_001;
    const PLATFORM_STATE_LABEL: i64 = -70_002;
    const PLATFORM_SW_COMPONENTS_LABEL: i64 = -70_003;
    const TSM_PUBLIC_KEY_LABEL: i64 = -70_004;

    #[derive(Debug)]
    pub enum AttestationError {
        InvalidPublicKey,
        MissingSignature,
        InvalidSignatureFormat,
        SignatureVerificationFailed,
    }
    /// A Compound Device Identifier (CDI) wrapper.
    #[derive(Clone)]
    pub struct Cdi(Vec<u8>);

    impl Default for Cdi {
        fn default() -> Self {
            Self(Default::default())
        }
    }

    impl Cdi {
        /// Derive the next CDI given measurements (using HKDF).
        fn derive_next(&self, next_measurement: &[u8]) -> Self {
            let mut okm = [0u8; CDI_LENGTH];
            let next_measurement = if next_measurement.len() > 0 {
                Some(next_measurement)
            } else {
                None
            };
            // HKDF(salt=measurement, input_key_material=previous CDI)
            hkdf::Hkdf::<Sha512>::new(next_measurement, &self.0)
                .expand(b"CDI_Attest", &mut okm)
                .expect("HKDF output length");
            Cdi(okm.to_vec())
        }
        /// Derive an Ed25519 keypair from this CDI.
        fn derive_keys(&self) -> KeyPair {
            let mut seed = [0u8; CDI_LENGTH];
            // HKDF(salt=ASYM_SALT, input_key_material=this CDI)
            hkdf::Hkdf::<Sha512>::new(Some(&ASYM_SALT), &self.0)
                .expand(b"Key Pair", &mut seed)
                .expect("HKDF for key seed");
            let seed = Seed::from_slice(&seed).unwrap();
            KeyPair::from_seed(seed)
        }
    }

    /// Trait representing a DICE layer. Layers can compute the next layer and verify their token.
    pub trait DiceLayer {
        type NextLayer;

        /// Derive the next layer from this one using `measurement`.
        fn compute_next(&self, measurement: &[u8]) -> Self::NextLayer;
        /// Get a reference to this layer's CDI.
        fn cdi(&self) -> &Cdi;
        /// Get a reference to this layer's COSE_Sign1 token.
        fn token(&self) -> &CoseSign1;
        /// Verify *this* layer's COSE_Sign1 token against an external Ed25519 public key.
        fn verify_with_pubkey(&self, parent_pubkey: &[u8]) -> Result<(), AttestationError> {
            verify_cose_signature(self.token(), parent_pubkey)
        }
    }

    /// Platform layer (e.g. hardware RoT); holds its CDI and self-signed token.
    #[derive(Clone)]
    pub struct PlatformAttestationContext {
        cdi: Cdi,
        token: CoseSign1,
    }

    impl DiceLayer for PlatformAttestationContext {
        type NextLayer = TsmAttestationContext;
        fn compute_next(&self, measurement: &[u8]) -> Self::NextLayer {
            // Derive TSM CDI from Platform CDI and TSM measurement
            let next_cdi = self.cdi.derive_next(measurement);
            let tsm_public_key = {
                let pkey = next_cdi.derive_keys().pk;
                CoseKeyBuilder::new_okp_key()
                    .algorithm(Algorithm::EdDSA)
                    .param(-2, coset::cbor::Value::Bytes(pkey.to_vec()))
                    .build()
            };

            // Add to the previous SW Components the TSM measurement
            let platform_claims = PlatformClaims::from_cbor(self.token.payload.as_ref().unwrap());
            let mut platform_sw_components = platform_claims.platform_sw_components.clone();

            platform_sw_components.push(RiscvCoveSwComponent {
                component_type: alloc::string::ToString::to_string(&"tsm"),
                measurement: measurement.to_vec(),
                svn: alloc::string::ToString::to_string(&"0"),
                manifest_hash: None,
                signer_pubkey_hash: Vec::new(),
                hash_alg_id: alloc::string::ToString::to_string(&"SHA512"),
            });

            let tsm_claims = TsmClaims {
                tsm_public_key: tsm_public_key.to_vec().unwrap(),
                platform_sw_components,
            };

            // Sign the TSM token payload with Platform's derived key
            let signing_key = self.cdi.derive_keys().sk;

            let protected = HeaderBuilder::new()
                .algorithm(iana::Algorithm::EdDSA)
                .key_id(b"TSM".to_vec())
                .build();
            let tsm_token = CoseSign1Builder::new()
                .protected(protected)
                .payload(
                    tsm_claims
                        .to_claims_set()
                        .to_cbor_value()
                        .unwrap()
                        .to_vec()
                        .unwrap(),
                )
                .create_signature(b"", |data| signing_key.sign(data, None).to_vec())
                .build();

            TsmAttestationContext {
                platform_token: self.token.clone(),
                cdi: next_cdi,
                token: tsm_token,
            }
        }
        fn cdi(&self) -> &Cdi {
            &self.cdi
        }
        fn token(&self) -> &CoseSign1 {
            &self.token
        }
    }

    impl PlatformAttestationContext {
        pub fn init_from_addr(addr: usize) -> Self {
            let ptr = addr as *const u8;
            Self::from_raw_bytes(ptr)
        }

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
                Cdi(Vec::from(slice))
            };

            let len = read_u32(&mut offset) as usize;

            // Read CoseSign1
            let token = {
                let slice = unsafe { core::slice::from_raw_parts(ptr.add(offset), len) };
                CoseSign1::from_slice(slice).unwrap()
            };

            Self { cdi, token }
        }
    }

    /// TSM layer: holds its CDI, its token (signed by Platform) and the Platform token for evidence composition
    #[derive(Clone)]
    pub struct TsmAttestationContext {
        cdi: Cdi,
        platform_token: CoseSign1,
        token: CoseSign1,
    }

    impl Default for TsmAttestationContext {
        fn default() -> Self {
            Self {
                cdi: Default::default(),
                platform_token: Default::default(),
                token: Default::default(),
            }
        }
    }

    impl TsmAttestationContext {
        pub fn init_from_addr(addr: usize) -> Self {
            let ptr = addr as *const u8;
            Self::from_raw_bytes(ptr)
        }

        fn from_raw_bytes(ptr: *const u8) -> Self {
            todo!()
        }
    }

    impl DiceLayer for TsmAttestationContext {
        type NextLayer = TvmAttestationContext;

        fn compute_next(&self, measurement: &[u8]) -> Self::NextLayer {
            // Derive TSM CDI from Platform CDI and TSM measurement
            let next_cdi = self.cdi.derive_next(measurement);

            TvmAttestationContext {
                platform_token: self.platform_token.clone(),
                cdi: next_cdi,
                tsm_token: self.token.clone(),
            }
        }

        fn cdi(&self) -> &Cdi {
            &self.cdi
        }

        fn token(&self) -> &CoseSign1 {
            &self.token
        }
    }

    /// TSM layer: holds its CDI, its token (signed by Platform) and the Platform token for evidence composition
    #[derive(Clone)]
    pub struct TvmAttestationContext {
        cdi: Cdi,
        platform_token: CoseSign1,
        tsm_token: CoseSign1,
    }

    impl Default for TvmAttestationContext {
        fn default() -> Self {
            Self {
                cdi: Cdi::default(),
                platform_token: Default::default(),
                tsm_token: Default::default(),
            }
        }
    }

    impl TvmAttestationContext {
        pub fn cdi(&self) -> &Cdi {
            &self.cdi
        }

        /// Returns (platform_token, tsm_token, tvm_token) packaged into a `Evidence` representation.
        pub fn get_evidence(&self, _tvm_measurement: &[u8], challenge: &[u8]) -> Evidence {
            // Build TVM token payload: include placeholder claims + the challenge
            let mut tvm_payload = Vec::new();
            tvm_payload.extend_from_slice(b"tvm-claims:");
            tvm_payload.extend_from_slice(challenge);

            // Sign TVM token with TSM's key (i.e., key derived from TSM CDI)
            let tsm_key = self.cdi.derive_keys();
            let protected = HeaderBuilder::new()
                .algorithm(iana::Algorithm::EdDSA)
                .build();

            let tvm_token = CoseSign1Builder::new()
                .protected(protected)
                .payload(tvm_payload)
                .create_signature(&[], |m| tsm_key.sk.sign(m, None).to_vec())
                .build();

            // Compose riscv-cove-token (submodule map) with platform/tsm/tvm tokens
            Evidence {
                platform: self.platform_token.clone(),
                tsm: self.tsm_token.clone(),
                tvm: tvm_token,
            }
        }
    }

    /// Evidence envelope struct matching the "riscv-cove-token" submodule form.
    pub struct Evidence {
        pub platform: CoseSign1,
        pub tsm: CoseSign1,
        pub tvm: CoseSign1,
    }

    impl Evidence {
        /// Serializes the Evidence into a CBOR byte vector.
        /// Format: CBOR Array [PlatformToken, TsmToken, TvmToken]
        pub fn to_bytes(&self) -> Result<Vec<u8>, coset::CoseError> {
            let value = self.to_cbor_value()?;
            let mut bytes = Vec::new();
            // Serialize the CBOR Value to bytes
            coset::cbor::ser::into_writer(&value, &mut bytes)
                .map_err(|e| coset::CoseError::EncodeFailed)?;
            Ok(bytes)
        }

        /// Converts the Evidence struct into a generic CBOR Value.
        fn to_cbor_value(&self) -> Result<Value, coset::CoseError> {
            Ok(Value::Array(alloc::vec![
                self.platform.clone().to_cbor_value()?,
                self.tsm.clone().to_cbor_value()?,
                self.tvm.clone().to_cbor_value()?,
            ]))
        }
    }

    /// Verify a COSE_Sign1 token using an external Ed25519 public key bytes.
    ///
    /// Uses `CoseSign1::verify_signature` to obtain (sig, data) and performs Ed25519 verification.
    fn verify_cose_signature(
        token: &CoseSign1,
        parent_pubkey: &[u8],
    ) -> Result<(), AttestationError> {
        // Convert public key
        let pk =
            PublicKey::from_slice(parent_pubkey).map_err(|_| AttestationError::InvalidPublicKey)?;

        // Use coset's verify_signature helper which supplies the payload to be verified.
        // The closure must return Result<(), E> where E is the underlying verification error type.
        token.verify_signature(b"", |sig, data| {
            // Convert signature to ed25519 type
            let sig =
                Signature::from_slice(sig).map_err(|_| AttestationError::InvalidSignatureFormat)?;
            pk.verify(data, &sig)
                .map_err(|_| AttestationError::SignatureVerificationFailed)
        })
    }

    impl core::fmt::Display for AttestationError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self {
                Self::InvalidPublicKey => write!(f, "invalid public key"),
                Self::MissingSignature => write!(f, "missing signature"),
                Self::InvalidSignatureFormat => write!(f, "invalid signature format"),
                Self::SignatureVerificationFailed => write!(f, "signature verification failed"),
            }
        }
    }
    impl core::error::Error for AttestationError {}

    #[derive(Debug, Clone)]
    struct RiscvCoveSwComponent {
        component_type: alloc::string::String, // 1 => text
        measurement: Vec<u8>,                  // 2 => hash
        svn: alloc::string::String,            // 3 => text
        manifest_hash: Option<Vec<u8>>,        // 4 => hash
        signer_pubkey_hash: Vec<u8>,           // 5 => hash
        hash_alg_id: alloc::string::String,    // 6 => text
    }

    impl RiscvCoveSwComponent {
        fn to_cbor(&self) -> Value {
            let mut map = Vec::from([
                (
                    Value::Integer(1.into()),
                    Value::Text(self.component_type.clone()),
                ),
                (
                    Value::Integer(2.into()),
                    Value::Bytes(self.measurement.clone()),
                ),
                (Value::Integer(3.into()), Value::Text(self.svn.clone())),
                (
                    Value::Integer(5.into()),
                    Value::Bytes(self.signer_pubkey_hash.clone()),
                ),
                (
                    Value::Integer(6.into()),
                    Value::Text(self.hash_alg_id.clone()),
                ),
            ]);

            if let Some(h) = &self.manifest_hash {
                map.push((Value::Integer(4.into()), Value::Bytes(h.clone())));
            }

            Value::Map(map)
        }
    }

    #[derive(Debug, Clone)]
    struct TsmClaims {
        tsm_public_key: Vec<u8>,
        platform_sw_components: Vec<RiscvCoveSwComponent>,
    }

    impl TsmClaims {
        fn to_claims_set(&self) -> ClaimsSet {
            let sw_components_array = Value::Array(
                self.platform_sw_components
                    .iter()
                    .map(|c| c.to_cbor())
                    .collect(),
            );

            let mut builder = cwt::ClaimsSetBuilder::new();
            let key_bytes = Value::Bytes(self.tsm_public_key.clone());

            builder = builder.private_claim(TSM_PUBLIC_KEY_LABEL, key_bytes);

            builder = builder.private_claim(PLATFORM_SW_COMPONENTS_LABEL, sw_components_array);

            builder.build()
        }
    }

    #[derive(Debug, Clone)]
    enum PlatformState {
        NotConfigured,
        Secured,
        Debug,
        Recovery,
    }

    #[derive(Debug, Clone)]
    struct PlatformClaims {
        platform_public_key: Vec<u8>,
        manufacturer_id: [u8; 64],
        platform_state: PlatformState,
        platform_sw_components: Vec<RiscvCoveSwComponent>,
    }

    impl PlatformClaims {
        fn from_cbor(data: &[u8]) -> PlatformClaims {
            let value: Value = coset::cbor::from_reader(data).expect("invalid CBOR");

            let map = match value {
                Value::Tag(_, boxed) => match *boxed {
                    Value::Map(m) => m,
                    _ => panic!("platform token must contain a CBOR map"),
                },
                Value::Map(m) => m,
                _ => panic!("platform token must be a CBOR map"),
            };

            // Helper closures
            let get_bytes = |v: Value| match v {
                Value::Bytes(b) => b,
                _ => panic!("expected bytes"),
            };

            let get_text = |v: Value| match v {
                Value::Text(t) => t,
                _ => panic!("expected text"),
            };

            let get_i64 = |v: Value| match v {
                Value::Integer(n) => n,
                _ => panic!("expected integer"),
            };

            let mut platform_public_key = None;
            let mut manufacturer_id = None;
            let mut platform_state = None;
            let mut platform_sw_components = None;

            for (k, v) in map {
                let key = match k {
                    Value::Integer(n) => n,
                    _ => continue,
                };

                if key == PROFILE_LABEL.into() {
                    continue;
                }

                if key == PLATFORM_PUBLIC_KEY_LABEL.into() {
                    platform_public_key = Some(get_bytes(v));
                    continue;
                }

                if key == MANUFACTURER_ID_LABEL.into() {
                    let bytes = get_bytes(v);
                    if bytes.len() != 64 {
                        panic!("manufacturer-id must be 64 bytes");
                    }
                    let mut arr = [0u8; 64];
                    arr.copy_from_slice(&bytes);
                    manufacturer_id = Some(arr);
                    continue;
                }

                if key == PLATFORM_STATE_LABEL.into() {
                    let n = get_i64(v);
                    let state = match n.into() {
                        1 => PlatformState::NotConfigured,
                        2 => PlatformState::Secured,
                        3 => PlatformState::Debug,
                        4 => PlatformState::Recovery,
                        _ => panic!("invalid platform-state"),
                    };
                    platform_state = Some(state);
                    continue;
                }

                if key == PLATFORM_SW_COMPONENTS_LABEL.into() {
                    let arr = match v {
                        Value::Array(a) => a,
                        _ => panic!("platform-sw-components must be array"),
                    };

                    let mut comps = Vec::new();

                    for entry in arr {
                        let comp_map = match entry {
                            Value::Map(m) => m,
                            _ => panic!("component entry must be map"),
                        };

                        let mut component_type = None;
                        let mut measurement = None;
                        let mut svn = None;
                        let mut manifest_hash = None;
                        let mut signer_pubkey_hash = None;
                        let mut hash_alg_id = None;

                        for (ck, cv) in comp_map {
                            let ckey = match ck {
                                Value::Integer(n) => n,
                                _ => continue,
                            };

                            match ckey.into() {
                                1 => component_type = Some(get_text(cv)),
                                2 => measurement = Some(get_bytes(cv)),
                                3 => svn = Some(get_text(cv)),
                                4 => manifest_hash = Some(get_bytes(cv)),
                                5 => signer_pubkey_hash = Some(get_bytes(cv)),
                                6 => hash_alg_id = Some(get_text(cv)),
                                _ => {}
                            }
                        }

                        comps.push(RiscvCoveSwComponent {
                            component_type: component_type.expect("missing 1"),
                            measurement: measurement.expect("missing 2"),
                            svn: svn.expect("missing 3"),
                            manifest_hash,
                            signer_pubkey_hash: signer_pubkey_hash.expect("missing 5"),
                            hash_alg_id: hash_alg_id.expect("missing 6"),
                        });
                    }

                    platform_sw_components = Some(comps);
                    continue;
                }
            }

            PlatformClaims {
                platform_public_key: platform_public_key.expect("missing platform-public-key"),
                manufacturer_id: manufacturer_id.expect("missing manufacturer-id"),
                platform_state: platform_state.expect("missing platform-state"),
                platform_sw_components: platform_sw_components
                    .expect("missing platform-sw-components"),
            }
        }
    }
}
