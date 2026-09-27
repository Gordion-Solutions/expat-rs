//! Character-level rules shared by the lexer and the entity layer.
//!
//! Per W3C XML 1.0 (Fifth Edition) §2.2 and §4.1.

/// Per §2.2 [Production 2]: Char
/// = #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]
///
/// Surrogates (D800-DFFF) and code points above #x10FFFF cannot be a Rust
/// `char`, so in practice this rejects #x0-#x8, #xB, #xC, #xE-#x1F, #xFFFE
/// and #xFFFF.
pub(crate) fn is_xml_char(c: char) -> bool {
    matches!(c as u32,
        0x9 | 0xA | 0xD |
        0x20..=0xD7FF |
        0xE000..=0xFFFD |
        0x10000..=0x10FFFF
    )
}

/// Byte offset of the first character in `s` that is not a §2.2 Char, if
/// any. Every character of the document entity must match Char
/// (§2.1 [Production 1], via `content`, `Misc`, etc.).
pub(crate) fn first_invalid_char(s: &str) -> Option<(usize, char)> {
    s.char_indices().find(|&(_, c)| !is_xml_char(c))
}

/// Decode the body of a character reference per §4.1 [Production 66]:
///
///   CharRef ::= '&#' [0-9]+ ';' | '&#x' [0-9a-fA-F]+ ';'
///
/// `body` is the text between `&#` and `;` (so `"x41"` or `"65"`). Returns
/// the referenced character, or a reason string if the reference is
/// malformed or names a character outside §2.2 Char (Legal Character WFC).
pub(crate) fn decode_char_ref(body: &str) -> std::result::Result<char, String> {
    let (radix, digits) = match body.strip_prefix('x') {
        Some(hex) => (16, hex),
        None      => (10, body),
    };
    // from_str_radix alone is too lenient: it accepts a leading '+'.
    let digits_ok = !digits.is_empty() && digits.bytes().all(|b| match radix {
        16 => b.is_ascii_hexdigit(),
        _  => b.is_ascii_digit(),
    });
    if !digits_ok {
        return Err(format!("malformed character reference '&#{body};'"));
    }
    let n = u32::from_str_radix(digits, radix)
        .map_err(|_| format!("character reference '&#{body};' is out of range"))?;
    let c = char::from_u32(n)
        .ok_or_else(|| format!("character reference '&#{body};' = U+{n:04X} is not a Unicode scalar value"))?;
    if !is_xml_char(c) {
        return Err(format!("character reference '&#{body};' = U+{n:04X} is not a legal XML Char (§2.2)"));
    }
    Ok(c)
}
