//! Namespace processing (W3C Namespaces in XML 1.0), opt-in via
//! `Parser::with_namespaces`.

use expat_rs::{Event, Parser, XmlError};

fn ns_events(src: &str) -> Result<Vec<String>, XmlError> {
    let mut p = Parser::new(src).with_namespaces();
    let mut out = Vec::new();
    while let Some(e) = p.next_event()? {
        out.push(match e {
            Event::StartElement { name, namespace, attributes } => {
                let attrs: Vec<String> = attributes.iter()
                    .map(|a| format!("{}[{}]={}", a.name, a.namespace.as_deref().unwrap_or("-"), a.value))
                    .collect();
                format!("<{name}[{}] {}>", namespace.as_deref().unwrap_or("-"), attrs.join(" "))
            }
            Event::EndElement(name) => format!("</{name}>"),
            Event::StartNamespace { prefix, uri } =>
                format!("+ns {}={}", prefix.as_deref().unwrap_or("#default"), uri.as_deref().unwrap_or("#none")),
            Event::EndNamespace { prefix } => format!("-ns {}", prefix.as_deref().unwrap_or("#default")),
            _ => continue,
        });
    }
    Ok(out)
}

fn ok(src: &str) -> Vec<String> {
    ns_events(src).unwrap_or_else(|e| panic!("rejected {src:?}: {e}"))
}

fn bad(src: &str) {
    assert!(ns_events(src).is_err(), "accepted {src:?}");
}

#[test]
fn off_by_default() {
    let mut p = Parser::new("<a:b xmlns:a='u'/>");
    match p.next_event().unwrap().unwrap() {
        Event::StartElement { namespace, attributes, .. } => {
            assert_eq!(namespace, None);
            assert_eq!(attributes.len(), 1, "xmlns stays an ordinary attribute");
        }
        other => panic!("{other:?}"),
    }
    // Plain XML 1.0 allows names that are not QNames.
    let mut p = Parser::new("<a:b:c/>");
    while p.next_event().expect("not a namespace error without namespaces").is_some() {}
}

#[test]
fn prefixes_and_default_namespace_resolve() {
    assert_eq!(ok("<r xmlns='d' xmlns:p='pu'><p:e p:x='1' y='2'/><e/></r>"), [
        "+ns #default=d", "+ns p=pu", "<r[d] >",
        "<p:e[pu] p:x[pu]=1 y[-]=2>", "</p:e>",
        "<e[d] >", "</e>",
        "</r>", "-ns p", "-ns #default",
    ]);
}

#[test]
fn rebinding_and_undeclaring_default() {
    assert_eq!(ok("<r xmlns:p='one'><p:a xmlns:p='two'/><p:b/></r>"), [
        "+ns p=one", "<r[-] >",
        "+ns p=two", "<p:a[two] >", "</p:a>", "-ns p",
        "<p:b[one] >", "</p:b>",
        "</r>", "-ns p",
    ]);
    let es = ok("<r xmlns='d'><e xmlns=''/></r>");
    assert!(es.contains(&"+ns #default=#none".to_string()) && es.contains(&"<e[-] >".to_string()), "{es:?}");
}

#[test]
fn xml_prefix_is_predeclared() {
    assert_eq!(ok("<r xml:lang='en'/>")[0], "<r[-] xml:lang[http://www.w3.org/XML/1998/namespace]=en>");
    ok("<r xmlns:xml='http://www.w3.org/XML/1998/namespace'/>");
}

#[test]
fn namespace_constraints() {
    bad("<p:r/>");                                   // unbound element prefix
    bad("<r p:a='1'/>");                             // unbound attribute prefix
    bad("<a:b:c xmlns:a='u'/>");                     // not a QName
    bad("<r xmlns:p=''/>");                          // no unbinding in NS 1.0
    bad("<r xmlns:xml='u'/>");
    bad("<r xmlns:x='http://www.w3.org/XML/1998/namespace'/>");
    bad("<r xmlns='http://www.w3.org/XML/1998/namespace'/>");
    bad("<r xmlns:xmlns='http://www.w3.org/2000/xmlns/'/>");
    bad("<r xmlns:x='http://www.w3.org/2000/xmlns/'/>");
    bad("<xmlns:r/>");
    bad("<r xmlns:a='u' xmlns:b='u' a:x='1' b:x='2'/>"); // same expanded name
    ok("<r xmlns:a='u' xmlns:b='v' a:x='1' b:x='2' x='3'/>");
    bad("<r><?a:b x?></r>");                          // colon in PI target
    bad("<!DOCTYPE r [<!ENTITY a:b 'x'>]><r/>");      // colon in entity name
    bad("<!DOCTYPE r [<!NOTATION a:b SYSTEM 'n'>]><r/>");
}

#[test]
fn declarations_defaulted_from_the_dtd() {
    let src = "<!DOCTYPE r [<!ATTLIST r xmlns CDATA #FIXED 'd' xmlns:p CDATA 'pu'>]><r><p:e/></r>";
    let es = ok(src);
    assert_eq!(es[..3], ["+ns #default=d".to_string(), "+ns p=pu".into(), "<r[d] >".into()]);
    assert!(es.contains(&"<p:e[pu] >".to_string()), "{es:?}");
}

#[test]
fn elements_from_entities_are_resolved_in_scope() {
    let src = "<!DOCTYPE r [<!ENTITY e '<p:x/>'>]><r xmlns:p='pu'>&e;</r>";
    assert!(ok(src).contains(&"<p:x[pu] >".to_string()));
    bad("<!DOCTYPE r [<!ENTITY e '<p:x/>'>]><r>&e;</r>");
}

#[test]
fn local_name_helper() {
    assert_eq!(expat_rs::local_name("p:local"), "local");
    assert_eq!(expat_rs::local_name("plain"), "plain");
}
