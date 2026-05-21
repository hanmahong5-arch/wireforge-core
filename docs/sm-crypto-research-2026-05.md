# SM crypto upstream research — 2026-05-21

Decision log for the SM3 / SM2 / SM4 dependency choice driving anchor B
(STRATEGY-v0.4 国密 first-class). Closes the "Phase 2 才开始研究依赖"
risk by selecting an upstream now and capturing the candidates considered.

Authoritative reference of record for the wf-sm crate built today.

## 1. Goal & constraints

- **Goal**: ship a pure-Rust SM3 wrapper today (`wf-sm`); leave SM2 / SM4
  as documented extension points.
- **Compliance**: must keep the path to GB/T 39786-2021 商用密码应用安全
  评估 (密评) open. Pure-Rust + Apache-2.0/MIT means the algorithm
  implementation is auditable and the license does not block enterprise
  redistribution.
- **Bundle**: Tauri shell bundle increment ≤ 200 KB (per strategy memo).
- **No new C toolchains**: avoid Tongsuo / GmSSL C-FFI on the desktop
  build to keep cross-platform compile times sane.

## 2. Candidate comparison

| crate                | upstream                  | status            | API shape                  | C deps | last release |
|----------------------|---------------------------|-------------------|----------------------------|--------|--------------|
| **smcrypto 0.3.1**   | zhuobie/smcrypto (MIT)    | active            | hex `String` for SM3       | none   | 0.3.1        |
| gmsm 0.1.0           | nfjBill/rust-gmsm (Apl-2) | maintenance       | `Vec<u8>` for SM3          | none   | 0.1.0        |
| RustCrypto sm3       | (unpublished GM-OID slot) | not published     | n/a — would need vendoring | none   | n/a          |
| Tongsuo (C-FFI)      | Tongsuo/Tongsuo           | active, audited   | OpenSSL-style EVP_MD       | yes    | rolling      |

Notes — plan v0.4 documented the upstream as `CrayfishGo/gm-rs`; that
attribution did not match the crate published on crates.io as `gmsm`,
which traces to `nfjBill/rust-gmsm`. Discrepancy surfaced and resolved
before committing — see § 3.

## 3. Selection — smcrypto 0.3.1

Chosen for 4 reasons:

1. **Newer & better-maintained revision count** (0.3.1 vs 0.1.0). More
   recent fixes land in `smcrypto`; `gmsm` 0.1.0 has not seen a release
   in the same window.
2. **Smaller call surface for SM3** — `smcrypto::sm3::sm3_hash` returns
   a 64-char lowercase hex `String` that we re-pack into `[u8; 32]` in
   the wrapper. `gmsm` returns `Vec<u8>` which is functionally similar
   but spends a heap allocation per call without offering richer
   semantics.
3. **No C / build-tool dependencies** — pure Rust through to the
   compression function. Tauri bundle delta measured during `cargo
   build -p wf-sm` adds 14 transitive crates (rand, num-bigint,
   yasna, pem, base64, hex, …) — within the ≤ 200 KB Tauri bundle
   budget since most are link-pruned for SM3-only builds.
4. **Migration path stays open** — both crates expose top-level free
   functions plus a streaming hasher. A future swap (to `gmsm`,
   RustCrypto, or Tongsuo) is a single-file change to
   `crates/wf-sm/src/sm3.rs` because the wf-sm public API is fixed
   (`sm3 / sm3_hex / Sm3::{new,update,finalize}`).

## 4. Measured throughput (laptop baseline)

`cargo test --release -p wf-sm --test sm3_throughput -- --ignored
--nocapture`, Windows 10 x86_64, single thread, target 64 MB hashed
per size:

| input size | iters  | wall-clock | throughput   |
|------------|--------|------------|--------------|
| 1 KB       | 65 536 | 1.092 s    | 58.60 MB/s   |
| 10 KB      | 6 553  | 0.818 s    | 78.23 MB/s   |
| 100 KB     | 655    | 0.839 s    | 76.25 MB/s   |
| 1 MB       | 64     | 1.127 s    | 56.76 MB/s   |

Honesty note: plan-v0.4 § anchor B estimated "~150 MB/s SM3". The
measured numbers come in at roughly half that on this hardware. This
is **measured baseline data**, not a target — Phase 2 has room to
explore SIMD-accelerated SM3 (RustCrypto-style avx2 lane parallelism)
if a downstream consumer demands higher throughput. The 56-78 MB/s
band is sufficient for the WAL signing / report-export workloads the
strategy memo flags as Phase 1 candidates.

## 5. Phase 2 action list

| week | item                                                                 |
|------|----------------------------------------------------------------------|
| 1-2  | Implement SM2 signature / verify wrapper under `wf-sm::sm2`.         |
| 1-2  | Hook SM3 into the WAL footer (signing the per-record checksum chain).|
| 3    | Draft 密评 (GB/T 39786) coverage report skeleton — algorithm OID +   |
|      | implementation lineage + audit trail per record.                     |
| 4    | Pre-research Tongsuo C-FFI path as a Tier-2 fallback for compliance  |
|      | reviewers who reject pure-Rust SM implementations. Decision-point    |
|      | gates Phase 3 enterprise pilots.                                     |

## 6. Risk register & exits

| risk                          | probability | mitigation / exit                                          |
|-------------------------------|-------------|------------------------------------------------------------|
| `smcrypto` stops releasing    | low         | Swap to `gmsm` — API is near-symmetric; single-file change.|
| Compliance reviewer rejects   | medium      | Tier-2 plan: vendor Tongsuo C-FFI behind same wf-sm trait. |
|   pure-Rust SM impl           |             |                                                            |
| Throughput cap blocks consumer| low         | RustCrypto-style SIMD lanes; consumer-pull decision.       |
| Upstream attribution drift    | low         | Caught this round (CrayfishGo vs nfjBill); always grep     |
|   (plan vs crates.io)         |             | crates.io before committing a named dep.                   |
