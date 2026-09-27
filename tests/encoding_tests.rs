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
