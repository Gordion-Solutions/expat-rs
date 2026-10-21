//! External parsed entities read by the caller (`with_external_entity_refs`,
//! `EntityContext::parser`). Expected behaviour is libexpat 2.8.2's, from
//! black-box runs of its external entity parser.

use expat_rs::{EntityContext, ErrorKind, Event, StreamParser, XmlError};

const DOC: &str = "<!DOCTYPE r [<!ENTITY e SYSTEM 'e.xml'><!ENTITY i 'int'>]><r>a&e;b</r>";

/// Parse `doc`, returning a compact event log and the context of each
/// external entity reference.
fn parse(parser: StreamParser<'_>, doc: &str) -> (Vec<String>, Vec<EntityContext>) {
    let mut p = parser;
    let (mut log, mut contexts) = (Vec::new(), Vec::new());
    let mut record = |e: Event<'_>| match e {
        Event::StartElement { name, namespace, .. } => log.push(format!("<{}{name}>", namespace.map(|n| format!("{{{n}}}")).unwrap_or_default())),
        Event::EndElement(name) => log.push(format!("</{name}>")),
        Event::Text(t) => log.push(t.into_owned()),
        Event::SkippedEntity(name) => log.push(format!("skipped {name}")),
        Event::ExternalEntityRef { name, system_id, public_id, context } => {
            log.push(format!("external {name} {system_id} {public_id:?}"));
            contexts.push(context);
        }
        _ => {}
    };
    p.feed_str(doc, &mut record).unwrap();
    p.finish(&mut record).unwrap();
    (log, contexts)
}

/// Parse an entity's content with a child parser.
fn child(context: &EntityContext, content: &str) -> Result<Vec<String>, XmlError> {
    let mut p = context.parser();
    let mut log = Vec::new();
    let mut record = |e: Event<'_>| match e {
        Event::StartElement { name, namespace, .. } => log.push(format!("<{}{name}>", namespace.map(|n| format!("{{{n}}}")).unwrap_or_default())),
        Event::EndElement(name) => log.push(format!("</{name}>")),
        Event::Text(t) => log.push(t.into_owned()),
        _ => {}
    };
    p.feed(content.as_bytes(), &mut record)?;
    p.finish(&mut record)?;
    Ok(log)
}

#[test]
fn without_the_option_external_entities_are_skipped() {
    let (log, _) = parse(StreamParser::new(), DOC);
    assert_eq!(log, ["<r>", "a", "skipped e", "b", "</r>"]);
}

#[test]
fn references_are_reported_for_the_caller() {
    let (log, contexts) = parse(StreamParser::new().with_external_entity_refs(), DOC);
    assert_eq!(log, ["<r>", "a", "external e e.xml None", "b", "</r>"]);
    assert_eq!(contexts[0].name(), "e");
}

#[test]
fn entity_content_is_parsed_with_the_documents_entities() {
    let (_, contexts) = parse(StreamParser::new().with_external_entity_refs(), DOC);
    let got = child(&contexts[0], "<?xml encoding='utf-8'?><x/>t&i;<y/>").unwrap();
    assert_eq!(got, ["<x>", "</x>", "t", "int", "<y>", "</y>"]);
    assert_eq!(child(&contexts[0], "<?xml version='1.0' encoding='utf-8'?>hi").unwrap(), ["hi"]);
    assert_eq!(child(&contexts[0], "").unwrap(), Vec::<String>::new(), "empty content is fine");
}

#[test]
fn entity_content_errors() {
    let (_, contexts) = parse(StreamParser::new().with_external_entity_refs(), DOC);
    let kind = |content: &str| child(&contexts[0], content).unwrap_err().kind();
    assert_eq!(kind("<?xml version='1.0'?>hi"), ErrorKind::TextDecl, "text declaration without encoding");
    assert_eq!(kind("<?xml version='1.0' encoding='utf-8' standalone='yes'?>hi"), ErrorKind::TextDecl);
    assert_eq!(kind("<x>"), ErrorKind::AsyncEntity, "unclosed element");
    assert_eq!(kind("<!DOCTYPE z><z/>"), ErrorKind::InvalidToken);
    assert_eq!(kind("&nope;"), ErrorKind::UndefinedEntity);
}

#[test]
fn namespace_declarations_in_scope_carry_into_the_entity() {
    let doc = "<!DOCTYPE r [<!ENTITY e SYSTEM 'e.xml'>]><r xmlns:p='urn:p'>&e;</r>";
    let (_, contexts) = parse(StreamParser::new().with_namespaces().with_external_entity_refs(), doc);
    assert_eq!(child(&contexts[0], "<p:x/>").unwrap(), ["<{urn:p}p:x>", "</p:x>"]);
}

#[test]
fn an_entity_may_not_refer_to_itself() {
    let (_, contexts) = parse(StreamParser::new().with_external_entity_refs(), DOC);
    let err = child(&contexts[0], "<x>&e;</x>").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::RecursiveEntityRef);
}

#[test]
fn nested_external_references_are_reported_too() {
    let doc = "<!DOCTYPE r [<!ENTITY e SYSTEM 'e.xml'><!ENTITY f SYSTEM 'f.xml'>]><r>&e;</r>";
    let (_, contexts) = parse(StreamParser::new().with_external_entity_refs(), doc);
    let mut p = contexts[0].parser();
    let mut inner = Vec::new();
    p.feed(b"<x>&f;</x>", |e| if let Event::ExternalEntityRef { context, .. } = e { inner.push(context) }).unwrap();
    p.finish(|_| {}).unwrap();
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0].name(), "f");
    // f, read from inside e, may not refer back to e.
    assert_eq!(child(&inner[0], "&e;").unwrap_err().kind(), ErrorKind::RecursiveEntityRef);
}
