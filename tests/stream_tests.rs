//! Incremental parsing: StreamParser must produce exactly what Parser
//! produces on the whole document, however the input is split. Character
//! data may arrive in more pieces when streamed (as documented on
//! `Event::Text`), so consecutive text events are joined before comparing.

use expat_rs::{Event, Parser, StreamParser, XmlError};

/// A readable, owned rendering of an event.
fn show(e: &Event<'_>) -> String {
    format!("{e:?}")
}

/// Events with consecutive text pieces joined into one.
fn joined(events: Vec<String>) -> Vec<String> {
    let text = |e: &str| e.strip_prefix("Text(\"").and_then(|t| t.strip_suffix("\")")).map(str::to_string);
    let mut out: Vec<String> = Vec::new();
    for e in events {
        match (text(&e), out.last().and_then(|l| text(l))) {
            (Some(t), Some(prev)) => *out.last_mut().unwrap() = format!("Text(\"{prev}{t}\")"),
            _ => out.push(e),
        }
    }
    out
}

fn whole(src: &str, ns: bool) -> Result<Vec<String>, XmlError> {
    let mut p = Parser::new(src);
    if ns { p = p.with_namespaces(); }
    let mut out = Vec::new();
    while let Some(e) = p.next_event()? { out.push(show(&e)); }
    Ok(joined(out))
}

/// Feed `bytes` split at `cuts`, collecting events.
fn streamed(bytes: &[u8], cuts: &[usize], ns: bool) -> Result<Vec<String>, XmlError> {
    let mut p = StreamParser::new();
    if ns { p = p.with_namespaces(); }
    let mut out = Vec::new();
    let mut from = 0;
    for &to in cuts.iter().chain(std::iter::once(&bytes.len())) {
        p.feed(&bytes[from..to], |e| out.push(show(&e)))?;
        from = to;
    }
    p.finish(|e| out.push(show(&e)))?;
    Ok(joined(out))
}

const DOCS: &[&str] = &[
    "<a/>",
    "<?xml version='1.0' encoding='UTF-8'?>\r\n<!-- c -->\r\n<root a=\"1\" b='x&amp;y'>text\r\nmore</root>\r\n",
    "\u{FEFF}<?xml version='1.0'?><r>é€𝄞 ünïcödé</r>",
    "<r><![CDATA[ ]] ]> ]]]]><![CDATA[x]]></r>",
    "<r>a]]b] ]</r>",
    "<r>&#65;&#x42;&lt;&gt;&amp;&apos;&quot;\r\r\n\n</r>",
    "<!DOCTYPE r [\n<!ELEMENT r ANY>\n<!ATTLIST r d CDATA 'dv' t NMTOKENS ' a  b '>\n<!ENTITY e 'x<b>y</b>&f;'>\n<!ENTITY f 'z'>\n<?pi data?>\n<!NOTATION n SYSTEM 's'>\n]>\n<r>&e;</r>",
    "<r xmlns='d' xmlns:p='pu'><p:e p:x='1'/><e/></r>",
    "<a><b><c/></b><d>text</d></a><!-- trailing --><?pi?>",
    "<r>long text without markup that runs across many chunks and has no line breaks at all</r>",
];

#[test]
fn every_two_way_split_matches_whole_document() {
    for doc in DOCS {
        let want = whole(doc, true).unwrap_or_else(|e| panic!("{doc:?}: {e}"));
        let bytes = doc.as_bytes();
        for cut in 0..=bytes.len() {
            let got = streamed(bytes, &[cut], true).unwrap_or_else(|e| panic!("{doc:?} cut {cut}: {e}"));
            assert_eq!(got, want, "{doc:?} split at byte {cut}");
        }
    }
}

#[test]
fn byte_at_a_time_matches_whole_document() {
    for doc in DOCS {
        let want = whole(doc, true).unwrap();
        let cuts: Vec<usize> = (1..doc.len()).collect();
        assert_eq!(streamed(doc.as_bytes(), &cuts, true).unwrap(), want, "{doc:?}");
    }
}

#[test]
fn streamed_text_really_is_split_and_joins_back() {
    // Guard against the join hiding everything: streaming must actually
    // deliver text in several pieces here, and they must join back up.
    let doc = "<r>one\r\ntwo]]three</r>";
    let mut p = StreamParser::new();
    let mut pieces = Vec::new();
    for b in doc.as_bytes().chunks(3) {
        p.feed(b, |e| if let Event::Text(t) = e { pieces.push(t.into_owned()) }).unwrap();
    }
    p.finish(|e| if let Event::Text(t) = e { pieces.push(t.into_owned()) }).unwrap();
    assert!(pieces.len() > 1, "{pieces:?}");
    assert_eq!(pieces.concat(), "one\ntwo]]three");
}

#[test]
fn utf16_input_in_pieces() {
    let doc = "<?xml version='1.0' encoding='UTF-16'?><r>𝄞€</r>";
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(doc.encode_utf16().flat_map(u16::to_le_bytes));
    let want = whole(doc, false).unwrap();
    for cut in 0..=bytes.len() {
        assert_eq!(streamed(&bytes, &[cut], false).unwrap(), want, "cut {cut}");
    }
}

#[test]
fn errors_are_reported_the_same() {
    for doc in ["<a><b></a>", "<a>&undeclared;</a>", "<a x='1' x='2'/>", "<a></a><b/>", "<a>\u{1}</a>",
                "<p:a/>", "<a", "<!DOCTYPE a [<!ENTITY e '&e;'>]><a>&e;</a>"] {
        let want = whole(doc, true).expect_err(doc);
        for cut in 0..=doc.len() {
            let got = streamed(doc.as_bytes(), &[cut], true).expect_err(doc);
            assert_eq!(got, want, "{doc:?} cut {cut}");
        }
    }
}

#[test]
fn events_arrive_before_the_document_ends() {
    let mut p = StreamParser::new();
    let mut seen = Vec::new();
    p.feed(b"<a><b>hello", |e| seen.push(show(&e))).unwrap();
    assert!(seen.iter().any(|e| e.contains("StartElement") && e.contains("\"b\"")), "{seen:?}");
    assert!(seen.iter().any(|e| e.contains("hell")), "text so far is delivered: {seen:?}");
}

#[test]
fn errors_are_reported_before_the_document_ends() {
    let mut p = StreamParser::new();
    let r = p.feed(b"<a><b></c> and much more input to come", |_| {});
    assert!(r.is_err(), "mismatched end tag is certain before finish()");
}

#[test]
fn api_misuse() {
    let mut p = StreamParser::new();
    p.feed_str("<a/>", |_| {}).unwrap();
    assert!(p.feed(b" ", |_| {}).is_err(), "can't mix feed_str and feed");

    let mut p = StreamParser::new();
    p.feed(b"<a/>", |_| {}).unwrap();
    p.finish(|_| {}).unwrap();
    assert!(p.feed(b" ", |_| {}).is_err(), "no input after finish");

    let mut p = StreamParser::new();
    assert!(p.feed(b"<a></b>", |_| {}).is_err());
    assert!(p.feed(b"</a>", |_| {}).is_err(), "a failed parser stays failed");

    let mut p = StreamParser::new();
    p.feed(b"<a>", |_| {}).unwrap();
    assert!(p.finish(|_| {}).is_err(), "unclosed element at finish");
}

#[test]
fn error_positions_are_document_positions() {
    let doc = "<a>\n  <b>\n    </c>\n</a>";
    let want = whole(doc, false).unwrap_err();
    assert_eq!((want.position().line, want.position().column), (3, 5));
    let cuts: Vec<usize> = (1..doc.len()).collect();
    assert_eq!(streamed(doc.as_bytes(), &cuts, false).unwrap_err(), want);
}
