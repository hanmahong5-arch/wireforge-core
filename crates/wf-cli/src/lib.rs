//! Testable core for the `wf` binary.
//!
//! Pure entry points the binary calls:
//! - [`parse_to_tree`] — ISO 8583 hex -> human-readable field tree
//! - [`parse_to_json`] — ISO 8583 hex -> JSON description
//! - [`build_from_json`] — JSON description -> ISO 8583 wire hex
//! - [`swift_parse_to_tree`] — SWIFT MT text -> human-readable block tree
//! - [`swift_parse_to_json`] — SWIFT MT text -> JSON block description

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wf_bitmap::Bitmap8583;
use wf_codec::iso8583::{
    build,
    field::{field_def, DataType, FieldDef, LengthSpec},
    parse, BuildError, Iso8583Message, ParseError,
};
use wf_codec::swift::{parse as swift_parse, Block as SwiftBlock, MtMessage as SwiftMessage};

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

pub fn parse_to_tree(hex_input: &str) -> Result<String, String> {
    let bytes = hex_decode(&strip_whitespace(hex_input))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {}", display_parse(&e)))?;
    Ok(render_tree(&msg))
}

pub fn parse_to_json(hex_input: &str) -> Result<String, String> {
    let bytes = hex_decode(&strip_whitespace(hex_input))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {}", display_parse(&e)))?;
    let view = build_view(&msg)?;
    serde_json::to_string_pretty(&view).map_err(|e| format!("serialize json: {e}"))
}

pub fn swift_parse_to_tree(text: &str) -> Result<String, String> {
    let msg = swift_parse(text).map_err(|e| format!("swift parse: {e}"))?;
    Ok(render_swift_tree(&msg))
}

pub fn swift_parse_to_json(text: &str) -> Result<String, String> {
    let msg = swift_parse(text).map_err(|e| format!("swift parse: {e}"))?;
    let view = build_swift_view(&msg);
    serde_json::to_string_pretty(&view).map_err(|e| format!("serialize json: {e}"))
}

pub fn build_from_json(json_input: &str) -> Result<String, String> {
    let input: BuildInput =
        serde_json::from_str(json_input).map_err(|e| format!("parse json input: {e}"))?;
    if input.mti.len() != 4 || !input.mti.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "mti must be exactly 4 ASCII digits, got {:?}",
            input.mti
        ));
    }
    let mut mti = [0u8; 4];
    mti.copy_from_slice(input.mti.as_bytes());
    let mut fields = BTreeMap::new();
    for (n, value) in input.fields {
        let bytes = match value {
            FieldValue::Ascii(s) => s.into_bytes(),
            FieldValue::Hex { hex } => {
                hex_decode(&hex).map_err(|e| format!("field {n} hex: {e}"))?
            }
        };
        fields.insert(n, bytes);
    }
    let msg = Iso8583Message { mti, fields };
    let wire = build(&msg).map_err(|e| format!("build: {}", display_build(&e)))?;
    Ok(hex_encode(&wire))
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BuildInput {
    mti: String,
    #[serde(default)]
    fields: BTreeMap<u8, FieldValue>,
}

/// Field value: plain string = ASCII bytes; `{ "hex": "..." }` = hex-decoded
/// bytes. The latter is needed for truly binary fields (PIN block, MAC).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FieldValue {
    Ascii(String),
    Hex { hex: String },
}

#[derive(Debug, Serialize)]
struct MessageView {
    mti: String,
    bitmap_hex: String,
    has_secondary: bool,
    fields: Vec<FieldView>,
}

#[derive(Debug, Serialize)]
struct FieldView {
    number: u8,
    name: &'static str,
    type_desc: String,
    length: usize,
    value_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_ascii: Option<String>,
}

fn build_view(msg: &Iso8583Message) -> Result<MessageView, String> {
    let bitmap_hex = encode_bitmap_for_display(msg)?;
    let has_secondary = msg.fields.keys().any(|n| *n > 64);
    let mut fields = Vec::with_capacity(msg.fields.len());
    for (number, data) in &msg.fields {
        fields.push(field_view(*number, data));
    }
    Ok(MessageView {
        mti: String::from_utf8_lossy(&msg.mti).into_owned(),
        bitmap_hex,
        has_secondary,
        fields,
    })
}

fn field_view(number: u8, data: &[u8]) -> FieldView {
    let def = field_def(number);
    let name = def.map_or("Unknown", |d| d.name);
    let type_desc = def.map_or_else(|| "?".to_string(), describe_field);
    let value_ascii = if data.iter().all(byte_is_printable) {
        Some(String::from_utf8_lossy(data).into_owned())
    } else {
        None
    };
    FieldView {
        number,
        name,
        type_desc,
        length: data.len(),
        value_hex: hex_encode(data),
        value_ascii,
    }
}

// ---------------------------------------------------------------------------
// Tree rendering
// ---------------------------------------------------------------------------

fn render_tree(msg: &Iso8583Message) -> String {
    let mut out = String::new();
    out.push_str("ISO 8583 Message\n");
    out.push_str(&format!("├── MTI: {}\n", String::from_utf8_lossy(&msg.mti)));
    let bitmap_hex = encode_bitmap_for_display(msg).unwrap_or_else(|_| "<error>".to_string());
    out.push_str(&format!("├── Bitmap: {}\n", bitmap_hex));
    let fields_set: Vec<String> = msg.fields.keys().map(|n| n.to_string()).collect();
    if fields_set.is_empty() {
        out.push_str("│   └── (no fields)\n");
    } else {
        out.push_str(&format!("│   └── Fields set: {}\n", fields_set.join(", ")));
    }
    out.push_str("└── Fields:\n");
    let n_total = msg.fields.len();
    for (i, (number, data)) in msg.fields.iter().enumerate() {
        let last = i + 1 == n_total;
        let branch = if last { "└──" } else { "├──" };
        let cont = if last { "    " } else { "│   " };
        let def = field_def(*number);
        let name = def.map_or("Unknown", |d| d.name);
        let type_desc = def.map_or_else(|| "?".to_string(), describe_field);
        out.push_str(&format!(
            "    {} [{:>3}] {} — {} ({} bytes)\n",
            branch,
            number,
            name,
            type_desc,
            data.len()
        ));
        let value = if data.iter().all(byte_is_printable) {
            format!("{:?}", String::from_utf8_lossy(data))
        } else {
            format!("hex:{}", hex_encode(data))
        };
        out.push_str(&format!("    {}    {}\n", cont, value));
    }
    out
}

fn describe_field(def: &FieldDef) -> String {
    let dt = data_type_short(def.data_type);
    match def.length {
        LengthSpec::Fixed(n) => format!("{dt}{n} fixed"),
        LengthSpec::LLVAR { max } => format!("LLVAR {dt}..{max}"),
        LengthSpec::LLLVAR { max } => format!("LLLVAR {dt}..{max}"),
    }
}

fn data_type_short(t: DataType) -> &'static str {
    match t {
        DataType::Numeric => "n",
        DataType::Alpha => "a",
        DataType::Special => "s",
        DataType::AlphaNumeric => "an",
        DataType::AlphaSpecial => "as",
        DataType::NumericSpecial => "ns",
        DataType::AlphaNumericSpecial => "ans",
        DataType::Binary => "b",
        DataType::Track => "z",
    }
}

fn encode_bitmap_for_display(msg: &Iso8583Message) -> Result<String, String> {
    let mut bm = Bitmap8583::new();
    for n in msg.fields.keys() {
        bm.set(u16::from(*n))
            .map_err(|e| format!("bitmap set field {n}: {e:?}"))?;
    }
    Ok(hex_encode(&bm.encode()))
}

// ---------------------------------------------------------------------------
// Error display
// ---------------------------------------------------------------------------

fn display_parse(e: &ParseError) -> String {
    match e {
        ParseError::InsufficientBytes { offset, need } => format!(
            "insufficient bytes at offset {offset}; need {need} more — check the input is complete and unencoded"
        ),
        ParseError::InvalidMti(b) => format!(
            "MTI must be 4 ASCII digits, got {:?} (hex {})",
            String::from_utf8_lossy(b),
            hex_encode(b)
        ),
        ParseError::BitmapError(b) => format!("bitmap: {b:?}"),
        ParseError::UnknownField(n) => format!("bitmap set unknown field {n} (must be 1..=128)"),
        ParseError::InvalidLengthPrefix { field, bytes } => format!(
            "field {field} length prefix not ASCII digits: hex {}",
            hex_encode(bytes)
        ),
        ParseError::LengthExceedsMax { field, decoded, max } => {
            format!("field {field} length {decoded} > spec max {max}")
        }
        ParseError::TrailingBytes { remaining } => format!(
            "{remaining} unexpected byte(s) after last field — message has trailing data"
        ),
        ParseError::InvalidBitmapHex { offset, byte } => format!(
            "FullAscii bitmap: byte {byte:#x} at offset {offset} is not a hex digit"
        ),
    }
}

fn display_build(e: &BuildError) -> String {
    match e {
        BuildError::InvalidMti(b) => format!(
            "MTI must be 4 ASCII digits, got {:?}",
            String::from_utf8_lossy(b)
        ),
        BuildError::InvalidFieldNumber(n) => format!("field number {n} out of range (1..=128)"),
        BuildError::UnknownField(n) => format!("no spec for field {n}"),
        BuildError::FixedLengthMismatch {
            field,
            expected,
            actual,
        } => format!("field {field}: fixed length expected {expected} bytes, got {actual}"),
        BuildError::LengthExceedsMax { field, actual, max } => {
            format!("field {field}: payload {actual} > spec max {max}")
        }
        BuildError::LengthOverflow {
            field,
            actual,
            prefix_digits,
        } => format!("field {field}: length {actual} cannot fit in {prefix_digits} digit prefix"),
        BuildError::BitmapError(b) => format!("bitmap: {b:?}"),
    }
}

// ---------------------------------------------------------------------------
// Hex + byte helpers
// ---------------------------------------------------------------------------

fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
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

fn hex_encode(data: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(data.len() * 2);
    for byte in data {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn byte_is_printable(b: &u8) -> bool {
    b.is_ascii_graphic() || *b == b' '
}

// ---------------------------------------------------------------------------
// SWIFT MT rendering
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct SwiftMessageView {
    blocks: Vec<SwiftBlockView>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum SwiftBlockView {
    Raw {
        id: u8,
        value: String,
    },
    Text {
        id: u8,
        fields: Vec<SwiftFieldView>,
    },
    Tagged {
        id: u8,
        entries: Vec<SwiftFieldView>,
    },
}

#[derive(Debug, Serialize)]
struct SwiftFieldView {
    tag: String,
    value: String,
}

fn build_swift_view(msg: &SwiftMessage) -> SwiftMessageView {
    let blocks = msg
        .blocks
        .iter()
        .map(|(id, block)| match block {
            SwiftBlock::Raw(s) => SwiftBlockView::Raw {
                id: *id,
                value: s.clone(),
            },
            SwiftBlock::Text(fs) => SwiftBlockView::Text {
                id: *id,
                fields: fs
                    .iter()
                    .map(|f| SwiftFieldView {
                        tag: f.tag.clone(),
                        value: f.value.clone(),
                    })
                    .collect(),
            },
            SwiftBlock::Tagged(subs) => SwiftBlockView::Tagged {
                id: *id,
                entries: subs
                    .iter()
                    .map(|s| SwiftFieldView {
                        tag: s.tag.clone(),
                        value: s.value.clone(),
                    })
                    .collect(),
            },
        })
        .collect();
    SwiftMessageView { blocks }
}

fn render_swift_tree(msg: &SwiftMessage) -> String {
    let mut out = String::new();
    out.push_str("SWIFT MT Message\n");
    let n_blocks = msg.blocks.len();
    for (i, (id, block)) in msg.blocks.iter().enumerate() {
        let last_block = i + 1 == n_blocks;
        let branch = if last_block { "└──" } else { "├──" };
        let cont = if last_block { "    " } else { "│   " };
        match block {
            SwiftBlock::Raw(s) => {
                out.push_str(&format!("{branch} Block {id} (raw): {s:?}\n"));
            }
            SwiftBlock::Text(fields) => {
                out.push_str(&format!(
                    "{branch} Block {id} (text, {} fields)\n",
                    fields.len()
                ));
                let n_fields = fields.len();
                for (j, f) in fields.iter().enumerate() {
                    let last = j + 1 == n_fields;
                    let fbranch = if last { "└──" } else { "├──" };
                    let preview = preview_value(&f.value);
                    out.push_str(&format!("{cont}{fbranch} :{}:  {}\n", f.tag, preview));
                }
            }
            SwiftBlock::Tagged(subs) => {
                out.push_str(&format!(
                    "{branch} Block {id} (tagged, {} entries)\n",
                    subs.len()
                ));
                let n_subs = subs.len();
                for (j, s) in subs.iter().enumerate() {
                    let last = j + 1 == n_subs;
                    let fbranch = if last { "└──" } else { "├──" };
                    out.push_str(&format!("{cont}{fbranch} {{{}: {}}}\n", s.tag, s.value));
                }
            }
        }
    }
    out
}

/// Truncate a multi-line field value to a one-line preview for tree
/// rendering. Embedded newlines become `\\n` so the tree stays aligned;
/// values over 60 chars are tail-clipped to keep terminal lines bounded.
fn preview_value(value: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect();
    if escaped.chars().count() > 60 {
        let head: String = escaped.chars().take(57).collect();
        format!("{head}...")
    } else {
        escaped
    }
}
