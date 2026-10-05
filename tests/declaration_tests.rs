//! Declaration events (element, attribute-list, entity, DOCTYPE details).
//! Expectations mirror libexpat 2.8.2's declaration handlers on the same
//! input (capi/phase1/declprobe.c).

use expat_rs::{ContentKind, ContentModel, ContentNode, Event, Parser, Quantifier};

const DOC: &str = "<!DOCTYPE r PUBLIC '-//X//Y//EN' 'r.dtd' [\n\
    <!ELEMENT e1 EMPTY><!ELEMENT e2 ANY><!ELEMENT e3 (#PCDATA)><!ELEMENT e4 (#PCDATA|a|b)*>\n\
    <!ELEMENT e5 (a)><!ELEMENT e6 (a,(b|c)*,d?)+>\n\
    <!ATTLIST r c CDATA 'x  y' n NMTOKENS #IMPLIED q ID #REQUIRED f CDATA #FIXED 'fv' \
    en ( a | b ) 'a' no NOTATION ( x | y ) #IMPLIED>\n\
    <!ENTITY g 'v&#65;&amp;'><!ENTITY % pe 'pv'><!ENTITY x SYSTEM 'x.xml'>\
    <!ENTITY u PUBLIC '-//U//EN' 'u.gif' NDATA gif>\n\
    ]><r q='1'/>";

/// libexpat-style rendering of a content model, as declprobe.c prints it.
fn render(m: &ContentModel, n: &ContentNode) -> String {
    let kind = match n.kind {
        ContentKind::Empty => "EMPTY", ContentKind::Any => "ANY", ContentKind::Mixed => "MIXED",
        ContentKind::Name => "NAME", ContentKind::Choice => "CHOICE", ContentKind::Seq => "SEQ",
    };
    let mut s = kind.to_string();
    if let Some(name) = &n.name { s += &format!(":{name}"); }
    if !n.children.is_empty() {
        s += &format!("[{}]", m.children(n).map(|c| render(m, c)).collect::<Vec<_>>().join(" "));
    }
    s += match n.quant { Quantifier::None => "", Quantifier::Opt => "?", Quantifier::Rep => "*", Quantifier::Plus => "+" };
    s
}

fn lines(src: &str) -> Vec<String> {
    let mut p = Parser::new(src);
    let mut out = Vec::new();
    let opt = |o: &Option<std::borrow::Cow<'_, str>>| o.as_deref().unwrap_or("(null)").to_string();
    while let Some(e) = p.next_event().unwrap() {
        match e {
            Event::Doctype { name, system_id, public_id, has_internal_subset, .. } => out.push(format!(
                "doctype {name} sys={} pub={} internal={}", opt(&system_id), opt(&public_id), has_internal_subset as u8)),
            Event::ElementDecl { name, model } => out.push(format!("element {name}: {}", render(&model, model.root()))),
            Event::AttlistDecl { element, attribute, att_type, default, required } => out.push(format!(
                "attlist {element} {attribute} type={att_type} default={} required={}", opt(&default), required as u8)),
            Event::EntityDecl { name, parameter, value, system_id, public_id, notation } => out.push(format!(
                "entity {name} param={} value={} sys={} pub={} notation={}", parameter as u8,
                value.map_or("(null)".to_string(), |v| format!("[{v}]")), opt(&system_id), opt(&public_id), opt(&notation))),
            _ => {}
        }
    }
    out
}

#[test]
fn declarations_match_libexpat() {
    assert_eq!(lines(DOC), [
        "doctype r sys=r.dtd pub=-//X//Y//EN internal=1",
        "element e1: EMPTY",
        "element e2: ANY",
        "element e3: MIXED",
        "element e4: MIXED[NAME:a NAME:b]*",
        "element e5: SEQ[NAME:a]",
        "element e6: SEQ[NAME:a CHOICE[NAME:b NAME:c]* NAME:d?]+",
        "attlist r c type=CDATA default=x  y required=0",
        "attlist r n type=NMTOKENS default=(null) required=0",
        "attlist r q type=ID default=(null) required=1",
        "attlist r f type=CDATA default=fv required=1",
        "attlist r en type=(a|b) default=a required=0",
        "attlist r no type=NOTATION(x|y) default=(null) required=0",
        "entity g param=0 value=[vA&amp;] sys=(null) pub=(null) notation=(null)",
        "entity pe param=1 value=[pv] sys=(null) pub=(null) notation=(null)",
        "entity x param=0 value=(null) sys=x.xml pub=(null) notation=(null)",
        "entity u param=0 value=(null) sys=u.gif pub=-//U//EN notation=gif",
    ]);
}

#[test]
fn doctype_without_ids_or_subset() {
    assert_eq!(lines("<!DOCTYPE r><r/>"), ["doctype r sys=(null) pub=(null) internal=0"]);
    assert_eq!(lines("<!DOCTYPE r SYSTEM 's'><r/>"), ["doctype r sys=s pub=(null) internal=0"]);
}

#[test]
fn entity_and_attlist_declarations_after_unread_pe_are_not_reported() {
    // §5.1, as for processing; element declarations still are.
    let got = lines("<!DOCTYPE r [%p;<!ENTITY e 'v'><!ATTLIST r a CDATA #IMPLIED><!ELEMENT r ANY>]><r/>");
    assert_eq!(got, ["doctype r sys=(null) pub=(null) internal=1", "element r: ANY"]);
}

#[test]
fn deep_content_model_is_safe_to_clone_print_and_drop() {
    let n = 100_000;
    let doc = format!("<!DOCTYPE a [<!ELEMENT a {}b{}>]><a/>", "(".repeat(n), ")".repeat(n));
    let mut p = Parser::new(&doc);
    while let Some(e) = p.next_event().unwrap() {
        if let Event::ElementDecl { model, .. } = e {
            assert_eq!(model.nodes.len(), n + 1);
            let copy = model.clone();
            assert_eq!(copy, model);
            assert!(format!("{model:?}").len() > n);
        }
    }
}
