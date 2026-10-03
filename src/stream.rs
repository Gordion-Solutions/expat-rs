//! Incremental (push) parsing: input arrives in chunks.
//!
//! [`StreamParser`] is the counterpart of libexpat's `XML_Parse`: call
//! [`feed`](StreamParser::feed) with each chunk of bytes as it arrives and
//! [`finish`](StreamParser::finish) at the end. Events go to a handler as
//! soon as they are complete; they may borrow the parser's buffer, so they
//! are valid only for the duration of the call.
//!
//! A construct cut off by the end of a chunk waits for the next one.
//! Character data at the end of a chunk is delivered up to a point where
//! it can't be affected by what follows (a `\r` or `]` is held back), so
//! `\r\n` and `]]>` split across chunks are handled correctly and long
//! text doesn't accumulate. An error is reported as soon as it can't be
//! the result of truncation.
//!
//! Reparse deferral: when a chunk ends inside a construct, that construct
//! is not re-scanned until the buffered input has at least doubled.
//! Otherwise a single huge construct (a long comment, say) fed in small
//! chunks would be re-scanned from its start every time, which is
//! quadratic: the class of libexpat's CVE-2023-52425, fixed there the same
//! way. Events are still delivered in order, at the latest by `finish`.

use std::collections::VecDeque;

use crate::chars::Edition;
use crate::encoding::StreamDecoder;
use crate::entities::{Dtd, ExpansionLimits};
use crate::error::{Position, Result, XmlError};
use crate::event::Event;
use crate::lexer::Lexer;
use crate::parser::State;
use crate::token::Token;

/// How close to the end of the buffer an error must be for it to possibly
/// come from truncation: the longest fixed lookahead in the lexer
/// (`<!NOTATION`, `standalone`) is 10 bytes.
const LOOKAHEAD: usize = 16;

pub struct StreamParser<'l> {
    state: State<'l>,
    decoder: StreamDecoder,
    /// Decoded text not yet tokenised; always starts at a token boundary.
    buf: String,
    /// Position of `buf[0]` in the document.
    base: Position,
    /// Nothing consumed yet, so the XML declaration may still come.
    at_start: bool,
    /// Whether input came as bytes (`feed`) or text (`feed_str`).
    input: Option<Input>,
    /// A parser that has failed stays failed.
    failed: Option<XmlError>,
    /// Size of `buf` when the last pass stopped at an incomplete construct
    /// (0 if it didn't). See the module docs on reparse deferral.
    stalled_at: usize,
    finished: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Input {
    Bytes,
    Text,
}

impl<'l> Default for StreamParser<'l> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'l> StreamParser<'l> {
    pub fn new() -> Self {
        Self {
            state: State::new(),
            decoder: StreamDecoder::new(),
            buf: String::new(),
            base: Position::start(),
            at_start: true,
            input: None,
            failed: None,
            stalled_at: 0,
            finished: false,
        }
    }

    /// See [`crate::Parser::with_edition`].
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.state.set_edition(edition);
        self
    }

    /// See [`crate::Parser::with_namespaces`].
    pub fn with_namespaces(mut self) -> Self {
        self.state.enable_namespaces();
        self
    }

    /// See [`crate::Parser::with_external_loader`]. Off by default.
    pub fn with_external_loader(
        mut self,
        load: impl FnMut(&str, Option<&str>) -> std::result::Result<Option<String>, String> + 'l,
    ) -> Self {
        self.state.loader = Some(Box::new(load));
        self
    }

    /// See [`crate::Parser::with_expansion_limits`].
    pub fn with_expansion_limits(mut self, limits: ExpansionLimits) -> Self {
        self.state.expansion_limits = limits;
        self
    }

    /// Parse the next chunk of the document's bytes. The encoding is
    /// detected as for [`crate::decode`].
    pub fn feed(&mut self, bytes: &[u8], handler: impl FnMut(Event<'_>)) -> Result<()> {
        self.guarded(Input::Bytes, |p| {
            p.decoder.decode(bytes, false, &mut p.buf)?;
            p.run(false, handler)
        })
    }

    /// Parse the next chunk of the document as text. Don't mix with
    /// [`feed`](Self::feed) in one document.
    pub fn feed_str(&mut self, text: &str, handler: impl FnMut(Event<'_>)) -> Result<()> {
        self.guarded(Input::Text, |p| {
            p.buf.push_str(text);
            p.run(false, handler)
        })
    }

    /// End of input: parse what remains and check the document is complete.
    pub fn finish(&mut self, handler: impl FnMut(Event<'_>)) -> Result<()> {
        let input = self.input.unwrap_or(Input::Text);
        self.guarded(input, |p| {
            if input == Input::Bytes {
                p.decoder.decode(&[], true, &mut p.buf)?;
            }
            p.run(true, handler)?;
            p.finished = true;
            Ok(())
        })
    }

    fn guarded(&mut self, input: Input, f: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        if self.finished {
            return Err(XmlError::NotWellFormed { pos: self.base, reason: "input after finish()".into() });
        }
        if *self.input.get_or_insert(input) != input {
            return Err(XmlError::NotWellFormed {
                pos: self.base,
                reason: "feed() and feed_str() can't be mixed in one document".into(),
            });
        }
        f(self).inspect_err(|e| self.failed = Some(e.clone()))
    }

    /// Tokenise and handle as much of `buf` as is complete, then drop the
    /// consumed text.
    fn run(&mut self, last: bool, mut handler: impl FnMut(Event<'_>)) -> Result<()> {
        if !last && self.stalled_at > 0 && self.buf.len() < 2 * self.stalled_at {
            return Ok(()); // reparse deferral: wait for more input
        }
        let base = self.base;
        let mut lexer = if self.at_start { Lexer::new(&self.buf) } else { Lexer::continuing(&self.buf) }
            .with_edition(self.state.edition);
        let mut out = VecDeque::new();
        let mut consumed: Option<Position> = None;
        loop {
            let start = lexer.position();
            self.state.last_pos = start;
            match lexer.next_token() {
                Ok(None) => break,
                Ok(Some(tok)) => {
                    let mut end = lexer.position();
                    let mut tok = tok;
                    if let (Token::Text(text), false, true) = (&tok, last, end.byte_offset == self.buf.len()) {
                        // Text may continue in the next chunk: deliver it up
                        // to a point the next chunk can't change.
                        let keep = text.len() - text.trim_end_matches(['\r', ']']).len();
                        if keep == text.len() {
                            break;
                        }
                        let text: &str = &text[..text.len() - keep];
                        end = start.advance(text);
                        tok = Token::Text(text);
                    }
                    let dtd: Dtd = lexer.take_dtd();
                    self.state.handle(tok, dtd, &mut out).map_err(|e| e.rebase(base))?;
                    out.drain(..).for_each(&mut handler);
                    consumed = Some(end);
                    if end.byte_offset < lexer.position().byte_offset {
                        break; // text was cut short; resume from `end` next time
                    }
                }
                Err(e) if !last && e.position().byte_offset + LOOKAHEAD >= self.buf.len() => break,
                Err(e) => return Err(e.rebase(base)),
            }
        }
        if last {
            self.state.finish().map_err(|e| e.rebase(base))?;
        }
        drop(lexer);
        if let Some(end) = consumed {
            self.base = end.rebase(base);
            self.buf.drain(..end.byte_offset);
            self.at_start = false;
        }
        // Whatever is left is an incomplete construct (or held-back text).
        self.stalled_at = if last { 0 } else { self.buf.len() };
        Ok(())
    }
}
