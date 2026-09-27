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
    let units = bytes.chunks_exact(2).map(|b| unit([b[0], b[1]]));
    let text: String = char::decode_utf16(units)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| error(format!("invalid UTF-16: {e}")))?;
    match declared_encoding(&text) {
        Some(enc) if !is_utf16(enc) => Err(error(format!(
            "document is UTF-16 but declares encoding {enc:?}"))),
        _ => Ok(Cow::Owned(text)),
    }
}

fn decode_8bit(bytes: &[u8]) -> Result<Cow<'_, str>> {
    let has_bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    // The declaration itself is ASCII in every supported 8-bit encoding.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(256)]);
    let declared = declared_encoding(head.trim_start_matches('\u{FEFF}')).map(str::to_ascii_uppercase);
    match declared.as_deref() {
        None | Some("UTF-8") => utf8(bytes),
        Some(enc) if has_bom => Err(error(format!("document has a UTF-8 byte order mark but declares encoding {enc:?}"))),
        Some(enc) if is_utf16(enc) => Err(error("UTF-16 documents must begin with a byte order mark".into())),
        Some("US-ASCII" | "ASCII") => match bytes.iter().position(|&b| b >= 0x80) {
            Some(i) => Err(error(format!("byte {:#04x} at offset {i} is not US-ASCII", bytes[i]))),
            None => utf8(bytes),
        },
        Some("ISO-8859-1" | "ISO_8859-1" | "LATIN1") => Ok(Cow::Owned(bytes.iter().map(|&b| b as char).collect())),
        Some(enc) => Err(error(format!("unsupported encoding {enc:?}"))),
    }
}

fn utf8(bytes: &[u8]) -> Result<Cow<'_, str>> {
    std::str::from_utf8(bytes)
        .map(Cow::Borrowed)
        .map_err(|e| error(format!("invalid UTF-8: {e}")))
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
