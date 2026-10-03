//! Any input, any options: the parser must return Ok or Err, never panic.
#![no_main]

use expat_rs::{decode, Edition, Parser};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&flags, bytes)) = data.split_first() else { return };
    let Ok(text) = decode(bytes) else { return };
    let mut p = Parser::new(&text);
    if flags & 1 != 0 {
        p = p.with_edition(Edition::Fourth);
    }
    if flags & 2 != 0 {
        p = p.with_namespaces();
    }
    if flags & 4 != 0 {
        // A loader that serves the input itself back for any entity.
        let ext = text.to_string();
        p = p.with_external_loader(move |_, _| Ok(Some(ext.clone())));
    }
    while let Ok(Some(_)) = p.next_event() {}
});
