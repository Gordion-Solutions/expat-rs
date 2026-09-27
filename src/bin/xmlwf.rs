//! xmlwf — well-formedness checker CLI.
//!
//! Modelled after libexpat's `xmlwf`. Reads an XML file from a path argument
//! and exits 0 if it is well-formed, non-zero otherwise. Errors go to stderr.
//!
//! Usage:  xmlwf [--edition 4|5] [--external] <path>
//!
//! `--edition` selects the XML 1.0 edition whose Name rules apply
//! (default 5). `--external` reads external parsed entities, resolving
//! system identifiers relative to the document's directory; without it,
//! nothing outside the named file is read.

use std::path::Path;
use std::process::ExitCode;

use expat_rs::Edition;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let usage = || {
        eprintln!("usage: {} [--edition 4|5] [--external] <xml-file>", args[0]);
        ExitCode::from(2)
    };
    let mut edition = Edition::Fifth;
    let mut external = false;
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
            _ if path.is_none() => path = Some(arg),
            _ => return usage(),
        }
    }
    let Some(path) = path else { return usage() };
    let src = match std::fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return ExitCode::from(2);
        }
    };

    let mut parser = expat_rs::Parser::new(&src).with_edition(edition);
    if external {
        let base = Path::new(path).parent().unwrap_or(Path::new(".")).to_path_buf();
        // Entities that can't be read as UTF-8 are left unread.
        parser = parser.with_external_loader(move |system_id, _public_id| {
            std::fs::read_to_string(base.join(system_id)).ok()
        });
    }
    loop {
        match parser.next_event() {
            Ok(Some(_)) => continue,
            Ok(None)    => return ExitCode::from(0),
            Err(e)      => {
                eprintln!("{}: {}", path, e);
                return ExitCode::from(1);
            }
        }
    }
}
