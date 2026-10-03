//! Character encoding detection and decoding (W3C XML 1.0 §4.3.3 and
//! Appendix F).
//!
//! The parser works on `&str`; [`decode`] turns the raw bytes of a document
//! or external entity into one. Supported: UTF-8 (with or without a byte
//! order mark), UTF-16 (little- or big-endian, with a byte order mark, or
//! detected from a leading `<?`), US-ASCII and ISO-8859-1. Any other
//! declared encoding is an [`XmlError::Encoding`] error.

use std::borrow::Cow;

use crate::error::{Position, Result, XmlError};

fn error(reason: String) -> XmlError {
    XmlError::Encoding { pos: Position::start(), reason }
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
        [0x00, 0x00, ..] | [_, 0x00, 0x00, ..] => Err(error("UCS-4 input is not supported".into())),
        _ => decode_8bit(bytes),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Result<Cow<'_, str>> {
    if !bytes.len().is_multiple_of(2) {
        return Err(error("UTF-16 input has an odd number of bytes".into()));
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|&pair| unit(pair));
    let text: String = char::decode_utf16(units)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| error(format!("invalid UTF-16: {e}")))?;
    check_utf16_declaration(&text)?;
    Ok(Cow::Owned(text))
}

/// UTF-16 text must not declare an 8-bit encoding (§4.3.3).
fn check_utf16_declaration(text: &str) -> Result<()> {
    match declared_encoding(text) {
        Some(enc) if !is_utf16(enc) => Err(error(format!("document is UTF-16 but declares encoding {enc:?}"))),
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
    let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    // The declaration itself is ASCII in every supported 8-bit encoding.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    let declared = declared_encoding(head.trim_start_matches('\u{FEFF}')).map(str::to_ascii_uppercase);
    match declared.as_deref() {
        None | Some("UTF-8") => Ok(Kind8::Utf8),
        Some(enc) if has_bom => Err(error(format!("document has a UTF-8 byte order mark but declares encoding {enc:?}"))),
        Some(enc) if is_utf16(enc) => Err(error("UTF-16 documents must begin with a byte order mark".into())),
        Some("US-ASCII" | "ASCII") => Ok(Kind8::Ascii),
        Some("ISO-8859-1" | "ISO_8859-1" | "LATIN1") => Ok(Kind8::Latin1),
        Some(enc) => Err(error(format!("unsupported encoding {enc:?}"))),
    }
}

fn check_ascii(bytes: &[u8], offset: usize) -> Result<()> {
    match bytes.iter().position(|&b| b >= 0x80) {
        Some(i) => Err(error(format!("byte {:#04x} at offset {} is not US-ASCII", bytes[i], offset + i))),
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
        .map_err(|e| error(format!("invalid UTF-8 at offset {}", e.valid_up_to())))
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

/// How a [`StreamDecoder`] decodes, once it has seen enough input.
#[derive(Clone, Copy)]
enum Kind {
    Bit8(Kind8),
    Utf16(fn([u8; 2]) -> u16),
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
}

impl StreamDecoder {
    /// A decoder that hasn't seen any input yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode `bytes` onto the end of `out`. With `last`, this is the end
    /// of the input and nothing may be left incomplete.
    pub fn decode(&mut self, bytes: &[u8], last: bool, out: &mut String) -> Result<()> {
        self.pending.extend_from_slice(bytes);
        let kind = match self.kind {
            Some(k) => k,
            None => match self.detect(last)? {
                Some(k) => {
                    self.kind = Some(k);
                    k
                }
                None => return Ok(()), // need more of the start
            },
        };
        let start = out.len();
        let used = match kind {
            Kind::Bit8(Kind8::Latin1) => {
                out.extend(self.pending.iter().map(|&b| b as char));
                self.pending.len()
            }
            Kind::Bit8(k) => {
                let valid = match std::str::from_utf8(&self.pending) {
                    Ok(s) => s.len(),
                    Err(e) if e.error_len().is_some() => {
                        return Err(error(format!("invalid UTF-8 at offset {}", self.offset + e.valid_up_to())));
                    }
                    Err(e) => e.valid_up_to(), // a character continues in the next chunk
                };
                if k == Kind8::Ascii {
                    check_ascii(&self.pending[..valid], self.offset)?;
                }
                out.push_str(std::str::from_utf8(&self.pending[..valid]).expect("validated above"));
                valid
            }
            Kind::Utf16(unit) => {
                let mut n = self.pending.len() / 2 * 2;
                // Keep a trailing high surrogate for its partner.
                if n >= 2 && !last && (0xD800..0xDC00).contains(&unit([self.pending[n - 2], self.pending[n - 1]])) {
                    n -= 2;
                }
                let units = self.pending[..n].as_chunks::<2>().0.iter().map(|&pair| unit(pair));
                for c in char::decode_utf16(units) {
                    out.push(c.map_err(|e| error(format!("invalid UTF-16: {e}")))?);
                }
                n
            }
        };
        self.pending.drain(..used);
        self.offset += used;
        if last && !self.pending.is_empty() {
            return Err(error("input ends in the middle of a character".into()));
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
    fn detect(&mut self, last: bool) -> Result<Option<Kind>> {
        let p = &self.pending[..];
        let utf16 = |be: bool| -> Kind { Kind::Utf16(if be { u16::from_be_bytes } else { u16::from_le_bytes }) };
        let kind = match p {
            [0xFE, 0xFF, ..] => utf16(true),
            [0xFF, 0xFE, ..] => utf16(false),
            _ if p.len() < 4 && !last => return Ok(None),
            [0x00, 0x3C, 0x00, 0x3F, ..] => utf16(true),
            [0x3C, 0x00, 0x3F, 0x00, ..] => utf16(false),
            [0x00, 0x00, ..] | [_, 0x00, 0x00, ..] => return Err(error("UCS-4 input is not supported".into())),
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
                Kind::Bit8(choose_8bit(p)?)
            }
        };
        if let Kind::Utf16(_) = kind {
            self.utf16_head = Some(String::new());
        }
        Ok(Some(kind))
    }
}
