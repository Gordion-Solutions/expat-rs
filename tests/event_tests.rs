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

// ─── §3.3.3 attribute-value normalisation and DTD defaults ─────────────────

fn root_attrs(src: &str) -> Vec<(String, String, bool)> {
    for e in events(src) {
        if let Event::StartElement { attributes, .. } = e {
            return attributes.into_iter()
                .map(|a| (a.name.into_owned(), a.value.into_owned(), a.specified))
                .collect();
        }
    }
    panic!("no start tag in {src:?}");
}

fn attr(src: &str) -> String {
    root_attrs(src).remove(0).1
}

#[test]
fn whitespace_becomes_space_but_char_refs_survive() {
    assert_eq!(attr("<a x='1\t2\n3\r\n4'/>"), "1 2 3 4");
    assert_eq!(attr("<a x='1&#9;2&#10;3&#13;4'/>"), "1\t2\n3\r4");
}

#[test]
fn references_expanded_in_attribute_values() {
    assert_eq!(attr("<a x='&lt;&amp;&#65;'/>"), "<&A");
    // Whitespace in replacement text also becomes spaces, including a LF
    // that came from a char ref in the entity *value*.
    assert_eq!(attr("<!DOCTYPE a [<!ENTITY e 'p&#10;q'>]><a x='[&e;]'/>"), "[p q]");
    // A char ref that survives into the replacement text is still a char ref.
    assert_eq!(attr("<!DOCTYPE a [<!ENTITY e 'p&#38;#10;q'>]><a x='&e;'/>"), "p\nq");
    assert_eq!(attr("<!DOCTYPE a [<!ENTITY e 'v&f;'><!ENTITY f 'w'>]><a x='&e;'/>"), "vw");
}

#[test]
fn non_cdata_types_trim_and_collapse() {
    let dtd = "<!DOCTYPE a [<!ATTLIST a t NMTOKENS #IMPLIED c CDATA #IMPLIED e (x|y) #IMPLIED>]>";
    let attrs = root_attrs(&format!("{dtd}<a t='  p   q  ' c='  p   q  ' e=' x '/>"));
    assert_eq!(attrs, [
        ("t".into(), "p q".into(), true),
        ("c".into(), "  p   q  ".into(), true),
        ("e".into(), "x".into(), true),
    ]);
    // Only spaces collapse; a TAB from a char ref is kept.
    assert_eq!(attr(&format!("{dtd}<a t=' p&#9;q '/>")), "p\tq");
}

#[test]
fn declared_defaults_are_added_unspecified() {
    let dtd = "<!DOCTYPE a [<!ATTLIST a d CDATA 'dv' f CDATA #FIXED 'fv' i CDATA #IMPLIED r CDATA #REQUIRED>]>";
    assert_eq!(root_attrs(&format!("{dtd}<a r='1'/>")), [
        ("r".into(), "1".into(), true),
        ("d".into(), "dv".into(), false),
        ("f".into(), "fv".into(), false),
    ]);
    // A specified value wins over the default.
    assert_eq!(root_attrs(&format!("{dtd}<a d='mine' r='1'/>"))[0], ("d".into(), "mine".into(), true));
}

#[test]
fn defaults_first_declaration_binds_and_are_normalised() {
    let src = "<!DOCTYPE a [<!ENTITY e 'E'>\
               <!ATTLIST a x NMTOKEN '  one  ' y CDATA 'a&e;\tb'>\
               <!ATTLIST a x CDATA 'ignored'>]><a/>";
    assert_eq!(root_attrs(src), [("x".into(), "one".into(), false), ("y".into(), "aE b".into(), false)]);
}

#[test]
fn defaults_apply_inside_entity_content() {
    let src = "<!DOCTYPE d [<!ATTLIST b x CDATA 'dv'><!ENTITY e '<b/>'>]><d>&e;</d>";
    let es = events(src);
    let b = es.iter().find_map(|e| match e {
        Event::StartElement { name, attributes } if name == "b" => Some(attributes.clone()),
        _ => None,
    }).expect("<b> from entity");
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].name.as_ref(), b[0].value.as_ref(), b[0].specified), ("x", "dv", false));
}

#[test]
fn defaults_after_unread_pe_ref_are_not_processed() {
    // §5.1, and W3C xmltest valid/sa 097's pattern.
    let src = "<!DOCTYPE a [<!ATTLIST a x CDATA 'v1'>%p;<!ATTLIST a y CDATA 'v2'>]><a/>";
    assert_eq!(root_attrs(src), [("x".into(), "v1".into(), false)]);
}

// ─── DOCTYPE contents as events ────────────────────────────────────────────

#[test]
fn internal_subset_pis_comments_notations_in_order() {
    let src = "<!DOCTYPE a [<?p1 x?><!NOTATION n PUBLIC 'pub' 'sys'><!--c--><!NOTATION m SYSTEM 's'>]><a/>";
    let es = events(src);
    let dtd: Vec<String> = es.iter()
        .skip_while(|e| !matches!(e, Event::Doctype { .. }))
        .take_while(|e| !matches!(e, Event::StartElement { .. }))
        .map(|e| match e {
            Event::Doctype { name, .. } => format!("doctype:{name}"),
            Event::ProcessingInstruction { target, body } => format!("pi:{target}:{body}"),
            Event::Comment(c) => format!("comment:{c}"),
            Event::NotationDecl { name, public_id, system_id } =>
                format!("notation:{name}:{public_id:?}:{system_id:?}"),
            Event::EndDoctype => "end".into(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(dtd, [
        "doctype:a",
        "pi:p1:x",
        "notation:n:Some(\"pub\"):Some(\"sys\")",
        "comment:c",
        "notation:m:None:Some(\"s\")",
        "end",
    ]);
}
