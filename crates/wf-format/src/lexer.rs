//! Lexer for the `.wf` flat-file format.
//!
//! The lexer is a hand-rolled byte-at-a-time scanner. Two design notes:
//!
//! 1. **Mode-switched output.** Most tokens (`{`, `}`, `:`, identifiers,
//!    numbers, newlines) are emitted by [`Lexer::next_token`]. After
//!    seeing `:`, the parser switches to value mode and calls
//!    [`Lexer::read_value_until_newline`] which returns the trimmed
//!    rest-of-line as the value. This avoids ambiguity around values
//!    that contain `:` themselves (e.g. SWIFT `:32A:` tags inside an
//!    embedded SWIFT block).
//!
//! 2. **Comment handling is upstream.** Block comments (`/* … */`) are
//!    stripped during a single pre-pass before the byte stream reaches
//!    the lexer; line comments (`//`) are handled by the value-mode
//!    reader and by skipping comment-only lines in [`Lexer::skip_trivia`].
//!    This keeps the per-token loop simple.

use core::fmt;

/// Coarse tokens the parser drives off. See [`Lexer::next_token`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// A run of `[A-Za-z_][A-Za-z0-9_-]*`. Used for block names
    /// (`meta`, `iso8583`, `swift-mt`) and key names.
    Ident(String),
    /// A run of ASCII digits, e.g. `"4"` in `field 4:`. Distinct from
    /// `Ident` so the parser can validate that `field <N>` has a
    /// numeric argument without re-parsing.
    Number(String),
    /// Single `:`. Triggers the parser's value mode.
    Colon,
    /// Single `{`.
    LBrace,
    /// Single `}`.
    RBrace,
    /// Logical line end. Emitted after each non-empty content line so
    /// the parser can require entries to live on their own line —
    /// useful for catching `a: x b: y` style mistakes that would
    /// otherwise silently merge into one entry.
    Newline,
    /// End of input.
    Eof,
}

/// Lexer / parser errors. Offsets are byte indices into the original
/// (un-comment-stripped) input string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// Encountered a character that doesn't fit any structural token
    /// (and isn't whitespace or a comment marker).
    UnexpectedChar { offset: usize, ch: char },
    /// `/*` opened a block comment that never closed.
    UnterminatedBlockComment { offset: usize },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnexpectedChar { offset, ch } => {
                write!(f, "unexpected character {ch:?} at offset {offset}")
            }
            LexError::UnterminatedBlockComment { offset } => {
                write!(
                    f,
                    "unterminated /* */ block comment starting at offset {offset}"
                )
            }
        }
    }
}

impl std::error::Error for LexError {}

/// Streaming lexer over a `.wf` source string.
pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    pending_newline: bool,
}

impl fmt::Debug for Lexer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lexer")
            .field("pos", &self.pos)
            .field("remaining", &self.bytes.len().saturating_sub(self.pos))
            .finish()
    }
}

impl<'a> Lexer<'a> {
    /// Create a new lexer over an already-block-comment-stripped source.
    /// Block comments must be removed by [`strip_block_comments`] before
    /// reaching this constructor — the lexer itself only handles line
    /// (`//`) comments.
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            pending_newline: false,
        }
    }

    /// Byte offset of the next un-consumed byte.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Pull the next structural token. Skips whitespace, line comments,
    /// and emits one `Tok::Newline` between non-empty lines.
    pub fn next_token(&mut self) -> Result<Tok, LexError> {
        if self.pending_newline {
            self.pending_newline = false;
            return Ok(Tok::Newline);
        }
        self.skip_inline_trivia();
        if self.pos >= self.bytes.len() {
            return Ok(Tok::Eof);
        }
        let b = self.bytes[self.pos];
        match b {
            b'{' => {
                self.pos += 1;
                Ok(Tok::LBrace)
            }
            b'}' => {
                self.pos += 1;
                Ok(Tok::RBrace)
            }
            b':' => {
                self.pos += 1;
                Ok(Tok::Colon)
            }
            b'\n' | b'\r' => {
                self.consume_line_break();
                Ok(Tok::Newline)
            }
            b'/' if self.peek(1) == Some(b'/') => {
                self.skip_line_comment();
                // Recurse — line comments don't themselves emit tokens;
                // whatever follows determines the next token.
                self.next_token()
            }
            c if is_ident_start(c) => Ok(self.lex_ident()),
            c if c.is_ascii_digit() => Ok(self.lex_number_or_alnum()),
            c => Err(LexError::UnexpectedChar {
                offset: self.pos,
                ch: c as char,
            }),
        }
    }

    /// Value-mode reader. After the parser consumes a `Colon` it calls
    /// this to capture the rest of the line as the entry's value.
    ///
    /// Termination rules (first match wins):
    ///
    /// - `\n` / `\r\n` — consumed; a `Tok::Newline` is queued so the
    ///   parser sees end-of-entry.
    /// - `//` — line-comment start; the comment is skipped and the
    ///   value ends just before it (trailing whitespace already
    ///   trimmed).
    /// - Unbalanced `}` (at brace depth 0) — left pending so the
    ///   enclosing block-parser sees the close.
    /// - End of input — equivalent to a terminating `\n`.
    ///
    /// Brace tracking is shallow: each `{` inside the value increments
    /// a depth counter; each `}` while depth > 0 decrements it. This
    /// allows embedded SWIFT sub-blocks like `{108:REF}` to round-trip
    /// inside a `block 3: ...` value without prematurely closing the
    /// outer block.
    ///
    /// Leading whitespace is skipped; trailing whitespace is trimmed.
    pub fn read_value_until_newline(&mut self) -> String {
        // Skip leading whitespace within the value.
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let start = self.pos;
        let mut end = start;
        let mut depth: i32 = 0;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'\n' || b == b'\r' {
                break;
            }
            if b == b'}' && depth == 0 {
                break;
            }
            if b == b'{' {
                depth += 1;
            } else if b == b'}' {
                depth -= 1;
            }
            // Inline line comment kills the rest of the line.
            if b == b'/' && self.peek(1) == Some(b'/') {
                self.skip_line_comment();
                break;
            }
            self.pos += 1;
            end = self.pos;
        }
        // Trim trailing whitespace.
        while end > start && matches!(self.bytes[end - 1], b' ' | b'\t') {
            end -= 1;
        }
        let value = self.src[start..end].to_string();
        // Consume the line break (if any) and request a Newline next
        // so the parser sees end-of-entry. A `}` terminator is left in
        // place — the enclosing block parser handles it on its next
        // `next_token` call.
        if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'\n' | b'\r') {
            self.consume_line_break();
            self.pending_newline = true;
        }
        value
    }

    fn lex_ident(&mut self) -> Tok {
        let start = self.pos;
        while self.pos < self.bytes.len() && is_ident_cont(self.bytes[self.pos]) {
            self.pos += 1;
        }
        Tok::Ident(self.src[start..self.pos].to_string())
    }

    /// Lex a digit-led identifier-or-number token. Examples:
    ///
    /// - `999` → `Number("999")`
    /// - `32A` → `Ident("32A")` (SWIFT tag — digit prefix + alpha
    ///   suffix is a single arg token)
    /// - `4_test` → `Ident("4_test")`
    ///
    /// Resolution rule: if every consumed byte is an ASCII digit,
    /// return `Number`; otherwise return `Ident`. Both variants are
    /// accepted by the parser in the arg position.
    fn lex_number_or_alnum(&mut self) -> Tok {
        let start = self.pos;
        while self.pos < self.bytes.len() && is_ident_cont(self.bytes[self.pos]) {
            self.pos += 1;
        }
        let s = &self.src[start..self.pos];
        if s.bytes().all(|b| b.is_ascii_digit()) {
            Tok::Number(s.to_string())
        } else {
            Tok::Ident(s.to_string())
        }
    }

    fn skip_inline_trivia(&mut self) {
        loop {
            while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\t') {
                self.pos += 1;
            }
            if self.pos + 1 < self.bytes.len()
                && self.bytes[self.pos] == b'/'
                && self.bytes[self.pos + 1] == b'/'
            {
                self.skip_line_comment();
                continue;
            }
            break;
        }
    }

    fn skip_line_comment(&mut self) {
        // Caller has verified bytes[pos..pos+2] == b"//".
        self.pos += 2;
        while self.pos < self.bytes.len() && !matches!(self.bytes[self.pos], b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn consume_line_break(&mut self) {
        // \r\n is consumed as one logical line break.
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\r' {
            self.pos += 1;
            if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
                self.pos += 1;
            }
        } else if self.pos < self.bytes.len() && self.bytes[self.pos] == b'\n' {
            self.pos += 1;
        }
    }

    fn peek(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_ident_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// Strip `/* ... */` block comments from the input, replacing each one
/// with a single space so the surrounding tokens stay separated. Nested
/// block comments are NOT supported — `/* /* */ */` will close at the
/// first `*/` and leave ` */` as a stray fragment that the lexer will
/// surface as an `UnexpectedChar`.
pub fn strip_block_comments(input: &str) -> Result<String, LexError> {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            loop {
                if i + 1 >= bytes.len() {
                    return Err(LexError::UnterminatedBlockComment { offset: start });
                }
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    out.push(' ');
                    break;
                }
                // Preserve newlines inside the comment so line numbers
                // line up in subsequent error messages.
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}
