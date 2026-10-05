//! Spans and raw source text on events (`next_located`, `feed_located`).

use expat_rs::{Event, Located, Parser, Position, StreamParser};

fn located(src: &str) -> Vec<(String, String)> {
    let mut p = Parser::new(src);
    let mut out = Vec::new();
    while let Some(l) = p.next_located().unwrap() {
        out.push((kind(&l.event), l.raw.to_string()));
    }
    out
}

fn kind(e: &Event<'_>) -> String {
    let s = format!("{e:?}");
    s.split(['(', ' ', '{']).next().unwrap().to_string()
}

#[test]
fn raw_text_of_each_event() {
    let src = "<?xml version='1.0'?>\n<!-- c --><?pi d?><r a='1'>t&amp;&#65;<![CDATA[x]]><e/></r>";
    assert_eq!(located(src), [
        ("XmlDecl", "<?xml version='1.0'?>"),
        ("Comment", "<!-- c -->"),
        ("ProcessingInstruction", "<?pi d?>"),
        ("StartElement", "<r a='1'>"),
        ("Text", "t"),
        ("Text", "&amp;"),
        ("Text", "&#65;"),
        ("CData", "<![CDATA[x]]>"),
        ("StartElement", "<e/>"),
        ("EndElement", "<e/>"),
        ("EndElement", "</r>"),
    ].map(|(k, r)| (k.to_string(), r.to_string())));
}

#[test]
fn internal_subset_declarations_and_entity_content() {
    let src = "<!DOCTYPE r [<?p x?><!NOTATION n SYSTEM 's'><!ENTITY e '<b/>'>]><r>&e;</r>";
    let got = located(src);
    let dtd = "<!DOCTYPE r [<?p x?><!NOTATION n SYSTEM 's'><!ENTITY e '<b/>'>]>";
    assert_eq!(got[0], ("Doctype".into(), dtd.into()));
    assert_eq!(got[1], ("ProcessingInstruction".into(), "<?p x?>".into()));
    assert_eq!(got[2], ("NotationDecl".into(), "<!NOTATION n SYSTEM 's'>".into()));
    assert_eq!(got[3], ("EntityDecl".into(), "<!ENTITY e '<b/>'>".into()));
    assert_eq!(got[4], ("EndDoctype".into(), dtd.into()));
    // Events from the entity carry the span of the reference.
    assert_eq!(got[6], ("StartElement".into(), "&e;".into()));
    assert_eq!(got[7], ("EndElement".into(), "&e;".into()));
}

#[test]
fn span_positions_agree_with_byte_offsets() {
    let src = "<a>\n  é<b x='y\nz'/>\r\n  <!--\n-->text</a>";
    let mut p = Parser::new(src);
    while let Some(Located { span, raw, .. }) = p.next_located().unwrap() {
        assert_eq!(raw, &src[span.start.byte_offset..span.end.byte_offset]);
        for pos in [span.start, span.end] {
            assert_eq!(pos, position_of(src, pos.byte_offset), "{raw:?}");
        }
    }
}

/// Line and character column of a byte offset, counted from the start.
fn position_of(src: &str, byte: usize) -> Position {
    let before = &src[..byte];
    let line = 1 + before.matches('\n').count() as u32;
    let col = 1 + before.rsplit('\n').next().unwrap().chars().count() as u32;
    Position { line, column: col, byte_offset: byte }
}

#[test]
fn streaming_spans_match_whole_document() {
    let src = "<?xml version='1.0'?><!DOCTYPE r [<!ENTITY e 'v'>]><r a='1'>one\r\ntwo&e;<x/></r>\n";
    let whole = joined(collect_whole(src));
    for cut in 0..=src.len() {
        let mut p = StreamParser::new();
        let mut got = Vec::new();
        p.feed_located(&src.as_bytes()[..cut], |l| got.push(simple(&l))).unwrap();
        p.feed_located(&src.as_bytes()[cut..], |l| got.push(simple(&l))).unwrap();
        p.finish_located(|l| got.push(simple(&l))).unwrap();
        assert_eq!(joined(got), whole, "cut at {cut}");
    }
}

type Simple = (String, usize, usize, String);

fn simple(l: &Located<'_>) -> Simple {
    (kind(&l.event), l.span.start.byte_offset, l.span.end.byte_offset, l.raw.to_string())
}

fn collect_whole(src: &str) -> Vec<Simple> {
    let mut p = Parser::new(src);
    let mut out = Vec::new();
    while let Some(l) = p.next_located().unwrap() {
        out.push(simple(&l));
    }
    out
}

/// Merge adjacent text pieces (streaming may split a text run).
fn joined(events: Vec<Simple>) -> Vec<Simple> {
    let mut out: Vec<Simple> = Vec::new();
    for e in events {
        match out.last_mut() {
            Some(last) if last.0 == "Text" && e.0 == "Text" && last.2 == e.1 => {
                last.2 = e.2;
                last.3.push_str(&e.3);
            }
            _ => out.push(e),
        }
    }
    out
}
