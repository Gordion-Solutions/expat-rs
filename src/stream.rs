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
use crate::encoding::{ByteMap, StreamDecoder, UnknownEncodingHandler};
use crate::entities::{AmplificationLimits, Dtd, ExpansionLimits};
use crate::error::{ErrorKind, Position, Result, XmlError};
use crate::event::{Event, Located, Span};
use crate::lexer::Lexer;
use crate::parser::State;
use crate::token::Token;

/// How close to the end of the buffer an error must be for it to possibly
/// come from truncation: the longest fixed lookahead in the lexer
/// (`<!NOTATION`, `standalone`) is 10 bytes.
const LOOKAHEAD: usize = 16;

/// Push parser for input that arrives in chunks, like libexpat's
/// `XML_Parse`: [`feed`](Self::feed) each chunk of bytes (or
/// [`feed_str`](Self::feed_str) each chunk of text), then
/// [`finish`](Self::finish).
///
/// Events go to the handler as soon as they are complete. They may borrow
/// the parser's buffer, so they are valid only for the duration of the
/// call; use `Cow::into_owned` to keep one. Results are the same however
/// the input is split, except that character data may arrive in more
/// [`Event::Text`] pieces.
///
/// - A construct cut off by the end of a chunk waits for the next one.
///   An error is reported as soon as it can't be the result of truncation.
/// - Text at the end of a chunk is delivered up to a point the next chunk
///   can't change, so long text doesn't accumulate.
/// - Reparse deferral: an incomplete construct isn't re-scanned until the
///   buffered input has at least doubled, so a huge construct fed in tiny
///   chunks costs linear, not quadratic, time (the class of libexpat
///   CVE-2023-52425).
/// - Once an error is returned, every later call returns it too.
///
/// `'l` is the lifetime of an external-entity loader, if one is set.
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
    /// Supplies byte maps for encodings expat-rs doesn't know.
    unknown_encoding: Option<UnknownEncodingHandler<'l>>,
    /// Size of `buf` when the last pass stopped at an incomplete construct
    /// (0 if it didn't). See the module docs on reparse deferral.
    stalled_at: usize,
    /// Whether reparse deferral is on (the default).
    deferral: bool,
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
    /// A parser with default options (as [`crate::Parser::new`]).
    pub fn new() -> Self {
        Self {
            state: State::new(),
            decoder: StreamDecoder::new(),
            buf: String::new(),
            base: Position::start(),
            at_start: true,
            input: None,
            failed: None,
            unknown_encoding: None,
            stalled_at: 0,
            deferral: true,
            finished: false,
        }
    }

    /// See [`crate::Parser::with_edition`].
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.state.set_edition(edition);
        self
    }

    /// See [`crate::Parser::with_libexpat_versions`].
    pub fn with_libexpat_versions(mut self) -> Self {
        self.state.libexpat_versions = true;
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

    /// Turn reparse deferral on (the default) or off. See
    /// [`set_reparse_deferral`](Self::set_reparse_deferral).
    pub fn with_reparse_deferral(mut self, enabled: bool) -> Self {
        self.deferral = enabled;
        self
    }

    /// Turn reparse deferral on or off, including between chunks. With it
    /// off, every chunk re-scans an incomplete construct from its start, so
    /// events fire as early as possible but a huge construct fed in tiny
    /// chunks costs quadratic time. Mirrors libexpat's
    /// `XML_SetReparseDeferralEnabled`.
    pub fn set_reparse_deferral(&mut self, enabled: bool) {
        self.deferral = enabled;
    }

    /// Whether reparse deferral is on.
    pub fn reparse_deferral(&self) -> bool {
        self.deferral
    }

    /// Decode the input as `encoding`, whatever it declares (libexpat's
    /// protocol encoding, as set by `XML_ParserCreate(encoding)` or
    /// `XML_SetEncoding`). See [`StreamDecoder::with_encoding`] for the
    /// names understood; others go to the unknown-encoding handler.
    pub fn with_encoding(mut self, encoding: &str) -> Self {
        self.decoder = StreamDecoder::with_encoding(encoding);
        self
    }

    /// Supply a [`ByteMap`] for declared (or forced) encodings expat-rs
    /// doesn't know, as libexpat's `XML_SetUnknownEncodingHandler` does;
    /// returning `None` makes the encoding an error.
    pub fn with_unknown_encoding(mut self, handler: impl FnMut(&str) -> Option<ByteMap> + 'l) -> Self {
        self.unknown_encoding = Some(Box::new(handler));
        self
    }

    /// How far parsing has got: the position just after the last input
    /// processed (after a successful [`finish`](Self::finish), the end of
    /// the document). Mirrors libexpat's position outside handlers.
    pub fn position(&self) -> Position {
        self.base
    }

    /// See [`crate::Parser::with_amplification_limits`].
    pub fn with_amplification_limits(mut self, limits: AmplificationLimits) -> Self {
        self.state.amplification = limits;
        self
    }

    /// Change the amplification limits; meant for before parsing starts.
    pub fn set_amplification_limits(&mut self, limits: AmplificationLimits) {
        self.state.amplification = limits;
    }

    /// See [`crate::Parser::with_expansion_limits`].
    pub fn with_expansion_limits(mut self, limits: ExpansionLimits) -> Self {
        self.state.expansion_limits = limits;
        self
    }

    /// Parse the next chunk of the document's bytes. The encoding is
    /// detected as for [`crate::decode`].
    pub fn feed(&mut self, bytes: &[u8], mut handler: impl FnMut(Event<'_>)) -> Result<()> {
        self.feed_located(bytes, |l| handler(l.event))
    }

    /// Like [`feed`](Self::feed), passing each event with its [`Span`] and
    /// source text. Positions are relative to the whole document.
    pub fn feed_located(&mut self, bytes: &[u8], mut handler: impl FnMut(Located<'_>)) -> Result<()> {
        self.guarded(Input::Bytes, |p| {
            p.decode(bytes, false, &mut handler)?;
            p.run(false, handler)
        })
    }

    /// Decode `bytes` onto the buffer. A byte that can't be decoded is
    /// reported, as libexpat does, after the input before it has been
    /// parsed (so its events are delivered and any earlier error wins),
    /// at its position in the document.
    fn decode(&mut self, bytes: &[u8], last: bool, handler: &mut impl FnMut(Located<'_>)) -> Result<()> {
        let unknown = &mut self.unknown_encoding;
        match self.decoder.decode_with(bytes, last, &mut self.buf, &mut |name| unknown.as_mut().and_then(|f| f(name))) {
            Err(XmlError::Encoding { kind: kind @ (ErrorKind::InvalidToken | ErrorKind::PartialChar), reason, .. }) => {
                let deferral = std::mem::replace(&mut self.deferral, false);
                let parsed = self.run(false, &mut *handler);
                self.deferral = deferral;
                parsed?;
                Err(XmlError::Encoding { pos: self.base.advance(&self.buf), kind, reason })
            }
            other => other,
        }
    }

    /// Parse the next chunk of the document as text. Don't mix with
    /// [`feed`](Self::feed) in one document.
    pub fn feed_str(&mut self, text: &str, mut handler: impl FnMut(Event<'_>)) -> Result<()> {
        self.feed_str_located(text, |l| handler(l.event))
    }

    /// Like [`feed_str`](Self::feed_str), with spans (see
    /// [`feed_located`](Self::feed_located)).
    pub fn feed_str_located(&mut self, text: &str, handler: impl FnMut(Located<'_>)) -> Result<()> {
        self.guarded(Input::Text, |p| {
            p.buf.push_str(text);
            p.run(false, handler)
        })
    }

    /// End of input: parse what remains and check the document is complete.
    pub fn finish(&mut self, mut handler: impl FnMut(Event<'_>)) -> Result<()> {
        self.finish_located(|l| handler(l.event))
    }

    /// Like [`finish`](Self::finish), with spans (see
    /// [`feed_located`](Self::feed_located)).
    pub fn finish_located(&mut self, handler: impl FnMut(Located<'_>)) -> Result<()> {
        let input = self.input.unwrap_or(Input::Text);
        self.guarded(input, |p| {
            let mut handler = handler;
            if input == Input::Bytes {
                p.decode(&[], true, &mut handler)?;
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
            return Err(XmlError::NotWellFormed { pos: self.base, kind: ErrorKind::Finished, reason: "input after finish()".into() });
        }
        if *self.input.get_or_insert(input) != input {
            return Err(XmlError::NotWellFormed {
                pos: self.base,
                kind: ErrorKind::InvalidArgument,
                reason: "feed() and feed_str() can't be mixed in one document".into(),
            });
        }
        f(self).inspect_err(|e| self.failed = Some(e.clone()))
    }

    /// Tokenise and handle as much of `buf` as is complete, then drop the
    /// consumed text.
    fn run(&mut self, last: bool, mut handler: impl FnMut(Located<'_>)) -> Result<()> {
        if !last && self.deferral && self.stalled_at > 0 && self.buf.len() < 2 * self.stalled_at {
            return Ok(()); // reparse deferral: wait for more input
        }
        let base = self.base;
        let mut lexer = if self.at_start { Lexer::new(&self.buf) } else { Lexer::continuing(&self.buf) }
            .with_edition(self.state.edition)
            .with_libexpat_versions(self.state.libexpat_versions);
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
                        // Before the root, text other than whitespace is an
                        // error whose position depends on the whole run.
                        let stray = self.state.before_root() && text.contains(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'));
                        if keep == text.len() || stray {
                            break;
                        }
                        let text: &str = &text[..text.len() - keep];
                        end = start.advance(text);
                        tok = Token::Text(text);
                    }
                    let dtd: Dtd = lexer.take_dtd();
                    self.state.span = Span { start, end };
                    self.state.direct_bytes = end.rebase(base).byte_offset as u64;
                    self.state.handle(tok, dtd, &mut out).map_err(|e| e.rebase(base))?;
                    for (event, span) in out.drain(..) {
                        let raw = self.buf.get(span.start.byte_offset..span.end.byte_offset).unwrap_or("");
                        let span = Span { start: span.start.rebase(base), end: span.end.rebase(base) };
                        handler(Located { event, span, raw });
                    }
                    consumed = Some(end);
                    if end.byte_offset < lexer.position().byte_offset {
                        break; // text was cut short; resume from `end` next time
                    }
                }
                // Running out of input (wherever the construct started), or
                // an error close enough to the end to be from truncation:
                // wait for more.
                Err(XmlError::UnexpectedEof { .. }) if !last => break,
                // A character that isn't allowed can't be the result of
                // truncation: report it now, as libexpat does.
                Err(e @ XmlError::InvalidChar { .. }) => return Err(e.rebase(base)),
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
