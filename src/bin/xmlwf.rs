//! xmlwf — well-formedness checker CLI.
//!
//! Modelled after libexpat's `xmlwf`. Reads an XML file from a path argument
//! and exits 0 if it is well-formed, non-zero otherwise. Errors go to stderr.
//!
//! Usage: `xmlwf [--edition 4|5] [--external] [--namespaces] [--canonical] [--chunk N] <path>`
//!
//! `--edition` selects the XML 1.0 edition whose Name rules apply
//! (default 5). `--external` reads external parsed entities, resolving
//! system identifiers relative to the document's directory; without it,
//! nothing outside the named file is read. `--namespaces` enables
//! namespace processing (Namespaces in XML 1.0). `--canonical` writes the
//! document to stdout in James Clark's canonical XML form, the format of
//! the W3C conformance suite's expected-output files. `--chunk N` parses
//! incrementally with `StreamParser`, feeding N bytes at a time.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use expat_rs::{Edition, Event, StreamParser};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    const HELP: &str = "\
Check that an XML file is well-formed; exit 0 if it is, 1 if not.

options:
  --edition 4|5   XML 1.0 edition for name rules (default 5)
  --external      read external parsed entities next to the file
  --namespaces    enable namespace processing
  --canonical     write the document in canonical XML form to stdout
  --chunk N       parse incrementally, N bytes at a time
  -h, --help      show this help";
    let usage = || {
        eprintln!("usage: {} [--edition 4|5] [--external] [--namespaces] [--canonical] [--chunk N] <xml-file>", args[0]);
        ExitCode::from(2)
    };
    if args.iter().skip(1).any(|a| a == "-h" || a == "--help") {
        println!("usage: {} [options] <xml-file>\n\n{HELP}", args[0]);
        return ExitCode::from(0);
    }
    let mut edition = Edition::Fifth;
    let mut external = false;
    let mut canonical = false;
    let mut namespaces = false;
    let mut chunk = None;
    let mut path = None;
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--edition" => match rest.next().map(String::as_str) {
                Some("4") => edition = Edition::Fourth,
                Some("5") => edition = Edition::Fifth,
                _ => return usage(),
            },
            "--external" => external = true,
            "--canonical" => canonical = true,
            "--namespaces" => namespaces = true,
            "--chunk" => match rest.next().and_then(|n| n.parse::<usize>().ok()) {
                Some(n) if n > 0 => chunk = Some(n),
                _ => return usage(),
            },
            _ if path.is_none() => path = Some(arg),
            _ => return usage(),
        }
    }
    let Some(path) = path else { return usage() };
    let bytes = match std::fs::read(path) {
        Ok(b)  => b,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return ExitCode::from(2);
        }
    };
    let base = Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf();
    let loader = move |system_id: &str, _public_id: Option<&str>| {
        let bytes = std::fs::read(base.join(system_id)).map_err(|e| e.to_string())?;
        let text = expat_rs::decode(&bytes).map_err(|e| e.to_string())?;
        Ok(Some(text.into_owned()))
    };
    let mut canon = Canonical::default();

    let result = match chunk {
        // Incremental parsing, `n` bytes at a time.
        Some(n) => {
            let mut parser = StreamParser::new().with_edition(edition);
            if namespaces {
                parser = parser.with_namespaces();
            }
            if external {
                parser = parser.with_external_loader(loader);
            }
            bytes.chunks(n.max(1))
                .try_for_each(|piece| parser.feed(piece, |e| if canonical { canon.event(e) }))
                .and_then(|()| parser.finish(|e| if canonical { canon.event(e) }))
        }
        None => (|| {
            let src = expat_rs::decode(&bytes)?;
            let mut parser = expat_rs::Parser::new(&src).with_edition(edition);
            if namespaces {
                parser = parser.with_namespaces();
            }
            if external {
                parser = parser.with_external_loader(loader);
            }
            while let Some(e) = parser.next_event()? {
                if canonical {
                    canon.event(e);
                }
            }
            Ok(())
        })(),
    };
    match result {
        Ok(()) => {
            if canonical {
                let _ = std::io::stdout().write_all(canon.out.as_bytes());
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("{}: {}", path, e);
            ExitCode::from(1)
        }
    }
}

/// Builds the canonical XML form of a document from its events.
#[derive(Default)]
struct Canonical {
    out: String,
    /// Second canonical form: a DOCTYPE listing the declared notations,
    /// written at the end of the DOCTYPE.
    doctype: Option<(String, Vec<Notation>)>,
}

impl Canonical {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Doctype { name, .. } => self.doctype = Some((name.to_string(), Vec::new())),
            Event::NotationDecl { name, public_id, system_id } => {
                if let Some((_, notations)) = &mut self.doctype {
                    notations.push((name.into_owned(), public_id.map(|p| p.into_owned()), system_id.map(|s| s.into_owned())));
                }
            }
            Event::EndDoctype => if let Some((root, notations)) = self.doctype.take() {
                write_notations(&root, notations, &mut self.out);
            },
            e => write_canonical(&e, &mut self.out),
        }
    }
}

type Notation = (String, Option<String>, Option<String>);

fn write_notations(root: &str, mut notations: Vec<Notation>, out: &mut String) {
    if notations.is_empty() {
        return;
    }
    notations.sort();
    out.push_str(&format!("<!DOCTYPE {root} [\n"));
    for (name, public_id, system_id) in &notations {
        let id = match (public_id, system_id) {
            (Some(p), Some(s)) => format!("PUBLIC '{p}' '{s}'"),
            (Some(p), None)    => format!("PUBLIC '{p}'"),
            (None, Some(s))    => format!("SYSTEM '{s}'"),
            (None, None)       => String::new(),
        };
        out.push_str(&format!("<!NOTATION {name} {id}>\n"));
    }
    out.push_str("]>\n");
}

/// Append one event in canonical XML form: attributes sorted by name,
/// empty elements as start + end tag, comments and declarations dropped,
/// and `& < > "` plus TAB, LF, CR escaped in text and attribute values.
fn write_canonical(event: &Event<'_>, out: &mut String) {
    match event {
        Event::StartElement { name, attributes, .. } => {
            out.push('<');
            out.push_str(name);
            let mut attrs: Vec<_> = attributes.iter().collect();
            attrs.sort_by(|a, b| a.name.cmp(&b.name));
            for a in attrs {
                out.push(' ');
                out.push_str(&a.name);
                out.push_str("=\"");
                escape(&a.value, out);
                out.push('"');
            }
            out.push('>');
        }
        Event::EndElement(name) => {
            out.push_str("</");
            out.push_str(name);
            out.push('>');
        }
        Event::Text(t) | Event::CData(t) => escape(t, out),
        Event::ProcessingInstruction { target, body } => {
            out.push_str("<?");
            out.push_str(target);
            out.push(' ');
            out.push_str(body);
            out.push_str("?>");
        }
        _ => {}
    }
}

fn escape(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '&'  => out.push_str("&amp;"),
            '<'  => out.push_str("&lt;"),
            '>'  => out.push_str("&gt;"),
            '"'  => out.push_str("&quot;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c    => out.push(c),
        }
    }
}
