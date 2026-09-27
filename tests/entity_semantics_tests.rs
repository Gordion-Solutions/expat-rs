//! What entity references bring in (W3C XML 1.0 §4.1, §4.3.2, §4.4):
//! replacement-text well-formedness, No Recursion, attribute-value rules,
//! the Entity Declared WFC, and external entity loading (opt-in only).

use std::cell::Cell;
use std::rc::Rc;

use expat_rs::{Parser, XmlError};

fn parse(src: &str) -> Result<(), XmlError> {
    let mut p = Parser::new(src);
    while p.next_event()?.is_some() {}
    Ok(())
}

fn ok(src: &str) {
    parse(src).unwrap_or_else(|e| panic!("rejected {src:?}: {e}"));
}

fn bad(src: &str) {
    assert!(parse(src).is_err(), "accepted {src:?}");
}

// ─── Replacement text in content must match `content` ──────────────────────

#[test]
fn content_entities_must_be_balanced() {
    ok("<!DOCTYPE d [<!ENTITY e '<a>x</a><b/>'>]><d>&e;</d>");
    ok("<!DOCTYPE d [<!ENTITY e '<a>&f;</a>'><!ENTITY f 'text'>]><d>&e;</d>");
    bad("<!DOCTYPE d [<!ENTITY e '</a><a>'>]><d><a>&e;</a></d>");
    bad("<!DOCTYPE d [<!ENTITY e '<a>'>]><d>&e;</a></d>");
    bad("<!DOCTYPE d [<!ENTITY e '&#60;!--'>]><d>&e;--></d>");
}

#[test]
fn char_refs_expand_before_the_text_is_parsed() {
    // &#38; becomes a bare '&' in the replacement text (§4.5).
    bad("<!DOCTYPE d [<!ENTITY e '&#38;'>]><d>&e;</d>");
    ok("<!DOCTYPE d [<!ENTITY e '&#38;amp;'>]><d>&e;</d>");
    ok("<!DOCTYPE d [<!ENTITY e '&#60;a/>'>]><d>&e;</d>");
}

#[test]
fn no_text_declaration_in_internal_entity() {
    bad("<!DOCTYPE d [<!ENTITY e \"<?xml encoding='UTF-8'?>\">]><d>&e;</d>");
}

#[test]
fn unparsed_entity_not_allowed_in_content() {
    bad("<!DOCTYPE d [<!NOTATION n SYSTEM 'n'><!ENTITY e SYSTEM 'x' NDATA n>]><d>&e;</d>");
}

// ─── No Recursion ───────────────────────────────────────────────────────────

#[test]
fn recursion_detected_in_content_attributes_and_defaults() {
    bad("<!DOCTYPE d [<!ENTITY e '&e;'>]><d>&e;</d>");
    bad("<!DOCTYPE d [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><d>&a;</d>");
    bad("<!DOCTYPE d [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><d x='&a;'/>");
    bad("<!DOCTYPE d [<!ENTITY a '&b;'><!ENTITY b '&a;'><!ATTLIST d x CDATA '&a;'>]><d/>");
    // Declared but never referenced: no recursion happens.
    ok("<!DOCTYPE d [<!ENTITY a '&b;'><!ENTITY b '&a;'>]><d/>");
    // The same entity twice side by side is not recursion.
    ok("<!DOCTYPE d [<!ENTITY a 'x'><!ENTITY b '&a;&a;'>]><d>&b;</d>");
}

// ─── Attribute values ───────────────────────────────────────────────────────

#[test]
fn attribute_value_entity_rules() {
    ok("<!DOCTYPE d [<!ENTITY e 'v'>]><d x='&e;&amp;'/>");
    bad("<!DOCTYPE d [<!ENTITY e '<'>]><d x='&e;'/>");                // No < in Attribute Values
    bad("<!DOCTYPE d [<!ENTITY e '&#60;'>]><d x='&e;'/>");
    ok("<!DOCTYPE d [<!ENTITY e '&#38;#60;'>]><d x='&e;'/>");        // '&#60;' again: a char ref, fine
    bad("<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'>]><d x='&e;'/>");     // No External Entity References
    bad("<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'><!ENTITY i '&e;'>]><d x='&i;'/>");
    bad("<!DOCTYPE d [<!ENTITY e '&#38;'>]><d x='&e;'/>");            // bare '&' after expansion
    bad("<!DOCTYPE d [<!ENTITY e 'v'><!ATTLIST d x CDATA '&f;'>]><d/>");
}

// ─── Entity Declared WFC ────────────────────────────────────────────────────

#[test]
fn entity_declared_wfc_applies_without_external_dtd() {
    bad("<d>&e;</d>");
    bad("<d x='&e;'/>");
    bad("<!DOCTYPE d [<!ELEMENT d ANY>]><d>&e;</d>");
    // Declaration must precede use in an attribute default.
    bad("<!DOCTYPE d [<!ATTLIST d x CDATA '&e;'><!ENTITY e 'v'>]><d/>");
}

#[test]
fn entity_declared_is_only_a_validity_issue_with_external_dtd_or_pe_refs() {
    ok("<!DOCTYPE d SYSTEM 'd.dtd'><d>&e;</d>");
    ok("<!DOCTYPE d SYSTEM 'd.dtd'><d x='&e;'/>");
    ok("<!DOCTYPE d [%p;]><d>&e;</d>");
    // ...unless standalone="yes".
    bad("<?xml version='1.0' standalone='yes'?><!DOCTYPE d SYSTEM 'd.dtd'><d>&e;</d>");
}

#[test]
fn declarations_after_unread_pe_reference_are_not_processed() {
    // §5.1: 'e' is declared after an unread PE reference, so it is not
    // processed and its (unbalanced) text is never checked.
    ok("<!DOCTYPE d [%p;<!ENTITY e '<a>'>]><d>&e;</d>");
}

#[test]
fn first_declaration_is_binding() {
    ok("<!DOCTYPE d [<!ENTITY e 'fine'><!ENTITY e '<a>'>]><d>&e;</d>");
    bad("<!DOCTYPE d [<!ENTITY e '<a>'><!ENTITY e 'fine'>]><d>&e;</d>");
}

// ─── External entities: opt-in loader ──────────────────────────────────────

const EXTERNAL: &str = "<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'>]><d>&e;</d>";

#[test]
fn external_entities_not_read_by_default() {
    ok(EXTERNAL);
}

fn with_loader(src: &str, text: &'static str) -> (Result<(), XmlError>, usize) {
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let mut p = Parser::new(src).with_external_loader(move |system_id, public_id| {
        assert_eq!((system_id, public_id), ("e.xml", None));
        seen.set(seen.get() + 1);
        Ok(Some(text.to_string()))
    });
    let r = (|| { while p.next_event()?.is_some() {} Ok(()) })();
    (r, calls.get())
}

#[test]
fn loader_text_is_checked_as_content() {
    assert_eq!(with_loader(EXTERNAL, "<a>text</a>"), (Ok(()), 1));
    assert!(with_loader(EXTERNAL, "<a>").0.is_err(), "unbalanced external entity");
    assert!(with_loader(EXTERNAL, "&e;").0.is_err(), "self-reference via external entity");
}

#[test]
fn text_declaration_in_external_entity() {
    assert!(with_loader(EXTERNAL, "<?xml encoding='UTF-8'?><a/>").0.is_ok());
    assert!(with_loader(EXTERNAL, "<?xml version='1.0' encoding='UTF-8'?><a/>").0.is_ok());
    assert!(with_loader(EXTERNAL, "<?xml version='1.0'?><a/>").0.is_err(), "encoding required");
    assert!(with_loader(EXTERNAL, "<?xml encoding='UTF-8' standalone='yes'?><a/>").0.is_err(),
            "no standalone in a text declaration");
    assert!(with_loader(EXTERNAL, "<a/><?xml encoding='UTF-8'?>").0.is_err(), "only at the start");
}

#[test]
fn loader_returning_none_leaves_entity_unread() {
    let mut p = Parser::new(EXTERNAL).with_external_loader(|_, _| Ok(None));
    while p.next_event().expect("unread external entity is fine").is_some() {}
}

#[test]
fn loader_failure_is_an_error() {
    let mut p = Parser::new(EXTERNAL).with_external_loader(|_, _| Err("no such file".into()));
    let r = (|| { while p.next_event()?.is_some() {} Ok(()) })();
    assert!(matches!(r, Err(XmlError::ExternalEntity { .. })), "got {r:?}");
}

// ─── Error positions ────────────────────────────────────────────────────────

#[test]
fn parser_errors_report_real_positions() {
    match parse("<a>\n  <b></c>\n</a>") {
        Err(XmlError::NotWellFormed { pos, .. }) => assert_eq!((pos.line, pos.column), (2, 6)),
        other => panic!("expected mismatched end tag, got {other:?}"),
    }
    match parse("<d>\n\n  &nope;</d>") {
        Err(XmlError::NotWellFormed { pos, .. }) => assert_eq!((pos.line, pos.column), (3, 3)),
        other => panic!("expected undeclared entity, got {other:?}"),
    }
}
