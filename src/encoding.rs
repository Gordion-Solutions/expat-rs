//! Character encoding detection and decoding (W3C XML 1.0 §4.3.3 and
//! Appendix F).
//!
//! The parser works on `&str`; [`decode`] turns the raw bytes of a document
//! or external entity into one. Supported: UTF-8 (with or without a byte
//! order mark), UTF-16 (little- or big-endian, with a byte order mark, or
//! detected from a leading `<?`), US-ASCII and ISO-8859-1. Any other
//! declared encoding is an [`XmlError::Encoding`] error.

use std::borrow::Cow;

use crate::error::{ErrorKind, Position, Result, XmlError};

fn error(kind: ErrorKind, reason: String) -> XmlError {
    XmlError::Encoding { pos: Position::start(), kind, reason }
}

/// Decode an XML document or external parsed entity to text.
///
/// The encoding comes from the byte order mark, or failing that from the
/// first bytes (Appendix F) and the encoding declaration. A declaration
/// that contradicts the bytes is a fatal error (§4.3.3). A UTF-8 or UTF-16
/// byte order mark is kept as a leading U+FEFF, which the lexer skips.
pub fn decode(bytes: &[u8]) -> Result<Cow<'_, str>> {
    match bytes {
        [0xFE, 0xFF, ..] => decode_utf16(bytes, u16::from_be_bytes),
        [0xFF, 0xFE, ..] => decode_utf16(bytes, u16::from_le_bytes),
        // No byte order mark, but '<?' in UTF-16 (Appendix F).
        [0x00, 0x3C, 0x00, 0x3F, ..] => decode_utf16(bytes, u16::from_be_bytes),
        [0x3C, 0x00, 0x3F, 0x00, ..] => decode_utf16(bytes, u16::from_le_bytes),
        [0x00, 0x00, ..] | [_, 0x00, 0x00, ..] => Err(error(ErrorKind::UnknownEncoding, "UCS-4 input is not supported".into())),
        _ => decode_8bit(bytes),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<Cow<'_, str>> {
    if !bytes.len().is_multiple_of(2) {
        return Err(error(ErrorKind::PartialChar, "UTF-16 input has an odd number of bytes".into()));
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|&pair| unit(pair));
    let text: String = char::decode_utf16(units)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| error(ErrorKind::InvalidToken, format!("invalid UTF-16: {e}")))?;
    check_utf16_declaration(&text)?;
    Ok(Cow::Owned(text))
}

/// UTF-16 text must not declare an 8-bit encoding (§4.3.3).
fn check_utf16_declaration(text: &str) -> Result<()> {
    match declared_encoding(text) {
        Some(enc) if !is_utf16(enc) => Err(error(ErrorKind::IncorrectEncoding, format!("document is UTF-16 but declares encoding {enc:?}"))),
        _ => Ok(()),
    }
}

/// How 8-bit (non-UTF-16) input is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind8 {
    Utf8,
    Ascii,
    Latin1,
}

/// Choose the decoding for 8-bit input from its first bytes: a UTF-8 byte
/// order mark and the encoding declaration, if any.
fn choose_8bit(bytes: &[u8]) -> Result<Kind8> {
    match choose_8bit_or_unknown(bytes)? {
        Ok(kind) => Ok(kind),
        Err(name) => Err(error(ErrorKind::UnknownEncoding, format!("unsupported encoding {name:?}"))),
    }
}

/// As `choose_8bit`, but an encoding name it doesn't support comes back as
/// `Err(name)` for the caller to resolve (see `ByteMap`).
fn choose_8bit_or_unknown(bytes: &[u8]) -> Result<std::result::Result<Kind8, String>> {
    let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    // The declaration itself is ASCII in every supported 8-bit encoding.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    // A name that isn't a valid EncName (§4.3.3 [81]) is a malformed
    // declaration, for the lexer to report; decode as UTF-8 meanwhile.
    let declared = declared_encoding(head.trim_start_matches('\u{FEFF}'))
        .filter(|name| is_enc_name(name))
        .map(str::to_ascii_uppercase);
    match declared.as_deref() {
        None | Some("UTF-8") => Ok(Ok(Kind8::Utf8)),
        Some(enc) if has_bom => Err(error(ErrorKind::IncorrectEncoding, format!("document has a UTF-8 byte order mark but declares encoding {enc:?}"))),
        Some(enc) if is_utf16(enc) => Err(error(ErrorKind::IncorrectEncoding, "UTF-16 documents must begin with a byte order mark".into())),
        Some("US-ASCII" | "ASCII") => Ok(Ok(Kind8::Ascii)),
        Some("ISO-8859-1" | "ISO_8859-1" | "LATIN1") => Ok(Ok(Kind8::Latin1)),
        Some(enc) => Ok(Err(enc.to_string())),
    }
}

fn check_ascii(bytes: &[u8], offset: usize) -> Result<()> {
    match bytes.iter().position(|&b| b >= 0x80) {
        Some(i) => Err(error(ErrorKind::InvalidToken, format!("byte {:#04x} at offset {} is not US-ASCII", bytes[i], offset + i))),
        None => Ok(()),
    }
}

fn decode_8bit(bytes: &[u8]) -> Result<Cow<'_, str>> {
    match choose_8bit(bytes)? {
        Kind8::Utf8 => utf8(bytes),
        Kind8::Ascii => {
            check_ascii(bytes, 0)?;
            utf8(bytes)
        }
        Kind8::Latin1 => Ok(Cow::Owned(bytes.iter().map(|&b| b as char).collect())),
    }
}

fn utf8(bytes: &[u8]) -> Result<Cow<'_, str>> {
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|e| {
            // No error length: the input just stops mid-character.
            let kind = if e.error_len().is_none() { ErrorKind::PartialChar } else { ErrorKind::InvalidToken };
            error(kind, format!("invalid UTF-8 at offset {}", e.valid_up_to()))
        })
}

/// §4.3.3 [Production 81]: EncName ::= [A-Za-z] ([A-Za-z0-9._] | '-')*
fn is_enc_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn is_utf16(enc: &str) -> bool {
    enc.eq_ignore_ascii_case("UTF-16")
}

/// The `encoding` pseudo-attribute of a leading XML or text declaration,
/// if any. Only locates the value; the lexer checks the declaration's
/// syntax properly.
fn declared_encoding(text: &str) -> Option<&str> {
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let decl = &text[..text.strip_prefix("<?xml")?.find("?>")? + 5];
    let after = &decl[decl.find("encoding")? + "encoding".len()..];
    let after = after.trim_start().strip_prefix('=')?.trim_start();
    let quote = after.chars().next().filter(|q| matches!(q, '"' | '\''))?;
    let value = &after[1..];
    Some(&value[..value.find(quote)?])
}

/// A single-byte encoding as a table from byte to character (`None`: the
/// byte is not valid in the encoding). Supplied by the caller for an
/// encoding expat-rs doesn't know, as libexpat's unknown-encoding handler
/// supplies its `map`.
pub type ByteMap = Box<[Option<char>; 256]>;

/// A caller-supplied resolver for unknown encodings: given the encoding's
/// name, a [`ByteMap`] for it, or `None` if it can't be supported.
pub type UnknownEncodingHandler<'l> = Box<dyn FnMut(&str) -> Option<ByteMap> + 'l>;

/// How a [`StreamDecoder`] decodes, once it has seen enough input.
#[derive(Clone)]
enum Kind {
    Bit8(Kind8),
    Utf16(fn([u8; 2]) -> u16),
    Map(ByteMap),
}

/// Incremental version of [`decode`], for input that arrives in chunks: a
/// multi-byte character may be split between chunks, and the encoding is
/// chosen once enough of the start of the input has arrived.
#[derive(Default)]
pub struct StreamDecoder {
    /// Bytes received but not yet decoded.
    pending: Vec<u8>,
    kind: Option<Kind>,
    /// Bytes decoded so far, for error offsets.
    offset: usize,
    /// UTF-16 only: the start of the text, until its declaration (if any)
    /// has been checked.
    utf16_head: Option<String>,
    /// Encoding chosen by the caller, overriding detection and the
    /// document's declaration (libexpat's protocol encoding).
    forced: Option<String>,
}

impl StreamDecoder {
    /// A decoder that hasn't seen any input yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// A decoder that decodes as `encoding` whatever the input declares:
    /// UTF-8, UTF-16 (byte order from a byte order mark, big-endian
    /// without one), UTF-16LE, UTF-16BE, US-ASCII, ISO-8859-1, or any other
    /// name the resolver passed to [`decode_with`](Self::decode_with)
    /// knows. Mirrors libexpat's protocol encoding (`XML_SetEncoding`).
    pub fn with_encoding(encoding: &str) -> Self {
        Self { forced: Some(encoding.to_ascii_uppercase()), ..Self::default() }
    }

    /// Decode `bytes` onto the end of `out`. With `last`, this is the end
    /// of the input and nothing may be left incomplete. Encodings it
    /// doesn't know are errors; see [`decode_with`](Self::decode_with).
    pub fn decode(&mut self, bytes: &[u8], last: bool, out: &mut String) -> Result<()> {
        self.decode_with(bytes, last, out, &mut |_| None)
    }

    /// As [`decode`](Self::decode), asking `unknown` for a [`ByteMap`] when
    /// the declared (or forced) encoding is one expat-rs doesn't know;
    /// `None` makes it an unknown-encoding error. Called at most once.
    pub fn decode_with(
        &mut self,
        bytes: &[u8],
        last: bool,
        out: &mut String,
        unknown: &mut dyn FnMut(&str) -> Option<ByteMap>,
    ) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        if self.kind.is_none() {
            match self.detect(last, unknown)? {
                Some(k) => self.kind = Some(k),
                None => return Ok(()), // need more of the start
            }
        }
        let kind = self.kind.as_ref().expect("set above");
        let start = out.len();
        let used = match kind {
            Kind::Map(map) => {
                for (i, &b) in self.pending.iter().enumerate() {
                    match map[b as usize] {
                        Some(c) => out.push(c),
                        None => return Err(error(ErrorKind::InvalidToken, format!(
                            "byte {b:#04x} at offset {} is not valid in the document's encoding", self.offset + i))),
                    }
                }
                self.pending.len()
            }
            Kind::Bit8(Kind8::Latin1) => {
                out.extend(self.pending.iter().map(|&b| b as char));
                self.pending.len()
            }
            &Kind::Bit8(k) => {
                let valid = match std::str::from_utf8(&self.pending) {
                    Ok(s) => s.len(),
                    Err(e) if e.error_len().is_some() => {
                        return Err(error(ErrorKind::InvalidToken, format!("invalid UTF-8 at offset {}", self.offset + e.valid_up_to())));
                    }
                    Err(e) => e.valid_up_to(), // a character continues in the next chunk
                };
                if k == Kind8::Ascii {
                    check_ascii(&self.pending[..valid], self.offset)?;
                }
                out.push_str(std::str::from_utf8(&self.pending[..valid]).expect("validated above"));
                valid
            }
            &Kind::Utf16(unit) => {
                let mut n = self.pending.len() / 2 * 2;
                // Keep a trailing high surrogate for its partner.
                if n >= 2 && !last && (0xD800..0xDC00).contains(&unit([self.pending[n - 2], self.pending[n - 1]])) {
                    n -= 2;
                }
                let units = self.pending[..n].as_chunks::<2>().0.iter().map(|&pair| unit(pair));
                for c in char::decode_utf16(units) {
                    out.push(c.map_err(|e| error(ErrorKind::InvalidToken, format!("invalid UTF-16: {e}")))?);
                }
                n
            }
        };
        self.pending.drain(..used);
        self.offset += used;
        if last && !self.pending.is_empty() {
            return Err(error(ErrorKind::PartialChar, "input ends in the middle of a character".into()));
        }
        if let Some(head) = &mut self.utf16_head {
            head.push_str(&out[start..]);
            let body = head.strip_prefix('\u{FEFF}').unwrap_or(head);
            let undecided = body.starts_with("<?xml") && !body.contains("?>") && body.len() < 256
                || "<?xml".starts_with(body);
            if !undecided || last {
                check_utf16_declaration(head)?;
                self.utf16_head = None;
            }
        }
        Ok(())
    }

    /// Choose the decoding from the start of the input, or `None` if more
    /// of it is needed first.
    fn detect(&mut self, last: bool, unknown: &mut dyn FnMut(&str) -> Option<ByteMap>) -> Result<Option<Kind>> {
        let p = &self.pending[..];
        let utf16 = |be: bool| -> Kind { Kind::Utf16(if be { u16::from_be_bytes } else { u16::from_le_bytes }) };
        let resolve = |name: &str, unknown: &mut dyn FnMut(&str) -> Option<ByteMap>| {
            unknown(name)
                .map(Kind::Map)
                .ok_or_else(|| error(ErrorKind::UnknownEncoding, format!("unsupported encoding {name:?}")))
        };
        if let Some(forced) = self.forced.clone() {
            // The caller's choice wins over detection and the declaration.
            if p.len() < 2 && !last {
                return Ok(None);
            }
            return Ok(Some(match forced.as_str() {
                "UTF-8" => Kind::Bit8(Kind8::Utf8),
                "US-ASCII" | "ASCII" => Kind::Bit8(Kind8::Ascii),
                "ISO-8859-1" | "ISO_8859-1" | "LATIN1" => Kind::Bit8(Kind8::Latin1),
                "UTF-16LE" => utf16(false),
                "UTF-16BE" => utf16(true),
                "UTF-16" => utf16(!p.starts_with(&[0xFF, 0xFE])),
                other => resolve(other, unknown)?,
            }));
        }
        let kind = match p {
            [0xFE, 0xFF, ..] => utf16(true),
            [0xFF, 0xFE, ..] => utf16(false),
            _ if p.len() < 4 && !last => return Ok(None),
            [0x00, 0x3C, 0x00, 0x3F, ..] => utf16(true),
            [0x3C, 0x00, 0x3F, 0x00, ..] => utf16(false),
            [0x00, 0x00, ..] | [_, 0x00, 0x00, ..] => return Err(error(ErrorKind::UnknownEncoding, "UCS-4 input is not supported".into())),
            _ => {
                let body = p.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(p);
                let has_decl = body.starts_with(b"<?xml");
                let decided = if has_decl {
                    p.windows(2).take(256).any(|w| w == b"?>") || p.len() >= 256
                } else {
                    !b"<?xml".starts_with(body)
                };
                if !decided && !last {
                    return Ok(None);
                }
                match choose_8bit_or_unknown(p)? {
                    Ok(kind) => Kind::Bit8(kind),
                    Err(name) => resolve(&name, unknown)?,
                }
            }
        };
        if let Kind::Utf16(_) = kind {
            self.utf16_head = Some(String::new());
        }
        Ok(Some(kind))
    }
}
