//! XML 1.0 edition selection: Fifth Edition §2.3 Name rules (default) vs.
//! Fourth Edition Appendix B character classes.

use expat_rs::{Edition, Parser, XmlError};

fn parse(src: &str, edition: Edition) -> Result<(), XmlError> {
    let mut p = Parser::new(src).with_edition(edition);
    while p.next_event()?.is_some() {}
    Ok(())
}

fn both_accept(src: &str) {
    parse(src, Edition::Fifth).unwrap_or_else(|e| panic!("5th rejected {src:?}: {e}"));
    parse(src, Edition::Fourth).unwrap_or_else(|e| panic!("4th rejected {src:?}: {e}"));
}

fn only_fifth_accepts(src: &str) {
    parse(src, Edition::Fifth).unwrap_or_else(|e| panic!("5th rejected {src:?}: {e}"));
    assert!(parse(src, Edition::Fourth).is_err(), "4th accepted {src:?}");
}

#[test]
fn default_is_fifth() {
    assert_eq!(Edition::default(), Edition::Fifth);
    // U+00D7 (multiplication sign) is outside BaseChar but not a 5th-ed
    // NameStartChar either; U+0132 (IJ ligature) is the classic split case.
    Parser::new("<\u{132}/>").next_event().expect("default = 5th accepts U+0132");
}

#[test]
fn ascii_names_same_in_both() {
    both_accept("<a-b.c_d:e f1='x'><_z/></a-b.c_d:e>");
}

#[test]
fn appendix_b_letters_digits_combining_extenders_accepted_by_both() {
    // Greek alpha (BaseChar), CJK ideograph, Arabic-Indic digit, combining
    // grave accent, middle dot extender.
    both_accept("<\u{3B1}\u{4E00}\u{661}a\u{300}\u{B7}/>");
}

#[test]
fn names_only_legal_in_fifth() {
    // U+0132/U+0133 (IJ ligatures) are excluded from BaseChar.
    only_fifth_accepts("<\u{132}/>");
    // U+0100-U+0131 is BaseChar but Latin Extended-B beyond U+0217 is not.
    only_fifth_accepts("<\u{218}/>");
    // Supplementary-plane letters were never Appendix B.
    only_fifth_accepts("<\u{10400}/>");
    // As an attribute name and as a second character.
    only_fifth_accepts("<a \u{132}='1'/>");
    only_fifth_accepts("<a\u{132}/>");
    // PI target.
    only_fifth_accepts("<a><?p\u{132} x?></a>");
}

#[test]
fn digits_as_name_start() {
    // ASCII digits never start a name.
    assert!(parse("<1/>", Edition::Fifth).is_err());
    assert!(parse("<1/>", Edition::Fourth).is_err());
    // U+0661 ARABIC-INDIC DIGIT ONE is a Digit in Appendix B (not a Letter),
    // but falls inside 5th ed. NameStartChar [#x37F-#x1FFF].
    only_fifth_accepts("<\u{661}/>");
}
