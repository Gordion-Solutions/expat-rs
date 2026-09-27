//! Event content: what callers actually receive.

use expat_rs::{Event, Parser};

/// Concatenated text between the root's start and end, plus the root's
/// attributes as (name, value, specified).
fn text(src: &str) -> String {
    let mut p = Parser::new(src);
    let mut out = String::new();
    while let Some(e) = p.next_event().unwrap_or_else(|e| panic!("{src:?}: {e}")) {
        if let Event::Text(t) | Event::CData(t) = e {
            out.push_str(&t);
        }
    }
    out
}

fn events(src: &str) -> Vec<Event<'_>> {
    let mut p = Parser::new(src);
    let mut out = Vec::new();
    while let Some(e) = p.next_event().unwrap_or_else(|e| panic!("{src:?}: {e}")) {
        out.push(e);
    }
    out
}

// ─── §2.11 line endings ─────────────────────────────────────────────────────

#[test]
fn line_endings_normalised() {
    assert_eq!(text("<a>1\r\n2\r3\n4</a>"), "1\n2\n3\n4");
    assert_eq!(text("<a><![CDATA[x\r\ny]]></a>"), "x\ny");
    let es = events("<a><!--c\r\nd--><?p x\ry?></a>");
    assert!(es.iter().any(|e| matches!(e, Event::Comment(c) if c == "c\nd")), "{es:?}");
    assert!(es.iter().any(|e| matches!(e, Event::ProcessingInstruction { body, .. } if body == "x\ny")), "{es:?}");
}

#[test]
fn char_ref_newline_is_not_normalised() {
    // Only literal line breaks are normalised; &#13; is a real CR.
    assert_eq!(text("<a>&#13;&#10;</a>"), "\r\n");
}

// ─── references in content ──────────────────────────────────────────────────

#[test]
fn char_and_builtin_refs_become_text() {
    assert_eq!(text("<a>&#65;&#x42;&lt;&gt;&amp;&apos;&quot;</a>"), "AB<>&'\"");
    assert_eq!(text("<a>&#x10FFFF;</a>"), "\u{10FFFF}");
}

#[test]
fn borrowed_when_unchanged() {
    let src = "<a>plain text</a>";
    let es = events(src);
    assert!(es.iter().any(|e| matches!(e, Event::Text(std::borrow::Cow::Borrowed("plain text")))), "{es:?}");
}

// ─── entity references in content deliver their events ─────────────────────

fn names(es: &[Event<'_>]) -> Vec<String> {
    es.iter().filter_map(|e| match e {
        Event::StartElement { name, .. } => Some(format!("<{name}>")),
        Event::EndElement(name) => Some(format!("</{name}>")),
        Event::Text(t) => Some(t.to_string()),
        Event::SkippedEntity(n) => Some(format!("skipped:{n}")),
        _ => None,
    }).collect()
}

#[test]
fn internal_entity_text_and_markup() {
    let src = "<!DOCTYPE d [<!ENTITY e 'x<b a=\"1\">y</b><c/>&f;'><!ENTITY f 'z'>]><d>&e;!</d>";
    assert_eq!(names(&events(src)), ["<d>", "x", "<b>", "y", "</b>", "<c>", "</c>", "z", "!", "</d>"]);
}

#[test]
fn entity_value_line_endings_and_char_refs() {
    // Literal CRLF in the entity value is normalised; &#13; survives.
    assert_eq!(text("<!DOCTYPE d [<!ENTITY e 'a\r\nb&#13;c'>]><d>&e;</d>"), "a\nb\rc");
    // Char refs in the value expand at declaration; &#38;amp; becomes &amp;
    // in the replacement text, then '&' when parsed.
    assert_eq!(text("<!DOCTYPE d [<!ENTITY e '&#38;amp;'>]><d>&e;</d>"), "&");
}

#[test]
fn unread_entities_are_skipped_entities() {
    assert_eq!(names(&events("<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'>]><d>&e;</d>")),
               ["<d>", "skipped:e", "</d>"]);
    // Undeclared, where Entity Declared is only a validity constraint.
    assert_eq!(names(&events("<!DOCTYPE d SYSTEM 'd.dtd'><d>&u;</d>")),
               ["<d>", "skipped:u", "</d>"]);
}

#[test]
fn loaded_external_entity_events() {
    let src = "<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'>]><d>&e;</d>";
    let mut p = Parser::new(src)
        .with_external_loader(|_, _| Ok(Some("<?xml encoding='UTF-8'?>ext\r\n<i/>".into())));
    let mut es = Vec::new();
    while let Some(e) = p.next_event().unwrap() { es.push(e); }
    assert_eq!(names(&es), ["<d>", "ext\n", "<i>", "</i>", "</d>"]);
}
