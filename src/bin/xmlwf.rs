//! xmlwf — well-formedness checker CLI.
//!
//! Modelled after libexpat's `xmlwf`. Reads an XML file from a path argument
//! and exits 0 if it is well-formed, non-zero otherwise. Errors go to stderr.
//!
//! Usage:  xmlwf [--edition 4|5] [--external] [--canonical] <path>
//!
//! `--edition` selects the XML 1.0 edition whose Name rules apply
//! (default 5). `--external` reads external parsed entities, resolving
//! system identifiers relative to the document's directory; without it,
//! nothing outside the named file is read. `--canonical` writes the
//! document to stdout in James Clark's canonical XML form, the format of
//! the W3C conformance suite's expected-output files.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use expat_rs::{Edition, Event};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let usage = || {
        eprintln!("usage: {} [--edition 4|5] [--external] [--canonical] <xml-file>", args[0]);
        ExitCode::from(2)
    };
    let mut edition = Edition::Fifth;
    let mut external = false;
    let mut canonical = false;
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
    let src = match expat_rs::decode(&bytes) {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return ExitCode::from(1);
        }
    };

    let mut parser = expat_rs::Parser::new(&src).with_edition(edition);
    if external {
        let base = Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf();
        parser = parser.with_external_loader(move |system_id, _public_id| {
            let bytes = std::fs::read(base.join(system_id)).map_err(|e| e.to_string())?;
            let text = expat_rs::decode(&bytes).map_err(|e| e.to_string())?;
            Ok(Some(text.into_owned()))
        });
    }
    let mut out = String::new();
    // Second canonical form: a DOCTYPE listing the declared notations,
    // written at the end of the DOCTYPE.
    let mut doctype: Option<(String, Vec<Notation>)> = None;
    loop {
        match parser.next_event() {
            Ok(Some(Event::Doctype { name, .. })) => doctype = Some((name.to_string(), Vec::new())),
            Ok(Some(Event::NotationDecl { name, public_id, system_id })) => {
                if let Some((_, notations)) = &mut doctype {
                    notations.push((name.into_owned(), public_id.map(|p| p.into_owned()), system_id.map(|s| s.into_owned())));
                }
            }
            Ok(Some(Event::EndDoctype)) => if let Some((root, notations)) = doctype.take() {
                write_notations(&root, notations, &mut out);
            },
            Ok(Some(e)) => if canonical { write_canonical(&e, &mut out) },
            Ok(None)    => {
                if canonical {
                    let _ = std::io::stdout().write_all(out.as_bytes());
                }
                return ExitCode::from(0);
            }
            Err(e)      => {
                eprintln!("{}: {}", path, e);
                return ExitCode::from(1);
            }
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
        Event::StartElement { name, attributes } => {
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
