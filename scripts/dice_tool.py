# /// script
# dependencies = [
#   "cbor2",
#   "cryptography",
#   "pycose"
# ]
# ///

####################################################################################################
# This program aims to substitute the HRoT to provide the TSM-Driver with CDI and Certificate according
# to the CoVE specification.
#
# DICE inputs:
#     - Code (64 byte) [MANDATORY]: hash of the code
#     - Configuration Data (64 bytes) [OPTIONAL]: generic configuration hash
#     - Authorization Data (64 bytes) [OPTIONAL]: hash of a public key. This is expected to be stable since it comes from manufacturers
#     - Mode (1 byte) [OPTIONAL]: mode of the application
#     - Hidden Inputs (64 bytes) [OPTIONAL]: The input value is hidden in the sense that it does not appear in any certificate.
#                                 It is used for both attestation and sealing CDI derivation so it is expected to be stable;
#
# This code will use HKDK and SHA512 as algorithms
#
# Reference https://trustedcomputinggroup.org/wp-content/uploads/DICE-Attestation-Architecture-v1.2_pub.pdf
# Reference https://github.com/google/open-dice/blob/main/docs/specification.md
# Reference architecture is Ch. 6 of the CoVE specification.
#
# Author: Giuseppe Capasso <capassog97@gmail.com>
####################################################################################################

import argparse
import cbor2
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

from pycose.algorithms import EdDSA
from pycose.headers import Algorithm, KID
from pycose.keys import CoseKey
from pycose.messages import Sign1Message
from pycose.keys.keyparam import KpKty, OKPKpD, OKPKpX, KpKeyOps, OKPKpCurve
from pycose.keys.keytype import KtyOKP
from pycose.keys.keyops import SignOp
from pycose.keys.curves import Ed25519

import struct

# UDS is a 32-byte secret which should be provided by the manufacturer
UDS = bytes(32)

# These are specified by the protocol
ASYM_SALT = bytes.fromhex(
    "63B6A04D2C077FC10F639F21DA793844356CC2B0B441B3A77124035C03F8E1BE6035D31F282821A7450A02222AB1B3CFF1679B05AB1CA5D1AFFB789CCD2B0B3B"
)
ID_SALT = bytes.fromhex(
    "DBDBAEBC8020DA9FF0DD5A24C83AA5A54286DFC263031E329B4DA148430659FE62CDB5B7E1E00FC680306711EB444AF77209359496FCFF1DB9520BA51C7B29EA"
)

# EAT Profile Claim
RISCV_COVE_EAT_PROFILE_LABEL = 265
RISCV_COVE_EAT_PROFILE_DOC = "https://riscv.org/TBD"

# Platform Public Key Claim
PLATFORM_PUBLIC_KEY_LABEL = 266

# Platform Manufacturer Identifier Claim
PLATFORM_MANUFACTURER_ID_LABEL = 267
PLATFORM_MANUFACTURER_ID_TYPE = bytes(64)

# Platform State Claim
PLATFORM_STATE_LABEL = 268
PLATFORM_STATE_NOT_CONFIGURED = 1
PLATFORM_STATE_SECURED = 2
PLATFORM_STATE_DEBUG = 3
PLATFORM_STATE_RECOVERY = 4

# Platform Software Components Claim
PLATFORM_SW_COMPONENTS_LABEL = 269


def calculate_hash_from_file(file: str) -> bytes:
    # Calulcate TSM-driver digest
    bufsize = 8192
    m = hashes.Hash(hashes.SHA512())
    with open(file, "rb") as file:
        # Read the file in chunks
        while chunk := file.read(bufsize):
            m.update(chunk)

    return m.finalize()


def calculate_input_hash(
    code_hash: bytes,
    authority_data: bytes,
    config_data: bytes,
    mode: bytes,
    hidden: bytes,
) -> bytes:
    # Calculate Root TCB digest
    digest = hashes.Hash(hashes.SHA512())
    digest.update(code_hash + config_data + authority_data + mode + hidden)

    return digest.finalize()


def calculate_asym_keypair(
    input: bytes,
) -> (ed25519.Ed25519PrivateKey, ed25519.Ed25519PublicKey):
    seed = HKDF(
        algorithm=hashes.SHA512(), length=32, salt=ASYM_SALT, info=b"Key Pair"
    ).derive(input)

    cdi_private = ed25519.Ed25519PrivateKey.from_private_bytes(seed)
    cdi_public = cdi_private.public_key()

    return (cdi_private, cdi_public)


def generate_platform_cwt(
    uds_keypair: (ed25519.Ed25519PrivateKey, ed25519.Ed25519PublicKey),
    cdi_public: ed25519.Ed25519PublicKey,
    fw_hash: bytes,
) -> bytes:
    # raw public bytes for the CDI (platform) key
    cdi_public_bytes = cdi_public.public_bytes(
        encoding=serialization.Encoding.Raw, format=serialization.PublicFormat.Raw
    )
    # raw uds private + public bytes (used to construct a COSE OKP key for signing)
    uds_private_bytes = uds_keypair[0].private_bytes(
        encoding=serialization.Encoding.Raw,
        format=serialization.PrivateFormat.Raw,
        encryption_algorithm=serialization.NoEncryption(),
    )
    uds_public_bytes = uds_keypair[1].public_bytes(
        encoding=serialization.Encoding.Raw, format=serialization.PublicFormat.Raw
    )

    # Build a COSE_Key for the platform public key (this must be CBOR-encoded bytes)
    platform_cose_key_map = {
        1: 1,  # kty = OKP
        -1: 6,  # crv = Ed25519
        -2: cdi_public_bytes,  # x coordinate (raw public key)
    }
    platform_public_key_claim = cbor2.dumps(
        platform_cose_key_map
    )  # bytes .cbor COSE_Key

    # Platform claims map (values must follow the spec types)
    platform_token_map = {
        RISCV_COVE_EAT_PROFILE_LABEL: RISCV_COVE_EAT_PROFILE_DOC,
        PLATFORM_PUBLIC_KEY_LABEL: platform_public_key_claim,
        PLATFORM_MANUFACTURER_ID_LABEL: PLATFORM_MANUFACTURER_ID_TYPE,
        PLATFORM_STATE_LABEL: PLATFORM_STATE_DEBUG,
        PLATFORM_SW_COMPONENTS_LABEL: [
            {
                1: "tsm-driver",  # Component Type
                2: fw_hash,  # Component hash
                3: 0x0,  # SVN
                4: 0x0,  # Manifest hash (unused)
                5: 0x0,  # Component signer (unused)
                6: "SHA512",  # Hash Algorithm Name
            }
        ],
    }
    # CBOR-encode the claims map -> payload for COSE_Sign1
    payload = cbor2.dumps(platform_token_map)

    # Build Sign1Message with protected header specifying EdDSA
    message = Sign1Message(phdr={Algorithm: EdDSA, KID: b"0"}, payload=payload)

    # Construct the COSE key dict for the RoT attestation key (UDS-derived).
    # IMPORTANT: OKPKpX must be the public key bytes; include the private scalar for signing.
    cose_key = {
        KpKty: KtyOKP,
        OKPKpCurve: Ed25519,
        KpKeyOps: [SignOp],
        OKPKpX: uds_public_bytes,  # public 'x' (raw)
        OKPKpD: uds_private_bytes,  # private scalar (label depends on library; KpD used here)
    }

    # Convert to library CoseKey object (assumes CoseKey.from_dict exists and constants match)
    cose_key_obj = CoseKey.from_dict(cose_key)

    # Attach signing key and encode
    message.key = cose_key_obj

    return message.encode(tag=False)


def generate_uds_keys(args) -> None:
    uds_private, uds_public = calculate_asym_keypair(UDS)

    with open(args.private_key, "wb") as f:
        uds_private_bytes = uds_private.private_bytes(
                encoding=serialization.Encoding.Raw,
                format=serialization.PrivateFormat.Raw,
                encryption_algorithm=serialization.NoEncryption(),
        )
        f.write(uds_private_bytes)

    with open(args.public_key, "wb") as f:
        uds_public_bytes = uds_public.public_bytes(
                encoding=serialization.Encoding.Raw,
                format=serialization.PublicFormat.Raw
        )
        f.write(uds_public_bytes)

def generate_platform_token(args) -> None:
    # DICE Inputs
    configuration_data = bytes(64)
    authority_data = bytes(64)  # (all zeros if not used)
    mode = bytes(1)  # Mode as single byte
    hidden = bytes(64)  # Hidden inputs (all zeros if not used)
    code_hash = calculate_hash_from_file(args.input)  # Calculates SHA512 (64 bytes)

    # read uds keys from files
    with open(args.uds_private_key, "rb") as f:
        uds_private = ed25519.Ed25519PrivateKey.from_private_bytes(f.read())

    with open(args.uds_public_key, "rb") as f:
        uds_public = ed25519.Ed25519PublicKey.from_public_bytes(f.read())

    # Calculate input hash
    attest_input = calculate_input_hash(
        code_hash, authority_data, configuration_data, mode, hidden
    )

    cdi0 = HKDF(
        algorithm=hashes.SHA512(), length=32, salt=attest_input, info=b"CDI_Attest"
    ).derive(UDS)

    # Create CDI and UDS asymetric key pair
    cdi_private, cdi_public = calculate_asym_keypair(cdi0)

    # Generate Platform CWT
    platform_cwt = generate_platform_cwt(
        (uds_private, uds_public), cdi_public, code_hash
    )

    # Saves the Payload input formatted as follows:
    # |--------|-----------------|--------|-----------------|
    # | 4byte  |      CDILEN     | 4byte  |      EATLEN     |
    # |--------|-----------------|--------|-----------------|
    # | CDILEN |       CDI       | EATLEN |       EAT       |
    # |--------|-----------------|--------|-----------------|
    with open(args.output, "wb") as f:
        f.write(struct.pack("<I", len(cdi0)))
        f.write(cdi0)
        f.write(struct.pack("<I", len(platform_cwt)))
        f.write(platform_cwt)

parser = argparse.ArgumentParser(
    description="Utility to manage and generate DICE keys and signatures for Platform",
    epilog="This program is for testing purposes. The UDS comes from the HRoT and must be secret",
    usage="python script <cmd> [args...]",
)

subparsers = parser.add_subparsers(help="Available commands are 'generate-platform-token' 'generate-uds-keys'", required=True)
parser_generate_uds_keys = subparsers.add_parser("generate-uds-keys", help='Generate UDS public and private keys')
parser_generate_uds_keys.add_argument("private_key", help="Path where to save the private UDS key")
parser_generate_uds_keys.add_argument("public_key", help="Path where to save the public UDS key")
parser_generate_uds_keys.set_defaults(func=generate_uds_keys)

parser_generate_platform_token = subparsers.add_parser("generate-platform-token", help='Generate platform EAT token according to CoVE specification')
parser_generate_platform_token.add_argument("input", help="Input file. This should be the TSM-driver binary")
parser_generate_platform_token.add_argument("--uds-private-key", help="Path to the UDS private key as binary")
parser_generate_platform_token.add_argument("--uds-public-key", help="Path to the UDS public key as binary")
parser_generate_platform_token.add_argument("output", help="Output DICE package. Contains 20-byte CDI and the EAT token")
parser_generate_platform_token.set_defaults(func=generate_platform_token)


if __name__ == "__main__":
    # Call the right function configured with the `set_defaults`
    args = parser.parse_args()
    args.func(args)
