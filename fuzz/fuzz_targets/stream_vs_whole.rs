//! Differential: StreamParser fed the input in fuzzer-chosen chunks must
//! give exactly what Parser gives on the whole input (with consecutive
//! text pieces joined, since streamed text may arrive in more pieces).
#![no_main]

use expat_rs::{decode, Event, Parser, StreamParser, XmlError};
use libfuzzer_sys::fuzz_target;

fn record(out: &mut Vec<String>, e: Event<'_>) {
    if let Event::Text(t) = &e {
        if let Some(last) = out.last_mut() {
            if let Some(prev) = last.strip_prefix("T:") {
                *last = format!("T:{prev}{t}");
                return;
            }
        }
        out.push(format!("T:{t}"));
    } else {
        out.push(format!("{e:?}"));
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let (flags, step, bytes) = (data[0], (data[1] as usize % 16) + 1, &data[2..]);
    // Whole-document reference. Input that doesn't decode as a whole is
    // out of scope (streaming may report a different error first).
    let Ok(text) = decode(bytes) else { return };
    let namespaces = flags & 1 != 0;

    let mut whole = Vec::new();
    let mut p = Parser::new(&text);
    if namespaces { p = p.with_namespaces(); }
    let whole_result: Result<(), XmlError> = (|| {
        while let Some(e) = p.next_event()? { record(&mut whole, e); }
        Ok(())
    })();

    let mut streamed = Vec::new();
    let mut s = StreamParser::new();
    if namespaces { s = s.with_namespaces(); }
    let stream_result: Result<(), XmlError> = (|| {
        for chunk in bytes.chunks(step) {
            s.feed(chunk, |e| record(&mut streamed, e))?;
        }
        s.finish(|e| record(&mut streamed, e))
    })();

    match (&whole_result, &stream_result) {
        (Ok(()), Ok(())) => assert_eq!(whole, streamed, "events differ"),
        (Err(a), Err(b)) => {
            assert_eq!(a, b, "errors differ");
            // Events before the error: streaming must not have delivered
            // anything the whole-document parser didn't.
            assert!(streamed.len() <= whole.len() + 1, "extra events before error");
        }
        _ => panic!("results differ: whole {whole_result:?}, streamed {stream_result:?}"),
    }
});
