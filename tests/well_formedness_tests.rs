//! Well-formedness conformance tests. One assertion per W3C XML 1.0 §2.1
//! constraint or related rule.

use expat_rs::{Event, Parser, XmlError};

fn events(src: &str) -> Result<Vec<Event<'_>>, XmlError> {
    let mut p = Parser::new(src);
    let mut out = Vec::new();
    while let Some(e) = p.next_event()? {
        out.push(e);
    }
    Ok(out)
}

// ─── Happy paths ─────────────────────────────────────────────────────────────

#[test]
fn smallest_well_formed_doc() {
    let es = events("<root/>").unwrap();
    assert_eq!(es.len(), 2, "<x/> emits StartElement+EndElement, got {es:?}");
    assert!(matches!(&es[0], Event::StartElement { name, .. } if *name == "root"));
    assert!(matches!(&es[1], Event::EndElement(n) if *n == "root"));
}

#[test]
fn nested_elements_balanced() {
    let es = events("<a><b><c/></b></a>").unwrap();
    let opens: Vec<&str> = es.iter().filter_map(|e| match e {
        Event::StartElement { name, .. } => Some(*name), _ => None,
    }).collect();
    let closes: Vec<&str> = es.iter().filter_map(|e| match e {
        Event::EndElement(n) => Some(*n), _ => None,
    }).collect();
    assert_eq!(opens, vec!["a", "b", "c"]);
    assert_eq!(closes, vec!["c", "b", "a"]);
}

#[test]
fn xml_declaration_first_then_root() {
    let es = events(r#"<?xml version="1.0"?><r/>"#).unwrap();
    assert!(matches!(&es[0], Event::XmlDecl(_)));
    assert!(matches!(&es[1], Event::StartElement { name, .. } if *name == "r"));
}

#[test]
fn comments_in_prolog_and_epilog() {
    let es = events("<!-- pre --><r/><!-- post -->").unwrap();
    assert!(matches!(&es[0], Event::Comment(s) if *s == " pre "));
    assert!(matches!(&es[3], Event::Comment(s) if *s == " post "));
}

#[test]
fn whitespace_in_prolog_is_absorbed() {
    let es = events("   \n\n  <r/>").unwrap();
    assert_eq!(es.len(), 2);
    assert!(matches!(&es[0], Event::StartElement { .. }));
}

// ─── Well-formedness violations ──────────────────────────────────────────────
// Per W3C XML 1.0 §2.1 — every one of these documents is not well-formed.

#[test]
fn no_root_element() {
    assert!(matches!(
        events("<!-- only a comment -->").unwrap_err(),
        XmlError::NotWellFormed { .. }
    ));
}

#[test]
fn unclosed_root() {
    assert!(events("<root>").is_err(), "missing </root> must be rejected");
}

#[test]
fn mismatched_tags() {
    assert!(events("<a></b>").is_err(),
        "end tag </b> with start tag <a> must be rejected");
}

#[test]
fn improperly_nested() {
    assert!(events("<a><b></a></b>").is_err(),
        "improper nesting must be rejected");
}

#[test]
fn second_root_element_rejected() {
    assert!(events("<a/><b/>").is_err(),
        "two root elements must be rejected");
}

#[test]
fn text_outside_root_rejected() {
    assert!(events("hello<r/>").is_err(),
        "non-whitespace text before root must be rejected");
    assert!(events("<r/>hello").is_err(),
        "non-whitespace text after root must be rejected");
}

#[test]
fn duplicate_attribute_rejected() {
    assert!(events(r#"<r a="1" a="2"/>"#).is_err(),
        "duplicate attribute names must be rejected (§3.1 Unique Att Spec)");
}

#[test]
fn xmldecl_not_at_start_rejected() {
    assert!(events("<r/><?xml version=\"1.0\"?>").is_err(),
        "XML declaration must be first");
}

#[test]
fn duplicate_xmldecl_rejected() {
    let src = r#"<?xml version="1.0"?><?xml version="1.0"?><r/>"#;
    assert!(events(src).is_err(), "two XML declarations must be rejected");
}

#[test]
fn duplicate_doctype_rejected() {
    let src = "<!DOCTYPE a><!DOCTYPE b><a/>";
    assert!(events(src).is_err(), "two DOCTYPE declarations must be rejected");
}

#[test]
fn doctype_after_root_rejected() {
    assert!(events("<a/><!DOCTYPE x>").is_err(),
        "DOCTYPE must appear before the root element");
}

#[test]
fn end_tag_with_no_open_rejected() {
    assert!(events("</foo>").is_err(),
        "end tag without matching start must be rejected");
}

// ─── Built-in entities (week-2 minimal handling) ────────────────────────────

#[test]
fn builtin_entities_inside_root_are_accepted() {
    // We don't yet expand entities into Text bytes; we just confirm they're
    // accepted inside the root and rejected outside.
    let _ = events("<r>&amp;&lt;&gt;</r>").expect("built-in entities inside root must parse");
}

#[test]
fn entity_reference_outside_root_rejected() {
    assert!(events("&amp;<r/>").is_err(),
        "entity reference in prolog must be rejected");
}

// ─── Comments and PIs may appear in prolog AND epilog ───────────────────────

#[test]
fn pi_in_prolog_and_epilog() {
    let es = events("<?xml-stylesheet href=\"a.xsl\"?><r/><?gen done?>").unwrap();
    let pi_targets: Vec<&str> = es.iter().filter_map(|e| match e {
        Event::ProcessingInstruction { target, .. } => Some(*target),
        _ => None,
    }).collect();
    assert_eq!(pi_targets, vec!["xml-stylesheet", "gen"]);
}

// ─── §2.2 Char / §4.1 Legal Character WFC ────────────────────────────────────

#[test]
fn char_ref_in_content_valid_range() {
    events("<x>&#65;&#x10FFFF;&#x9;</x>").expect("legal Chars by reference");
}

#[test]
fn char_ref_in_content_illegal_chars_rejected() {
    for r in ["&#0;", "&#x0;", "&#x1F;", "&#xD800;", "&#xFFFE;", "&#xFFFF;", "&#x110000;"] {
        let doc = format!("<x>{r}</x>");
        assert!(events(&doc).is_err(), "{r} must be rejected (Legal Character WFC)");
    }
}

#[test]
fn char_ref_in_content_malformed_rejected() {
    for r in ["&#;", "&#x;", "&#X41;", "&#+65;", "&#6a;", "&#xG1;"] {
        let doc = format!("<x>{r}</x>");
        assert!(events(&doc).is_err(), "{r} is not a legal CharRef");
    }
}

#[test]
fn literal_illegal_chars_rejected_everywhere() {
    for c in ['\u{0}', '\u{1}', '\u{B}', '\u{C}', '\u{1F}', '\u{FFFE}', '\u{FFFF}'] {
        for doc in [
            format!("<x>{c}</x>"),
            format!("<x a=\"{c}\"/>"),
            format!("<x><!--{c}--></x>"),
            format!("<x><?pi {c}?></x>"),
            format!("<x><![CDATA[{c}]]></x>"),
            format!("<x/><!--{c}-->"),
        ] {
            match events(&doc) {
                Err(XmlError::InvalidChar { char, .. }) => assert_eq!(char, c),
                other => panic!("{doc:?}: expected InvalidChar, got {other:?}"),
            }
        }
    }
}

#[test]
fn illegal_char_position_is_reported() {
    match events("<x>\n  ok\u{1}</x>") {
        Err(XmlError::InvalidChar { pos, .. }) => {
            assert_eq!((pos.line, pos.column, pos.byte_offset), (2, 5, 8));
        }
        other => panic!("expected InvalidChar, got {other:?}"),
    }
}

#[test]
fn legal_whitespace_controls_accepted() {
    events("<x a=\"\t\r\n\">\t\r\n</x>").expect("TAB, CR, LF are legal Chars");
}

/// Regression: the lexer used to decode a fixed 4-byte window after a name,
/// which fails when the window ends mid-codepoint (`x>` + 2 of €'s 3 bytes).
#[test]
fn multibyte_char_right_after_short_name() {
    events("<x>€</x>").expect("3-byte char after a 1-char name");
    events("<ab>𝄞</ab>").expect("4-byte char after a 2-char name");
    events("<x a='€'/>").expect("3-byte char in attribute after short name");
}

/// §2.6: PITarget must be followed by whitespace or `?>`.
#[test]
fn pi_target_needs_whitespace_before_body() {
    events("<x><?pi?></x>").expect("empty PI");
    events("<x><?pi data?></x>").expect("PI with body");
    assert!(events("<x><?a%b?></x>").is_err(), "'%' is not a NameChar and no S follows target");
}

/// §3.1 [Production 40]: attributes are separated by required whitespace.
#[test]
fn attributes_need_whitespace_between() {
    events("<x a='1' b='2'/>").expect("space-separated attributes");
    assert!(events("<x a='1'b='2'/>").is_err(), "missing S between attributes");
}

// ─── §2.8 XML declaration ────────────────────────────────────────────────────

#[test]
fn xml_declaration_forms() {
    for doc in [
        "<?xml version='1.0'?><x/>",
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><x/>",
        "<?xml version = '1.0' encoding = 'ISO-8859-1' standalone = 'yes' ?><x/>",
        "<?xml version='1.0' standalone='no'?><x/>",
        "\u{FEFF}<?xml version='1.0'?><x/>",
        "\u{FEFF}<x/>",
    ] {
        events(doc).unwrap_or_else(|e| panic!("rejected {doc:?}: {e}"));
    }
    for doc in [
        " <?xml version='1.0'?><x/>",                         // not at start
        "<!-- c --><?xml version='1.0'?><x/>",
        "<x/><?xml version='1.0'?>",
        "<?XML version='1.0'?><x/>",                          // case matters
        "<?xml encoding='UTF-8'?><x/>",                       // version required
        "<?xml version='1.0' standalone='yes' encoding='UTF-8'?><x/>", // order
        "<?xml version='1.0'encoding='UTF-8'?><x/>",          // S required
        "<?xml version='1.0' encoding='UTF-8'standalone='yes'?><x/>",
        "<?xml version='1 .0'?><x/>",
        "<?xml version='#1.0'?><x/>",
        "<?xml version='1.0' encoding='_utf8'?><x/>",
        "<?xml version='1.0' encoding='UTF 8'?><x/>",
        "<?xml version='1.0' encoding='a/b'?><x/>",
        "<?xml version='1.0' standalone='YES'?><x/>",
        "<x><?xmL pi?></x>",                                  // reserved target
    ] {
        assert!(events(doc).is_err(), "accepted {doc:?}");
    }
}

#[test]
fn version_number_follows_edition() {
    use expat_rs::Edition;
    let run = |doc: &str, ed| {
        let mut p = Parser::new(doc).with_edition(ed);
        while p.next_event()?.is_some() {}
        Ok::<_, XmlError>(())
    };
    run("<?xml version='1.1'?><x/>", Edition::Fifth).expect("5th ed.: '1.' [0-9]+");
    assert!(run("<?xml version='1.1'?><x/>", Edition::Fourth).is_err(), "4th ed.: only '1.0'");
    assert!(run("<?xml version='2.0'?><x/>", Edition::Fifth).is_err());
}

// ─── §3.1 references in attribute values ─────────────────────────────────────

#[test]
fn attribute_value_references() {
    events("<x a='&amp;&lt;&#65;&#x42;'/>").expect("legal references in AttValue");
    for doc in ["<x a='&'/>", "<x a='a & b'/>", "<x a='&amp'/>", "<x a='&#65'/>",
                "<x a='&#x0;'/>", "<x a='&#5~0;'/>", "<x a='&#xFFFE;'/>"] {
        assert!(events(doc).is_err(), "accepted {doc:?}");
    }
}
