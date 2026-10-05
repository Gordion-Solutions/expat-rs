//! Encoding detection and decoding (W3C XML 1.0 §4.3.3, Appendix F).

use expat_rs::{decode, Parser, XmlError};

fn utf16(s: &str, bom: bool, le: bool) -> Vec<u8> {
    let mut units: Vec<u16> = Vec::new();
    if bom { units.push(0xFEFF); }
    units.extend(s.encode_utf16());
    units.iter().flat_map(|u| if le { u.to_le_bytes() } else { u.to_be_bytes() }).collect()
}

fn well_formed(bytes: &[u8]) -> Result<(), XmlError> {
    let text = decode(bytes)?;
    let mut p = Parser::new(&text);
    while p.next_event()?.is_some() {}
    Ok(())
}

#[test]
fn utf8_with_and_without_bom() {
    assert_eq!(decode("<x>é</x>".as_bytes()).unwrap(), "<x>é</x>");
    well_formed(b"\xEF\xBB\xBF<x/>").expect("UTF-8 BOM");
    assert!(decode(b"<x>\xFF</x>").is_err(), "invalid UTF-8");
}

#[test]
fn utf16_both_byte_orders() {
    let doc = "<?xml version='1.0' encoding='UTF-16'?><x>€𝄞</x>";
    for le in [true, false] {
        well_formed(&utf16(doc, true, le)).expect("UTF-16 with BOM");
        well_formed(&utf16(doc, false, le)).expect("UTF-16 detected from '<?'");
    }
    well_formed(&utf16("<x/>", true, true)).expect("UTF-16 BOM, no declaration");
}

#[test]
fn utf16_errors() {
    let mut odd = utf16("<x/>", true, true);
    odd.push(0);
    assert!(decode(&odd).is_err(), "odd length");
    assert!(decode(&[0xFF, 0xFE, 0x00, 0xD8]).is_err(), "unpaired surrogate");
    assert!(decode(&utf16("<?xml version='1.0' encoding='UTF-8'?><x/>", true, true)).is_err(),
            "UTF-16 bytes declaring UTF-8");
}

#[test]
fn declared_8bit_encodings() {
    // ISO-8859-1: byte 0xE9 is 'é'.
    let latin1 = b"<?xml version='1.0' encoding='ISO-8859-1'?><x>\xE9</x>";
    assert_eq!(decode(latin1).unwrap(), "<?xml version='1.0' encoding='ISO-8859-1'?><x>é</x>");
    well_formed(b"<?xml version='1.0' encoding='US-ASCII'?><x/>").expect("ASCII");
    assert!(decode(b"<?xml version='1.0' encoding='US-ASCII'?><x>\xE9</x>").is_err(), "non-ASCII byte");
    assert!(decode(b"<?xml version='1.0' encoding='UTF-16'?><x/>").is_err(), "UTF-16 needs a BOM");
    assert!(decode(b"\xEF\xBB\xBF<?xml version='1.0' encoding='ISO-8859-1'?><x/>").is_err(),
            "UTF-8 BOM declaring another encoding");
    assert!(matches!(decode(b"<?xml version='1.0' encoding='EBCDIC-XYZ'?><x/>"),
                     Err(XmlError::Encoding { .. })), "unsupported encoding");
}

// ─── unknown encodings via byte maps; forced (protocol) encodings ─────────────

use expat_rs::{ByteMap, ErrorKind, Event, StreamParser};

/// windows-1252 as a byte map: ASCII and Latin-1 except 0x80 = '€', and
/// 0x81 left undefined.
fn cp1252() -> ByteMap {
    let mut map: ByteMap = Box::new([None; 256]);
    for b in 0..256usize {
        map[b] = Some(b as u8 as char);
    }
    map[0x80] = Some('€');
    map[0x81] = None;
    map
}

fn stream_text(bytes: &[u8], chunk: usize, parser: StreamParser<'_>) -> Result<String, expat_rs::XmlError> {
    let mut p = parser;
    let mut text = String::new();
    for c in bytes.chunks(chunk) {
        p.feed(c, |e| if let Event::Text(t) = e { text.push_str(&t) })?;
    }
    p.finish(|e| if let Event::Text(t) = e { text.push_str(&t) })?;
    Ok(text)
}

const CP1252_DOC: &[u8] = b"<?xml version='1.0' encoding='windows-1252'?><p>\x80 caf\xe9</p>";

#[test]
fn unknown_encoding_resolved_by_byte_map() {
    let mut asked = Vec::new();
    let got = stream_text(CP1252_DOC, 1000, StreamParser::new().with_unknown_encoding(|name| {
        asked.push(name.to_string());
        Some(cp1252())
    })).unwrap();
    assert_eq!(got, "€ café");
    assert_eq!(asked, ["WINDOWS-1252"], "asked once, with the declared name");
    for chunk in [1, 7] {
        let got = stream_text(CP1252_DOC, chunk, StreamParser::new().with_unknown_encoding(|_| Some(cp1252()))).unwrap();
        assert_eq!(got, "€ café", "chunk {chunk}");
    }
}

#[test]
fn unknown_encoding_without_a_map_is_an_error() {
    let no_handler = stream_text(CP1252_DOC, 1000, StreamParser::new());
    assert_eq!(no_handler.unwrap_err().kind(), ErrorKind::UnknownEncoding);
    let declined = stream_text(CP1252_DOC, 1000, StreamParser::new().with_unknown_encoding(|_| None));
    assert_eq!(declined.unwrap_err().kind(), ErrorKind::UnknownEncoding);
}

#[test]
fn byte_invalid_in_the_map_is_an_error() {
    let doc = b"<?xml version='1.0' encoding='windows-1252'?><p>\x81</p>";
    let r = stream_text(doc, 1000, StreamParser::new().with_unknown_encoding(|_| Some(cp1252())));
    assert_eq!(r.unwrap_err().kind(), ErrorKind::InvalidToken);
}

#[test]
fn forced_encoding_overrides_the_declaration() {
    // Declares UTF-8, but the caller says ISO-8859-1: 0xE9 is 'é'.
    let doc = b"<?xml version='1.0' encoding='UTF-8'?><p>caf\xe9</p>";
    assert_eq!(stream_text(doc, 3, StreamParser::new().with_encoding("ISO-8859-1")).unwrap(), "café");
    // A forced name expat-rs doesn't know goes to the byte-map handler.
    let doc = b"<p>\x80</p>";
    let got = stream_text(doc, 2, StreamParser::new().with_encoding("cp1252").with_unknown_encoding(|_| Some(cp1252())));
    assert_eq!(got.unwrap(), "€");
}

#[test]
fn forced_utf16_without_bom_is_big_endian() {
    let doc: Vec<u8> = "<p>€</p>".encode_utf16().flat_map(u16::to_be_bytes).collect();
    assert_eq!(stream_text(&doc, 3, StreamParser::new().with_encoding("UTF-16")).unwrap(), "€");
    let le: Vec<u8> = "<p>€</p>".encode_utf16().flat_map(u16::to_le_bytes).collect();
    assert_eq!(stream_text(&le, 3, StreamParser::new().with_encoding("UTF-16LE")).unwrap(), "€");
}
