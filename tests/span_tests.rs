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

#[test]
fn input_offsets_count_bytes_before_decoding() {
    use expat_rs::StreamParser;
    // Where each element starts, in input bytes, for one document in
    // several encodings: <r>é<x/>𝄞<y/></r>.
    let doc = "<r>é<x/>𝄞<y/></r>";
    let starts = |input: &[u8], chunk: usize| {
        let mut p = StreamParser::new();
        let mut got = Vec::new();
        let mut record = |l: expat_rs::Located<'_>| if let expat_rs::Event::StartElement { .. } = l.event { got.push(l.input_start) };
        for c in input.chunks(chunk) {
            p.feed_located(c, &mut record).unwrap();
        }
        p.finish_located(&mut record).unwrap();
        got
    };
    let latin1_doc = "<?xml version='1.0' encoding='iso-8859-1'?><r>é<x/>x<y/></r>";
    let latin1: Vec<u8> = latin1_doc.chars().map(|c| c as u8).collect();
    let utf16le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(doc.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let utf8_bom: Vec<u8> = [0xEF, 0xBB, 0xBF].into_iter().chain(doc.bytes()).collect();
    for chunk in [usize::MAX, 1] {
        assert_eq!(starts(doc.as_bytes(), chunk), [0, 5, 13], "UTF-8: é is 2 bytes, 𝄞 4");
        assert_eq!(starts(&latin1, chunk), [43, 47, 52], "Latin-1: one byte per character (declaration 43)");
        assert_eq!(starts(&utf16le, chunk), [2, 10, 22], "UTF-16: BOM 2, then 2 per character, 4 for 𝄞");
        assert_eq!(starts(&utf8_bom, chunk), [3, 8, 16], "UTF-8 with BOM");
    }
}
