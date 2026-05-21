//! Hex encode / decode helpers. Lifted from wf-cli to avoid a clap-laden
//! dependency. ~30 lines, not worth a shared crate yet.

pub fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

pub fn decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err(format!(
            "hex string has odd length {} — every byte needs 2 hex digits",
            s.len()
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = nibble(bytes[i])?;
        let lo = nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!(
            "non-hex byte {:#04x} ({:?}) — strip non-hex chars first",
            b, b as char
        )),
    }
}

pub fn encode(data: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(data.len() * 2);
    for byte in data {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let raw = b"\x00\x01\xff\x80";
        let h = encode(raw);
        assert_eq!(h, "0001ff80");
        assert_eq!(decode(&h).unwrap(), raw);
    }

    #[test]
    fn rejects_odd_length() {
        assert!(decode("abc").is_err());
    }

    #[test]
    fn rejects_non_hex() {
        assert!(decode("0g").is_err());
    }

    #[test]
    fn strips_whitespace() {
        assert_eq!(strip_whitespace("ab cd\nef"), "abcdef");
    }
}
