//! Append-only write-ahead log.
//!
//! The WAL is the durability primitive shared by autosave, undo-stack
//! persistence, and crash-restore. It stores opaque caller-defined byte
//! payloads (e.g. serialized edit ops); semantic meaning is owned by the
//! caller, not by this crate.
//!
//! ## File format
//!
//! ```text
//! +----------------+-----------+-----------+ ... +-----------+
//! | magic 8 bytes  | record 0  | record 1  |     | record N  |
//! +----------------+-----------+-----------+ ... +-----------+
//! ```
//!
//! Magic = `b"WFWAL\x00\x01\n"` (identifier + version byte + newline so
//! `head -c8 wal.log` is visually distinct). The version byte rejects
//! files written by future formats.
//!
//! Each record:
//!
//! ```text
//! +---------------+---------------+--------------------+
//! | payload_len   | crc32         | payload            |
//! | u32 LE        | u32 LE        | payload_len bytes  |
//! +---------------+---------------+--------------------+
//! ```
//!
//! `crc32` covers the payload bytes only (IEEE 802.3 polynomial
//! `0xEDB88320`, init `0xFFFFFFFF`, final xor `0xFFFFFFFF`).
//!
//! ## Crash safety
//!
//! Appending is non-atomic from the OS's view: a record split across a
//! crash can leave a partial header, a truncated payload, or a payload
//! whose CRC fails. On reopen, [`Wal::read_all`] surfaces the first such
//! anomaly via [`TailCorruption`]; the caller decides whether to
//! [`Wal::truncate_to`] the last good offset (recovery) or treat it as a
//! fatal error.
//!
//! Single-file, single-writer. Concurrent writers are not supported and
//! are not detected — wrap with an OS file lock at the caller boundary
//! if multiple processes share a WAL path.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// File identifier + format version (8 bytes). Version byte is `0x01`.
pub const MAGIC: [u8; 8] = *b"WFWAL\x00\x01\n";

/// Bytes of per-record overhead (payload length + CRC32).
pub const RECORD_HEADER_LEN: usize = 8;

/// Hard cap on a single payload to bound recovery-time memory.
/// 16 MiB is generous for edit-op records; bump deliberately if a
/// caller demonstrates a need.
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

// --- errors ---------------------------------------------------------------

#[derive(Debug)]
pub enum WalError {
    Io(io::Error),
    /// File is non-empty but does not start with [`MAGIC`].
    BadMagic {
        found: [u8; 8],
    },
    /// Payload exceeds [`MAX_PAYLOAD`].
    PayloadTooLarge {
        len: usize,
    },
}

impl std::fmt::Display for WalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalError::Io(e) => write!(f, "wal io error: {}", e),
            WalError::BadMagic { found } => {
                write!(f, "wal bad magic: expected {:?}, found {:?}", MAGIC, found)
            }
            WalError::PayloadTooLarge { len } => write!(
                f,
                "wal payload too large: {} bytes exceeds limit {}",
                len, MAX_PAYLOAD
            ),
        }
    }
}

impl std::error::Error for WalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WalError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for WalError {
    fn from(e: io::Error) -> Self {
        WalError::Io(e)
    }
}

/// Why a record at this offset cannot be trusted.
///
/// Tail-only: corruption mid-file (hardware damage) is out of MVP scope;
/// such a file is treated the same as tail corruption and recovery
/// truncates everything past the first bad offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Corruption {
    /// File ended part-way through the 8-byte record header.
    TruncatedHeader { have: usize },
    /// Header parsed, but the file ended before the full payload arrived.
    TruncatedPayload { need: u32, have: u64 },
    /// Header and payload both present, CRC32 over payload did not match.
    Checksum { stored: u32, computed: u32 },
    /// Header declared `payload_len > MAX_PAYLOAD` — treat as garbage.
    HeaderPayloadTooLarge { len: u32 },
}

impl std::fmt::Display for Corruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Corruption::TruncatedHeader { have } => {
                write!(f, "truncated record header: have {}/8 bytes", have)
            }
            Corruption::TruncatedPayload { need, have } => write!(
                f,
                "truncated record payload: declared {} bytes, only {} available",
                need, have
            ),
            Corruption::Checksum { stored, computed } => write!(
                f,
                "record checksum mismatch: stored {:#010x}, computed {:#010x}",
                stored, computed
            ),
            Corruption::HeaderPayloadTooLarge { len } => write!(
                f,
                "record header declares payload_len {} > MAX_PAYLOAD {}",
                len, MAX_PAYLOAD
            ),
        }
    }
}

/// Tail-corruption signal returned by [`Wal::read_all`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailCorruption {
    /// Byte offset where the first unparseable record begins. Truncating
    /// the file to this length drops the corruption and leaves all
    /// records before it intact.
    pub offset: u64,
    pub kind: Corruption,
}

impl std::fmt::Display for TailCorruption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "tail corruption at offset {}: {}",
            self.offset, self.kind
        )
    }
}

// --- Wal ------------------------------------------------------------------

#[derive(Debug)]
pub struct Wal {
    file: std::fs::File,
    path: PathBuf,
    /// Number of bytes currently in the file (header + records).
    /// Tracked locally so `append` does not re-stat per call.
    end_offset: u64,
}

impl Wal {
    /// Open or create the WAL at `path`.
    ///
    /// - File does not exist → created, magic header written, fsynced.
    /// - File exists, starts with [`MAGIC`] → opened append-mode.
    /// - File exists, empty → magic header written.
    /// - File exists, wrong/short magic → [`WalError::BadMagic`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WalError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        let len = file.metadata()?.len();
        let end_offset = if len == 0 {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&MAGIC)?;
            file.sync_data()?;
            MAGIC.len() as u64
        } else {
            let mut header = [0u8; 8];
            file.seek(SeekFrom::Start(0))?;
            // A file shorter than MAGIC is always BadMagic — anything that
            // ever wrote magic would have ≥ 8 bytes.
            let n = read_full_or_short(&mut file, &mut header)?;
            if n != header.len() || header != MAGIC {
                return Err(WalError::BadMagic { found: header });
            }
            len
        };
        // Position writer at EOF so first append lands after existing data.
        file.seek(SeekFrom::End(0))?;
        Ok(Wal {
            file,
            path,
            end_offset,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Total file size, including magic header.
    pub fn end_offset(&self) -> u64 {
        self.end_offset
    }

    /// Append one record. Returns the byte offset of the record's header
    /// (i.e. the offset at which `truncate_to` would erase this record
    /// and everything after it).
    ///
    /// Does **not** fsync — call [`Wal::sync`] when you need durability.
    /// Batching multiple appends under one sync is the throughput knob.
    pub fn append(&mut self, payload: &[u8]) -> Result<u64, WalError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(WalError::PayloadTooLarge { len: payload.len() });
        }
        let len_u32 = payload.len() as u32;
        let crc = crc32(payload);
        // Single buffered write: header + payload concatenated so a
        // partial-write at the OS layer leaves either nothing or a
        // contiguous prefix — easier for tail recovery to reason about.
        let mut buf = Vec::with_capacity(RECORD_HEADER_LEN + payload.len());
        buf.extend_from_slice(&len_u32.to_le_bytes());
        buf.extend_from_slice(&crc.to_le_bytes());
        buf.extend_from_slice(payload);

        let record_offset = self.end_offset;
        self.file.write_all(&buf)?;
        self.end_offset += buf.len() as u64;
        Ok(record_offset)
    }

    /// `fsync(data)` — flush user data + minimum metadata required to
    /// recover. Costs one disk round-trip; batch appends between syncs
    /// where the durability boundary allows.
    pub fn sync(&mut self) -> Result<(), WalError> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Read every record from the start of the file.
    ///
    /// On success returns `(records, tail)`:
    /// - `records` — all records up to (but not including) the first
    ///   corruption point.
    /// - `tail` — `Some` iff a corruption was hit; carries the byte
    ///   offset that the caller can pass to [`Wal::truncate_to`] to
    ///   discard the corrupt tail. `None` means the file was read clean
    ///   end-to-end.
    ///
    /// This is O(file size). Callers replaying large WALs should treat
    /// this as a one-shot crash-recovery primitive, not a hot path.
    pub fn read_all(&mut self) -> Result<(Vec<Vec<u8>>, Option<TailCorruption>), WalError> {
        self.file.seek(SeekFrom::Start(MAGIC.len() as u64))?;
        let mut records = Vec::new();
        let mut cursor = MAGIC.len() as u64;
        let end = self.end_offset;

        loop {
            if cursor == end {
                break;
            }
            // Header
            let remaining = end - cursor;
            if remaining < RECORD_HEADER_LEN as u64 {
                let mut partial = [0u8; RECORD_HEADER_LEN];
                let have = read_full_or_short(&mut self.file, &mut partial[..remaining as usize])?;
                return Ok((
                    records,
                    Some(TailCorruption {
                        offset: cursor,
                        kind: Corruption::TruncatedHeader { have },
                    }),
                ));
            }
            let mut header = [0u8; RECORD_HEADER_LEN];
            self.file.read_exact(&mut header)?;
            let payload_len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
            let stored_crc = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

            if payload_len as usize > MAX_PAYLOAD {
                return Ok((
                    records,
                    Some(TailCorruption {
                        offset: cursor,
                        kind: Corruption::HeaderPayloadTooLarge { len: payload_len },
                    }),
                ));
            }

            // Payload
            let need = payload_len as u64;
            let after_header = cursor + RECORD_HEADER_LEN as u64;
            let payload_available = end - after_header;
            if payload_available < need {
                return Ok((
                    records,
                    Some(TailCorruption {
                        offset: cursor,
                        kind: Corruption::TruncatedPayload {
                            need: payload_len,
                            have: payload_available,
                        },
                    }),
                ));
            }
            let mut payload = vec![0u8; payload_len as usize];
            self.file.read_exact(&mut payload)?;
            let computed = crc32(&payload);
            if computed != stored_crc {
                return Ok((
                    records,
                    Some(TailCorruption {
                        offset: cursor,
                        kind: Corruption::Checksum {
                            stored: stored_crc,
                            computed,
                        },
                    }),
                ));
            }
            records.push(payload);
            cursor = after_header + need;
        }
        // Leave file positioned at end so next append continues cleanly.
        self.file.seek(SeekFrom::End(0))?;
        Ok((records, None))
    }

    /// Drop everything at and after `offset`. The file is shrunk and
    /// fsynced; subsequent appends start at `offset`.
    ///
    /// `offset < MAGIC.len()` is rejected (would destroy the header).
    pub fn truncate_to(&mut self, offset: u64) -> Result<(), WalError> {
        if offset < MAGIC.len() as u64 {
            return Err(WalError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "wal truncate_to: offset would discard magic header",
            )));
        }
        if offset > self.end_offset {
            return Err(WalError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "wal truncate_to: offset beyond current end",
            )));
        }
        self.file.set_len(offset)?;
        self.file.sync_all()?;
        self.file.seek(SeekFrom::End(0))?;
        self.end_offset = offset;
        Ok(())
    }
}

// --- helpers --------------------------------------------------------------

fn read_full_or_short<R: Read>(r: &mut R, buf: &mut [u8]) -> Result<usize, io::Error> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..])? {
            0 => break,
            n => total += n,
        }
    }
    Ok(total)
}

// --- CRC32 (IEEE 802.3, table-driven) -------------------------------------

const CRC32_POLY: u32 = 0xEDB8_8320;

const CRC32_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0u32;
    while i < 256 {
        let mut c = i;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                CRC32_POLY ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        table[i as usize] = c;
        i += 1;
    }
    table
};

/// CRC32 (IEEE 802.3 polynomial, reflected). Public for callers that
/// want to checksum their payloads ahead of time (e.g. to dedupe before
/// appending).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = CRC32_TABLE[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Cross-check against the well-known IEEE-CRC32 of `b"123456789"`,
    /// 0xCBF43926. Detects accidental polynomial / endianness swaps.
    #[test]
    fn crc32_matches_iso_3309_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn crc32_of_empty_is_zero() {
        assert_eq!(crc32(b""), 0);
    }
}
