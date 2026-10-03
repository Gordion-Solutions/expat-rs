//! Character-level rules shared by the lexer and the entity layer.
//!
//! Per W3C XML 1.0 (Fifth Edition) §2.2, §2.3 and §4.1, plus the
//! Fourth Edition name rules (Appendix B) behind [`Edition::Fourth`].

use crate::edition4::{BASE_CHAR, COMBINING_CHAR, DIGIT, EXTENDER, IDEOGRAPHIC};

/// Which edition of XML 1.0 decides what characters may appear in names.
///
/// The editions differ only in the Name productions. The Fifth Edition
/// (2008, the current Recommendation) allows broad Unicode ranges. Editions
/// one to four allow a fixed list of letters, digits, combining characters
/// and extenders (Appendix B), so some names legal under the Fifth Edition
/// are not well-formed under the Fourth. Choose `Fourth` to accept and
/// reject names the way parsers built on the older rules do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edition {
    /// XML 1.0 Fourth Edition, Appendix B character classes.
    Fourth,
    /// XML 1.0 Fifth Edition, §2.3 [Productions 4, 4a].
    #[default]
    Fifth,
}

/// Name start character under `edition`.
pub(crate) fn is_name_start_char(c: char, edition: Edition) -> bool {
    if c.is_ascii() {
        // Same in both editions.
        return c.is_ascii_alphabetic() || c == '_' || c == ':';
    }
    match edition {
        Edition::Fifth  => is_name_start_char_5e(c),
        // 4th ed. [5] Name ::= (Letter | '_' | ':') (NameChar)*
        Edition::Fourth => c == '_' || c == ':' || is_letter_4e(c),
    }
}

/// Name character under `edition`.
pub(crate) fn is_name_char(c: char, edition: Edition) -> bool {
    if c.is_ascii() {
        // Same in both editions.
        return c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-' | '.');
    }
    match edition {
        Edition::Fifth => is_name_start_char_5e(c) || matches!(c,
            '-' | '.' | '0'..='9' | '\u{B7}' |
            '\u{0300}'..='\u{036F}' | '\u{203F}'..='\u{2040}'
        ),
        // 4th ed. [4] NameChar ::= Letter | Digit | '.' | '-' | '_' | ':'
        //                         | CombiningChar | Extender
        Edition::Fourth => matches!(c, '.' | '-' | '_' | ':')
            || is_letter_4e(c)
            || in_table(DIGIT, c)
            || in_table(COMBINING_CHAR, c)
            || in_table(EXTENDER, c),
    }
}

/// Per 5th ed. §2.3 [Production 4]: NameStartChar.
fn is_name_start_char_5e(c: char) -> bool {
    matches!(c,
        ':' | '_' | 'A'..='Z' | 'a'..='z' |
        '\u{C0}'..='\u{D6}'    | '\u{D8}'..='\u{F6}'   |
        '\u{F8}'..='\u{2FF}'   | '\u{370}'..='\u{37D}' |
        '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' |
        '\u{2070}'..='\u{218F}'| '\u{2C00}'..='\u{2FEF}' |
        '\u{3001}'..='\u{D7FF}'| '\u{F900}'..='\u{FDCF}' |
        '\u{FDF0}'..='\u{FFFD}'| '\u{10000}'..='\u{EFFFF}'
    )
}

/// 4th ed. [84] Letter ::= BaseChar | Ideographic
fn is_letter_4e(c: char) -> bool {
    in_table(BASE_CHAR, c) || in_table(IDEOGRAPHIC, c)
}

/// Binary search a sorted, non-overlapping range table.
fn in_table(table: &[(char, char)], c: char) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c { std::cmp::Ordering::Less }
            else if lo > c { std::cmp::Ordering::Greater }
            else { std::cmp::Ordering::Equal }
        })
        .is_ok()
}

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
    // Byte scan: in valid UTF-8 the only non-Chars are C0 controls other
    // than TAB, LF and CR (single bytes), and U+FFFE / U+FFFF (EF BF BE,
    // EF BF BF). Surrogates can't occur in a Rust string.
    let bytes = s.as_bytes();
    let mut i = 0;
    // Tight search for candidates (any C0 control, or 0xEF), then check.
    while let Some(k) = bytes[i..].iter().position(|&b| b < 0x20 || b == 0xEF) {
        i += k;
        match bytes[i] {
            b @ 0x00..=0x1F if !matches!(b, b'\t' | b'\n' | b'\r') => return Some((i, b as char)),
            0xEF if bytes.get(i + 1) == Some(&0xBF) && matches!(bytes.get(i + 2), Some(0xBE | 0xBF)) => {
                return Some((i, if bytes[i + 2] == 0xBE { '\u{FFFE}' } else { '\u{FFFF}' }));
            }
            _ => {}
        }
        i += 1;
    }
    None
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
