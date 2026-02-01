use core::{error::Error, fmt::Display};

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
