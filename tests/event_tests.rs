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
