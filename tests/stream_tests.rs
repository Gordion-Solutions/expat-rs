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
    // Names and literals of characters of several bytes in the DOCTYPE.
    "<!DOCTYPE dé [\r\n<!ELEMENT éa (éb|ü|c)>\r\n<!ENTITY éx \"ü\u{333}\">\r\n<!ATTLIST éa é\u{333} CDATA 'v\u{333}'>\r\n]>\r\n<dé>&éx;</dé>",
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
                "<p:a/>", "<a", "<!DOCTYPE a [<!ENTITY e '&e;'>]><a>&e;</a>",
                // Found by fuzzing: two errors in one text run outside the
                // root; both modes must report the earlier one.
                "<!DOCTYPE d []>\nstray text then a NUL \u{0} later",
                "\n\n>\r\n!EL]]>dc>",
                // Found by fuzzing: a `\r\n` split between chunks before
                // stray text after the root, and inside the DOCTYPE.
                "<doc></doc>\r\nIllegal data\r\n",
                "<!DOCTYPE d [\r\n<!ENTITY e \"abc",
                "<doc></doc>\r\n\u{0}"] {
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
    // At the end tag's name, as libexpat reports it.
    assert_eq!((want.position().line, want.position().column), (3, 7));
    let cuts: Vec<usize> = (1..doc.len()).collect();
    assert_eq!(streamed(doc.as_bytes(), &cuts, false).unwrap_err(), want);
}

#[test]
fn earliest_error_wins_when_a_text_run_has_two() {
    // Text outside the root is reported at its first non-whitespace
    // character, before a later invalid character or ']]>' in the same run.
    for doc in ["<a/>  junk \u{1}", "<a/>  junk ]]>"] {
        let e = whole(doc, false).unwrap_err();
        assert_eq!(e.position().byte_offset, 6, "{doc:?}: {e}");
    }
    // Inside the root, text before an invalid character is delivered first.
    let mut p = Parser::new("<a>ok\u{1}</a>");
    assert!(matches!(p.next_event().unwrap(), Some(Event::StartElement { .. })));
    assert!(matches!(p.next_event().unwrap(), Some(Event::Text(t)) if t == "ok"));
    assert!(matches!(p.next_event(), Err(XmlError::InvalidChar { .. })));
}

#[test]
fn reparse_deferral_on_and_off_match_libexpat_timing() {
    // CPython's test_reparse_deferral: feed "<doc" then "/>".
    let starts = |deferral: bool| {
        let mut p = StreamParser::new().with_reparse_deferral(deferral);
        let mut seen = Vec::new();
        for chunk in [&b"<doc"[..], b"/>"] {
            p.feed(chunk, |e| if let Event::StartElement { .. } = e { seen.push("start") }).unwrap();
        }
        let before_finish = seen.len();
        p.finish(|_| {}).unwrap();
        before_finish
    };
    assert_eq!(starts(true), 0, "deferred: nothing before the final call (libexpat with deferral)");
    assert_eq!(starts(false), 1, "not deferred: fires on the second chunk (libexpat without)");

    // Switchable between chunks.
    let mut p = StreamParser::new();
    assert!(p.reparse_deferral());
    let mut n = 0;
    p.feed(b"<doc", |_| n += 1).unwrap();
    p.set_reparse_deferral(false);
    p.feed(b"/>", |_| n += 1).unwrap();
    assert_eq!(n, 2, "start and end delivered once deferral is off");
}

#[test]
fn every_line_end_counts_as_one_line() {
    // \n, \r\n and a lone \r each end a line (§2.11), however the input is
    // split; the error is on line 4.
    let doc = "<a>\n\r\n\r<b></c></a>";
    let want = whole(doc, false).unwrap_err();
    assert_eq!((want.position().line, want.position().column), (4, 6));
    for cut in 0..=doc.len() {
        assert_eq!(streamed(doc.as_bytes(), &[cut], false).unwrap_err(), want, "cut {cut}");
    }
}

#[test]
fn whitespace_outside_the_root_on_request() {
    let doc = "<?xml version='1.0'?>\r\n<!--c-->\n<r>\n</r>\t\n";
    let collect = |p: StreamParser<'_>| {
        let mut p = p;
        let mut ws = Vec::new();
        let mut record = |e: Event<'_>| if let Event::Whitespace(w) = e { ws.push(w.into_owned()) };
        p.feed_str(doc, &mut record).unwrap();
        p.finish(&mut record).unwrap();
        ws
    };
    assert!(collect(StreamParser::new()).is_empty(), "off by default");
    // Line ends normalised; the whitespace inside <r> is text, not this.
    assert_eq!(collect(StreamParser::new().with_outside_whitespace()), ["\n", "\n", "\t\n"]);
}

/// Feed `src` in pieces of `size` to a parser reporting CDATA in pieces,
/// collecting the events inside the root element.
fn cdata_pieces(src: &str, size: usize) -> Result<Vec<String>, XmlError> {
    let mut p = StreamParser::new().with_cdata_pieces();
    let mut out = Vec::new();
    for piece in src.as_bytes().chunks(size) {
        p.feed(piece, |e| out.push(show(&e)))?;
    }
    p.finish(|e| out.push(show(&e)))?;
    Ok(out)
}

#[test]
fn cdata_in_pieces_follows_the_input() {
    let doc = "<r><![CDATA[a]]b\r\nc]]></r>";
    // Whole: one piece of text between the start and the end.
    assert_eq!(
        cdata_pieces(doc, 4096).unwrap()[1..4],
        ["StartCData", "CData(\"a]]b\\nc\")", "EndCData"].map(String::from)
    );
    // In pieces, the text comes as it arrives; joined, it is the same, and
    // no piece ends in a `]` or `\r` the next could have changed.
    for size in 1..doc.len() {
        let events = cdata_pieces(doc, size).unwrap();
        assert_eq!(events.first().unwrap(), "StartElement { name: \"r\", namespace: None, attributes: [] }");
        assert_eq!(events[1], "StartCData", "pieces of {size}");
        assert_eq!(events[events.len() - 2], "EndCData", "pieces of {size}");
        let text: String = events[2..events.len() - 2]
            .iter()
            .map(|e| e.strip_prefix("CData(\"").and_then(|t| t.strip_suffix("\")")).expect("only text inside").to_string())
            .collect();
        assert_eq!(text, "a]]b\\nc", "pieces of {size}");
    }
    // An empty section has no text; one in an entity's replacement text
    // comes whole.
    let events = cdata_pieces("<!DOCTYPE r [<!ENTITY e '<![CDATA[in]]>'>]><r><![CDATA[]]>&e;</r>", 7).unwrap();
    let inside: Vec<&str> = events.iter().map(String::as_str).filter(|e| e.contains("CData")).collect();
    assert_eq!(inside, ["StartCData", "EndCData", "StartCData", "CData(\"in\")", "EndCData"]);
    // Without the option, a section is one event, as before.
    let mut events = Vec::new();
    let mut p = StreamParser::new();
    for piece in doc.as_bytes().chunks(3) {
        p.feed(piece, |e| events.push(show(&e))).unwrap();
    }
    p.finish(|e| events.push(show(&e))).unwrap();
    assert!(events.contains(&"CData(\"a]]b\\nc\")".to_string()));
}

#[test]
fn unclosed_cdata_in_pieces_is_still_an_error() {
    for doc in ["<r><![CDATA[never", "<r><![CDATA[almost]]", "<r><![CDATA[x\r"] {
        for size in [1, 3, 4096] {
            let err = cdata_pieces(doc, size).unwrap_err();
            assert_eq!(err.kind(), expat_rs::ErrorKind::UnclosedCdataSection, "{doc:?} in pieces of {size}");
        }
    }
}

