//! Inputs whose cost could grow faster than their size: each of these was
//! quadratic or crashed before being fixed. The sizes are ones that took
//! seconds (or overflowed the stack) with the old code; the assertions
//! check behaviour, not timing.

use expat_rs::{Event, Parser, StreamParser, XmlError};

fn parse(src: &str, ns: bool) -> Result<usize, XmlError> {
    let mut p = Parser::new(src);
    if ns { p = p.with_namespaces(); }
    let mut n = 0;
    while p.next_event()?.is_some() { n += 1; }
    Ok(n)
}

#[test]
fn deeply_nested_content_model_does_not_overflow_the_stack() {
    // Was parsed recursively: 100k nested groups aborted with a stack overflow.
    let n = 200_000;
    let doc = format!("<!DOCTYPE a [<!ELEMENT a {}b{}>]><a/>", "(".repeat(n), ")".repeat(n));
    parse(&doc, false).expect("deep but well-formed content model");
    // Still checked: one ')' short.
    let doc = format!("<!DOCTYPE a [<!ELEMENT a {}b{}>]><a/>", "(".repeat(n), ")".repeat(n - 1));
    assert!(parse(&doc, false).is_err());
}

#[test]
fn many_attributes_on_one_element() {
    // Duplicate check compared every pair: 40k attributes took ~3 s.
    let attrs: Vec<String> = (0..100_000).map(|i| format!("a{i}='1'")).collect();
    let doc = format!("<a {}/>", attrs.join(" "));
    parse(&doc, false).expect("100k distinct attributes");
    let dup = format!("<a {} a5='2'/>", attrs.join(" "));
    assert!(parse(&dup, false).is_err(), "duplicate still detected");
}

#[test]
fn many_namespaced_attributes() {
    let attrs: Vec<String> = (0..50_000).map(|i| format!("p:a{i}='1'")).collect();
    parse(&format!("<a xmlns:p='u' {}/>", attrs.join(" ")), true).expect("50k namespaced attributes");
    // Same expanded name through two prefixes is still caught.
    parse(&format!("<a xmlns:p='u' xmlns:q='u' {} q:a7='2'/>", attrs.join(" ")), true)
        .expect_err("duplicate expanded name");
}

#[test]
fn many_declared_attributes_used_together() {
    let n = 30_000;
    let decls: Vec<String> = (0..n).map(|i| format!("a{i} NMTOKEN 'd{i}'")).collect();
    let uses: Vec<String> = (0..n).step_by(2).map(|i| format!("a{i}=' v '")).collect();
    let doc = format!("<!DOCTYPE a [<!ATTLIST a {}>]><a {}/>", decls.join(" "), uses.join(" "));
    let mut p = Parser::new(&doc);
    while let Some(e) = p.next_event().unwrap() {
        if let Event::StartElement { attributes, .. } = e {
            assert_eq!(attributes.len(), n, "specified + defaulted");
            assert!(attributes.iter().filter(|a| a.specified).all(|a| a.value == "v"), "NMTOKEN normalised");
        }
    }
}

#[test]
fn deep_nesting_with_namespace_declarations() {
    // Prefix lookup walked every open scope: 50k levels took ~7 s.
    let n = 100_000;
    let open: String = (0..n).map(|i| format!("<p:b xmlns:x{i}='v'>")).collect();
    let doc = format!("<p:a xmlns:p='u'>{open}{}</p:a>", "</p:b>".repeat(n));
    parse(&doc, true).expect("deep namespaced document");
}

#[test]
fn huge_construct_streamed_in_tiny_chunks() {
    // Re-scanned from its start on every chunk: a 4 MB comment in 1 KB
    // chunks took ~20 s (libexpat CVE-2023-52425 class). Reparse deferral
    // makes it linear.
    let doc = format!("<a><!--{}--></a>", "x".repeat(8_000_000));
    let mut p = StreamParser::new();
    let mut comment_len = 0;
    for chunk in doc.as_bytes().chunks(256) {
        p.feed(chunk, |e| if let Event::Comment(c) = e { comment_len = c.len() }).unwrap();
    }
    p.finish(|e| if let Event::Comment(c) = e { comment_len = c.len() }).unwrap();
    assert_eq!(comment_len, 8_000_000);
}

#[test]
fn deferral_still_delivers_everything_in_order() {
    let doc = format!("<a><b/><!--{}--><c>t</c></a>", "y".repeat(100_000));
    let mut names = Vec::new();
    let mut p = StreamParser::new();
    let mut take = |e: Event<'_>| match e {
        Event::StartElement { name, .. } => names.push(name.into_owned()),
        Event::Comment(_) => names.push("comment".into()),
        _ => {}
    };
    for chunk in doc.as_bytes().chunks(10) {
        p.feed(chunk, &mut take).unwrap();
    }
    p.finish(&mut take).unwrap();
    assert_eq!(names, ["a", "b", "comment", "c"]);
}
