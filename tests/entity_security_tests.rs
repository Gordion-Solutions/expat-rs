//! Entity-expansion security tests.
//!
//! These exercise the defences against the **billion-laughs** and
//! **quadratic-blowup** vulnerability classes that have repeatedly affected
//! libexpat (and many other XML parsers). Each test sends an adversarial
//! payload and asserts the parser rejects it without runaway memory use.

use expat_rs::{ExpansionLimits, Parser, XmlError};

fn drain(p: &mut Parser<'_>) -> Result<(), XmlError> {
    while p.next_event()?.is_some() {}
    Ok(())
}

/// The classic billion-laughs payload: 10 levels of 10× expansion = 10^10.
/// A naïve parser allocates ~10 GB; we must reject before that happens.
#[test]
fn billion_laughs_classic_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY lol  "lol">
  <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
  <!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
  <!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
  <!ENTITY lol6 "&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;">
  <!ENTITY lol7 "&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;">
  <!ENTITY lol8 "&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;">
  <!ENTITY lol9 "&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;">
]>
<lolz>&lol9;</lolz>"#;
    let mut p = Parser::new(payload);
    let err = drain(&mut p).expect_err("billion-laughs payload must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("billion-laughs") || msg.contains("budget") || msg.contains("depth"),
        "error message should explain why ({msg:?})");
}

/// Quadratic blowup: linear-size payload (no exponential nesting) but the
/// parser would do quadratic work. Often used to bypass naïve depth-only caps.
#[test]
fn quadratic_blowup_rejected() {
    let mut payload = String::from(r#"<?xml version="1.0"?>
<!DOCTYPE bomb [
  <!ENTITY a "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">
]>
<bomb>"#);
    // 100,000 references to &a; — 10^5 × 100 chars = 10 MB total expansion
    for _ in 0..100_000 { payload.push_str("&a;"); }
    payload.push_str("</bomb>");

    let mut p = Parser::new(&payload);
    let err = drain(&mut p).expect_err("quadratic-blowup payload must be rejected");
    // The error happens on one of the first ~few thousand &a; references —
    // the budget is exhausted long before all 100k are processed.
    let _ = err;
}

/// Self-referential entity → infinite recursion if depth check missing.
#[test]
fn self_referential_entity_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY a "&a;">
]>
<x>&a;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "self-referential entity must be rejected");
}

/// Two entities referencing each other → mutual recursion.
#[test]
fn mutual_recursion_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY a "&b;">
  <!ENTITY b "&a;">
]>
<x>&a;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "mutually-recursive entities must be rejected");
}

/// Sane DTD-defined entity must work.
#[test]
fn declared_entity_accepted() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY greeting "Hello, world">
]>
<x>&greeting;</x>"#;
    let mut p = Parser::new(payload);
    drain(&mut p).expect("simple declared entity must parse cleanly");
}

/// Sane recursive (but bounded) entities must work.
#[test]
fn shallow_recursion_accepted() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY base "x">
  <!ENTITY one  "&base;&base;">
  <!ENTITY two  "&one;&one;">
]>
<x>&two;</x>"#;
    let mut p = Parser::new(payload);
    drain(&mut p).expect("shallow recursion within limits must parse");
}

/// Caller can tighten the limits.
#[test]
fn custom_tighter_limits_apply() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY a "abcdefghij">
  <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
]>
<x>&b;</x>"#;
    // Default limits would accept this (100 bytes total). Tighten to 50.
    let mut p = Parser::new(payload).with_expansion_limits(ExpansionLimits {
        max_depth: 20,
        max_expanded_bytes: 50,
    });
    assert!(drain(&mut p).is_err(), "tighter byte budget must reject");
}

/// Undeclared entity reference must be rejected.
#[test]
fn undeclared_entity_rejected() {
    let payload = "<doc>&missing;</doc>";
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "undeclared entity reference must be rejected");
}

/// Built-in entities never need a DTD.
#[test]
fn builtin_entities_without_dtd() {
    let payload = "<doc>&amp;&lt;&gt;&quot;&apos;</doc>";
    let mut p = Parser::new(payload);
    drain(&mut p).expect("built-in entities don't need DTD");
}

// -- XML 1.0 §2.2 Char validation for numeric character references --------
//
// Prior to fixing entities.rs:119, every `&#…;` was charged a flat 4 bytes
// against the budget and no validation of the codepoint itself was
// performed — so out-of-range and non-Char references slipped through.
// Each ref below sits inside an entity value to exercise the validation
// path in `entities.rs`. Refs directly in document content go through the
// lexer; both share `chars::decode_char_ref` and are covered in
// well_formedness_tests.rs.

/// Sebastian's example: the maximum valid XML Char (U+10FFFF), 4-byte UTF-8.
#[test]
fn numeric_ref_max_unicode_is_valid() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY max "&#x10FFFF;">
]>
<x>&max;</x>"#;
    let mut p = Parser::new(payload);
    drain(&mut p).expect("U+10FFFF must be valid per XML 1.0 §2.2");
}

/// One past the top of the Unicode scalar range — must reject.
#[test]
fn numeric_ref_above_unicode_is_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY over "&#x110000;">
]>
<x>&over;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "&#x110000; is above U+10FFFF and must be rejected");
}

/// Surrogate code points are never valid XML Chars.
#[test]
fn numeric_ref_surrogate_is_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY sur "&#xD800;">
]>
<x>&sur;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "&#xD800; (surrogate) must be rejected");
}

/// NUL is not in the §2.2 Char production — must reject.
#[test]
fn numeric_ref_nul_is_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY n "&#x0;">
]>
<x>&n;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "&#x0; (NUL) is not a valid XML Char and must be rejected");
}

/// #xFFFE is excluded from §2.2 Char — must reject.
#[test]
fn numeric_ref_noncharacter_fffe_is_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY nc "&#xFFFE;">
]>
<x>&nc;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "&#xFFFE; (non-character) must be rejected");
}

/// ASCII character references should cost their actual UTF-8 length (1 byte),
/// not the hardcoded 4 the old code charged. Four refs expand to "ABCD"
/// (4 bytes). Under the old accounting they would consume 16 bytes against
/// the budget; under the new accounting they consume 4. A 10-byte budget
/// passes under the new accounting and would have failed under the old.
#[test]
fn numeric_ref_ascii_costs_one_byte_not_four() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY a "&#x41;&#x42;&#x43;&#x44;">
]>
<x>&a;</x>"#;
    let mut p = Parser::new(payload).with_expansion_limits(ExpansionLimits {
        max_depth: 20,
        max_expanded_bytes: 10,
    });
    drain(&mut p).expect("4 ASCII char refs (4 bytes expanded) must fit in a 10-byte budget");
}

/// `u32::from_str_radix` accepts a leading '+'; Production 66 does not.
#[test]
fn numeric_ref_with_plus_sign_is_rejected() {
    let payload = r#"<?xml version="1.0"?>
<!DOCTYPE x [
  <!ENTITY p "&#+65;">
]>
<x>&p;</x>"#;
    let mut p = Parser::new(payload);
    assert!(drain(&mut p).is_err(), "&#+65; is not a legal CharRef and must be rejected");
}
