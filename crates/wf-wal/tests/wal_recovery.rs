//! End-to-end tests for [`wf_wal::Wal`].
//!
//! Each test owns its own temp file (cleaned up on drop) so they can run
//! concurrently without crosstalk. Crash-equivalent scenarios are
//! produced by [`std::fs::OpenOptions`] direct writes, not by killing a
//! process — same on-disk shape, deterministic, fast.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use wf_wal::{Corruption, Wal, WalError, MAGIC, MAX_PAYLOAD, RECORD_HEADER_LEN};

struct TempPath(PathBuf);
impl TempPath {
    fn new(name: &str) -> Self {
        let mut p = std::env::temp_dir();
        let pid = std::process::id();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("wf-wal-test-{}-{}-{}", pid, nanos, name));
        TempPath(p)
    }
    fn as_path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// --- happy paths ----------------------------------------------------------

#[test]
fn open_creates_file_with_magic_header() {
    let tmp = TempPath::new("magic-header");
    let wal = Wal::open(tmp.as_path()).unwrap();
    assert_eq!(wal.end_offset(), MAGIC.len() as u64);
    let on_disk = std::fs::read(tmp.as_path()).unwrap();
    assert_eq!(on_disk, MAGIC);
}

#[test]
fn append_then_read_all_returns_payloads_in_order() {
    let tmp = TempPath::new("ordered-roundtrip");
    let mut wal = Wal::open(tmp.as_path()).unwrap();
    wal.append(b"alpha").unwrap();
    wal.append(b"bravo").unwrap();
    wal.append(b"charlie-zeta").unwrap();
    wal.sync().unwrap();

    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(tail, None, "clean WAL must report no tail corruption");
    let as_str: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
    assert_eq!(
        as_str,
        vec![&b"alpha"[..], &b"bravo"[..], &b"charlie-zeta"[..]]
    );
}

#[test]
fn empty_payloads_are_legal_records() {
    let tmp = TempPath::new("empty-payload");
    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let off0 = wal.append(b"").unwrap();
    let off1 = wal.append(b"after-empty").unwrap();
    assert_eq!(off0, MAGIC.len() as u64);
    assert_eq!(off1, off0 + RECORD_HEADER_LEN as u64);

    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(tail, None);
    assert_eq!(records.len(), 2);
    assert!(records[0].is_empty());
    assert_eq!(records[1].as_slice(), b"after-empty");
}

#[test]
fn reopen_preserves_previously_appended_records() {
    let tmp = TempPath::new("reopen");
    {
        let mut wal = Wal::open(tmp.as_path()).unwrap();
        wal.append(b"persistent").unwrap();
        wal.sync().unwrap();
    }
    let mut wal2 = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal2.read_all().unwrap();
    assert_eq!(tail, None);
    assert_eq!(records, vec![b"persistent".to_vec()]);

    wal2.append(b"after-reopen").unwrap();
    wal2.sync().unwrap();
    let (records, _) = wal2.read_all().unwrap();
    assert_eq!(
        records,
        vec![b"persistent".to_vec(), b"after-reopen".to_vec()]
    );
}

// --- crash-equivalent scenarios ------------------------------------------

#[test]
fn truncated_header_tail_is_reported() {
    let tmp = TempPath::new("truncated-header");
    {
        let mut wal = Wal::open(tmp.as_path()).unwrap();
        wal.append(b"good-record").unwrap();
        wal.sync().unwrap();
    }
    // Simulate a crash mid-header: append 3 garbage bytes (less than
    // the 8-byte record header) to the end.
    let mut raw = OpenOptions::new().append(true).open(tmp.as_path()).unwrap();
    raw.write_all(&[0xFF, 0xFF, 0xFF]).unwrap();
    drop(raw);

    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(records, vec![b"good-record".to_vec()]);
    let tail = tail.expect("partial header must surface");
    assert_eq!(
        tail.offset,
        MAGIC.len() as u64 + RECORD_HEADER_LEN as u64 + 11
    );
    assert_eq!(tail.kind, Corruption::TruncatedHeader { have: 3 });
}

#[test]
fn truncated_payload_tail_is_reported() {
    let tmp = TempPath::new("truncated-payload");
    {
        let mut wal = Wal::open(tmp.as_path()).unwrap();
        wal.append(b"good").unwrap();
        wal.sync().unwrap();
    }
    // Append a full 8-byte header claiming a 100-byte payload, but
    // only write 5 bytes of payload — partial payload tail.
    let mut raw = OpenOptions::new().append(true).open(tmp.as_path()).unwrap();
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&100u32.to_le_bytes());
    hdr.extend_from_slice(&0u32.to_le_bytes());
    raw.write_all(&hdr).unwrap();
    raw.write_all(&[0xAA; 5]).unwrap();
    drop(raw);

    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(records, vec![b"good".to_vec()]);
    let tail = tail.expect("partial payload must surface");
    assert!(matches!(
        tail.kind,
        Corruption::TruncatedPayload { need: 100, have: 5 }
    ));
}

#[test]
fn corrupted_checksum_tail_is_reported() {
    let tmp = TempPath::new("bad-crc");
    let bad_offset;
    {
        let mut wal = Wal::open(tmp.as_path()).unwrap();
        wal.append(b"good").unwrap();
        bad_offset = wal.append(b"will-bitflip").unwrap();
        wal.sync().unwrap();
    }
    // Flip one payload byte in the 2nd record (offset = bad_offset
    // + 8-byte header + 0).
    let mut raw = OpenOptions::new()
        .read(true)
        .write(true)
        .open(tmp.as_path())
        .unwrap();
    let payload_byte_offset = bad_offset + RECORD_HEADER_LEN as u64;
    raw.seek(SeekFrom::Start(payload_byte_offset)).unwrap();
    raw.write_all(b"X").unwrap(); // flips 'w' → 'X'
    drop(raw);

    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(records, vec![b"good".to_vec()]);
    let tail = tail.expect("bit-flip must surface");
    assert_eq!(tail.offset, bad_offset);
    assert!(matches!(tail.kind, Corruption::Checksum { .. }));
}

#[test]
fn header_payload_too_large_is_reported_not_oomed() {
    let tmp = TempPath::new("header-too-large");
    {
        let _ = Wal::open(tmp.as_path()).unwrap();
    }
    // Write a fake header claiming a payload size beyond MAX_PAYLOAD.
    // Recovery must reject this without trying to allocate the full
    // declared payload buffer.
    let mut raw = OpenOptions::new().append(true).open(tmp.as_path()).unwrap();
    let bogus = (MAX_PAYLOAD as u32).wrapping_add(1);
    raw.write_all(&bogus.to_le_bytes()).unwrap();
    raw.write_all(&0u32.to_le_bytes()).unwrap();
    drop(raw);

    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert!(records.is_empty());
    let tail = tail.expect("oversize header must surface");
    assert!(matches!(
        tail.kind,
        Corruption::HeaderPayloadTooLarge { .. }
    ));
}

// --- recovery actions ----------------------------------------------------

#[test]
fn truncate_to_drops_corrupt_tail_and_restores_append() {
    let tmp = TempPath::new("truncate-recover");
    {
        let mut wal = Wal::open(tmp.as_path()).unwrap();
        wal.append(b"first").unwrap();
        wal.append(b"second").unwrap();
        wal.sync().unwrap();
    }
    // Crash mid-header on a third would-be record.
    let mut raw = OpenOptions::new().append(true).open(tmp.as_path()).unwrap();
    raw.write_all(&[0xFF, 0xFF]).unwrap();
    drop(raw);

    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(records.len(), 2);
    let tail = tail.expect("partial header must surface");
    wal.truncate_to(tail.offset).unwrap();

    // After truncation the next append must succeed and read_all must
    // be clean again.
    wal.append(b"third").unwrap();
    wal.sync().unwrap();
    let (records, tail) = wal.read_all().unwrap();
    assert_eq!(tail, None, "post-truncate WAL must be clean");
    assert_eq!(
        records,
        vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
    );
}

#[test]
fn truncate_to_rejects_offset_below_magic_header() {
    let tmp = TempPath::new("truncate-into-magic");
    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let err = wal.truncate_to(0).expect_err("must reject magic erasure");
    let WalError::Io(io_err) = err else {
        panic!("expected Io error guarding the magic header");
    };
    assert_eq!(io_err.kind(), std::io::ErrorKind::InvalidInput);
}

// --- input validation ----------------------------------------------------

#[test]
fn append_rejects_payload_above_limit() {
    let tmp = TempPath::new("payload-too-large");
    let mut wal = Wal::open(tmp.as_path()).unwrap();
    let oversized = vec![0u8; MAX_PAYLOAD + 1];
    let err = wal.append(&oversized).expect_err("must reject");
    assert!(matches!(err, WalError::PayloadTooLarge { .. }));
}

#[test]
fn open_rejects_file_with_wrong_magic() {
    let tmp = TempPath::new("bad-magic");
    std::fs::write(tmp.as_path(), b"NOTWAL\x00\x01").unwrap();
    let err = Wal::open(tmp.as_path()).expect_err("must reject");
    assert!(matches!(err, WalError::BadMagic { .. }));
}

#[test]
fn open_rejects_file_shorter_than_magic_when_non_empty() {
    let tmp = TempPath::new("short-magic");
    std::fs::write(tmp.as_path(), b"WF").unwrap();
    let err = Wal::open(tmp.as_path()).expect_err("must reject");
    assert!(matches!(err, WalError::BadMagic { .. }));
}
