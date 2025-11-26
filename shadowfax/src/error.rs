use core::{error::Error, fmt::Display};

#[derive(Debug)]
pub enum TsmError {
    PublicKeyDecode(ed25519_compact::Error),
    SignatureDecode(ed25519_compact::Error),
    SignatureVerification(ed25519_compact::Error),
}

impl Display for TsmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::PublicKeyDecode(err) => write!(f, "public key format error: {}", err),
            Self::SignatureDecode(err) => write!(f, "signature format error: {}", err),
            Self::SignatureVerification(err) => write!(f, "signature verification error: {}", err),
        }
    }
}

impl Error for TsmError {}
