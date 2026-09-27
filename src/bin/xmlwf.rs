//! xmlwf — well-formedness checker CLI.
//!
//! Modelled after libexpat's `xmlwf`. Reads an XML file from a path argument
//! and exits 0 if it is well-formed, non-zero otherwise. Errors go to stderr.
//!
//! Usage:  xmlwf [--edition 4|5] <path>
//!
//! `--edition` selects the XML 1.0 edition whose Name rules apply
//! (default 5).

use std::process::ExitCode;

use expat_rs::Edition;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let usage = || {
        eprintln!("usage: {} [--edition 4|5] <xml-file>", args[0]);
        ExitCode::from(2)
    };
    let (edition, path) = match &args[1..] {
        [path] => (Edition::Fifth, path),
        [flag, ed, path] if flag == "--edition" => match ed.as_str() {
            "4" => (Edition::Fourth, path),
            "5" => (Edition::Fifth, path),
            _   => return usage(),
        },
        _ => return usage(),
    };
    let src = match std::fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("{}: {}", path, e);
            return ExitCode::from(2);
        }
    };

    let mut parser = expat_rs::Parser::new(&src).with_edition(edition);
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
