use core::{error::Error, fmt::Display};

use alloc::string::String;
use fdt_rs::{
    base::DevTreeNode,
    prelude::{FallibleIterator, PropReader},
};
use rsa::{
    pkcs1::DecodeRsaPublicKey,
    pkcs1v15::{Signature, VerifyingKey},
    signature::Verifier,
};
use sha2::Sha256;

#[derive(Clone)]
pub struct Domain {
    pub id: usize,
    name: String,
    pub active: usize,
    pub tsm_type: TsmType,
    pub trust_map: usize,
}

impl Domain {
    fn empty() -> Self {
        Self {
            id: 0,
            name: String::default(),
            active: 0,
            tsm_type: TsmType::None,
            trust_map: 0,
        }
    }

    pub fn from_fdt_node(node: &DevTreeNode) -> (Self, usize, usize) {
        let mut domain = Domain::empty();
        let mut start_addr = 0;
        let mut end_addr = 0;
        for prop in node.props().iterator() {
            if let Ok(prop) = prop {
                let name = prop.name().unwrap_or("");
                match name {
                    "id" => domain.id = prop.u32(0).unwrap() as usize,
                    "name" => domain.name = String::from(prop.str().unwrap()),
                    "tsm-type" => domain.tsm_type = TsmType::from(prop.str().unwrap()),
                    "memory" => {
                        start_addr = prop.u64(0).unwrap() as usize;
                        end_addr = prop.u64(1).unwrap() as usize;
                    }
                    "trust" => {
                        let node = node
                            .props()
                            .iterator()
                            .find(|c| c.as_ref().unwrap().name().unwrap_or("") == "trust")
                            .unwrap()
                            .unwrap();

                        let mut i = 0;
                        let mut trust = 0;
                        loop {
                            if let Ok(d) = node.u32(i) {
                                trust |= 1 << (d as usize);
                                i += 1
                            } else {
                                break;
                            }
                        }
                        domain.trust_map = trust;
                    }
                    _ => {}
                }
            }
        }
        domain.active = if domain.id == 0 { 1 } else { 0 };
        (domain, start_addr, end_addr)
    }
    pub fn verify_and_load_tsm(
        bin: &[u8],
        start_addr: usize,
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<(), anyhow::Error> {
        // Verify the tsm signature with the provided payload using the the public key
        let signature = Signature::try_from(signature).map_err(TsmError::SignatureError)?;
        let verifying_key = VerifyingKey::<Sha256>::from_pkcs1_der(&public_key)
            .map_err(TsmError::RsaPublicKeyError)?;
        verifying_key
            .verify(bin, &signature)
            .map_err(TsmError::SignatureError)?;

        // load the tsm into the destination address
        unsafe {
            core::ptr::copy_nonoverlapping(bin.as_ptr(), start_addr as *mut u8, bin.len());
        }

        Ok(())
    }

    pub fn is_trusted(&self, dst: usize) -> bool {
        self.trust_map & (1 << dst) != 0
    }
}

#[allow(unused)]
#[derive(Clone)]
pub enum TsmType {
    Default,
    External,
    None,
}

impl From<&str> for TsmType {
    fn from(value: &str) -> Self {
        match value.to_lowercase().as_ref() {
            "default" => TsmType::Default,
            "none" => TsmType::None,
            "external" => TsmType::External,
            _ => panic!("unknown tsm type"),
        }
    }
}

#[derive(Debug)]
pub enum TsmError {
    RsaPublicKeyError(rsa::pkcs1::Error),
    SignatureError(rsa::signature::Error),
}

impl Display for TsmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RsaPublicKeyError(err) => write!(f, "verification error: {}", err),
            Self::SignatureError(err) => write!(f, "signature error: {}", err),
        }
    }
}

impl Error for TsmError {}
