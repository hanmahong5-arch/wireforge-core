//! Wireforge bindings to the China GM/T cryptographic standards
//! (SM2 / SM3 / SM4).
//!
//! This crate exposes [`sm3`] (hash), [`sm4`] (block cipher, ECB/CBC
//! with PKCS#7 plus a bounded streaming encryptor), and [`sm2`]
//! (elliptic-curve digital signature). Each module is a thin,
//! fixed-shape wrapper over the corresponding RustCrypto crate.
//!
//! The SM2 and SM4 upstreams are **unaudited**: these modules provide
//! functional correctness only and carry **no** 密评 / GB/T 39786
//! compliance claim. Compliance-grade GM/T crypto is a separate
//! Tongsuo C-FFI route, not this crate.
//!
//! ## Why a Wireforge-owned wrapper
//!
//! Upstream crates (currently the RustCrypto `sm3` crate; previously
//! surveyed `smcrypto`, `gmsm`, and the C-FFI `Tongsuo` route — see
//! `docs/sm-crypto-research-2026-05.md`, including the 2026-05-29
//! reversal that moved SM3 onto RustCrypto) move at different paces and
//! ship slightly different APIs. Re-exporting the upstream directly
//! would couple every Wireforge call site to whichever crate today
//! happens to be in the dependency graph. A thin wrapper:
//!
//! - keeps the public API stable across upstream swaps,
//! - lets us add invariants the upstream does not enforce (e.g. fixed
//!   output array types instead of `Vec<u8>`), and
//! - gives us a single place to plug in compliance-relevant metadata
//!   (algorithm OID, GB/T standard version) when Phase 2 lands.

pub mod sm2;
pub mod sm3;
pub mod sm4;

pub use sm3::{sm3, sm3_hex, Sm3};

pub use sm4::{
    sm4_cbc_decrypt, sm4_cbc_encrypt, sm4_ecb_decrypt, sm4_ecb_encrypt, Sm4CbcEncryptor, Sm4Error,
    SM4_BLOCK_LEN, SM4_KEY_LEN,
};

pub use sm2::{
    sm2_verify, Sm2Error, Sm2KeyPair, Sm2Signature, SM2_DEFAULT_ID, SM2_PRIVATE_KEY_LEN,
    SM2_SIGNATURE_RAW_LEN,
};
