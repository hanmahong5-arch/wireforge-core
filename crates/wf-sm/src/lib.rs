//! Wireforge bindings to the China GM/T cryptographic standards
//! (SM2 / SM3 / SM4).
//!
//! Scope of this crate is intentionally narrow today: only [`sm3`] is
//! exposed. The `sm2` and `sm4` modules exist as named extension points
//! (zero items right now) so adding signature / cipher support later is
//! a one-file change in this crate rather than a workspace rearrangement.
//!
//! ## Why a Wireforge-owned wrapper
//!
//! Upstream crates (currently `smcrypto`; previously surveyed `gmsm`,
//! pure-Rust RustCrypto plug-ins, and the C-FFI `Tongsuo` route — see
//! `docs/sm-crypto-research-2026-05.md`) move at different paces and
//! ship slightly different APIs. Re-exporting the upstream directly
//! would couple every Wireforge call site to whichever crate today
//! happens to be in the dependency graph. A thin wrapper:
//!
//! - keeps the public API stable across upstream swaps,
//! - lets us add invariants the upstream does not enforce (e.g. fixed
//!   output array types instead of `Vec<u8>`), and
//! - gives us a single place to plug in compliance-relevant metadata
//!   (algorithm OID, GB/T standard version) when Phase 2 lands.

pub mod sm3;

pub mod sm2 {
    //! SM2 (elliptic-curve digital signature, key exchange) — extension
    //! point.
    //!
    //! Empty in the MVP. The wrap-up is straightforward once a consumer
    //! lands: re-export `smcrypto::sm2::Signer` / `Verifier` behind
    //! types that take `&[u8]` byte slices and return owned signatures,
    //! mirroring the [`super::sm3`] module's shape.
}

pub mod sm4 {
    //! SM4 (128-bit block cipher) — extension point.
    //!
    //! Empty in the MVP. Wireforge's report-replay flows don't encrypt
    //! payloads today; this slot is reserved for the Phase 2 wallet /
    //! PIN-block path documented in the strategy memo.
}

pub use sm3::{sm3, sm3_hex, Sm3};
