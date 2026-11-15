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
//! Reparse deferral: when a chunk ends inside a construct and re-scanning
//! it made no progress, it is not re-scanned again until the buffered
//! input has doubled, or grown by a quarter or past a power of two (and by
//! at least 1 KiB).
//! Otherwise a single huge construct (a long comment, say) fed in small
//! chunks would be re-scanned from its start every time, which is
//! quadratic: the class of libexpat's CVE-2023-52425, fixed there in a similar
//! way. Events are still delivered in order, at the latest by `finish`.

use std::cell::Cell;
use std::collections::VecDeque;

use crate::chars::Edition;
use crate::encoding::{EncodingMap, StreamDecoder, UnknownEncodingHandler};
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
/// - Reparse deferral: once an incomplete construct has been re-scanned
///   without progress, it isn't re-scanned again until the buffered input
///   has doubled, or grown by a quarter or past a power of two (and by at
///   least 1 KiB), so a huge
///   construct fed in tiny chunks costs linear, not quadratic, time (the
///   class of libexpat CVE-2023-52425).
///   The first chunk after a pass that made progress is always tried, as
///   libexpat does: a construct completed by a small chunk is reported at
///   once.
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
    /// Bytes of input (before decoding) that `buf[0]` comes after.
    input_base: usize,
    /// How much of `buf` has been measured in input bytes: (offset in
    /// `buf`, input bytes up to it), so events in order are measured once.
    measured: Cell<(usize, usize)>,
    /// The same for the whole of `buf`, kept apart so that measuring the
    /// buffer at each feed only measures what was added.
    measured_end: Cell<(usize, usize)>,
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
    /// The last pass made progress before stalling, so the next chunk is
    /// tried at once whatever its size (as libexpat does).
    stall_fresh: bool,
    /// When the last pass stalled inside the DOCTYPE: where libexpat's last
    /// complete token there ends (offset in `buf`), and the input bytes
    /// before it. Deferral is then measured from there, as libexpat's is
    /// from its partial token (see `dtd_token_boundary`).
    dtd_stall: Option<(usize, usize)>,
    /// Whether reparse deferral is on (the default).
    deferral: bool,
    /// `buf` starts inside a CDATA section reported in pieces.
    in_cdata: bool,
    /// Inside the DOCTYPE's internal subset.
    in_subset: bool,
    /// The text consumed before `buf` ended with `\r`.
    after_cr: bool,
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
            input_base: 0,
            measured: Cell::new((0, 0)),
            measured_end: Cell::new((0, 0)),
            at_start: true,
            input: None,
            failed: None,
            unknown_encoding: None,
            stalled_at: 0,
            stall_fresh: false,
            dtd_stall: None,
            deferral: true,
            in_cdata: false,
            in_subset: false,
            after_cr: false,
            finished: false,
        }
    }

    /// See [`crate::Parser::with_edition`].
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.state.set_edition(edition);
        self
    }

    /// Report whitespace outside the root element (before it and after it)
    /// as [`Event::Whitespace`] rather than dropping it, so that, with the
    /// other events' source text, the whole document can be reproduced.
    pub fn with_outside_whitespace(mut self) -> Self {
        self.state.outside_whitespace = true;
        self
    }

    /// Report each CDATA section as [`Event::StartCData`], its text in
    /// [`Event::CData`] pieces, and [`Event::EndCData`], delivering the text
    /// of a section cut off by the end of a chunk as far as the next chunk
    /// can't change, as libexpat does, instead of as one [`Event::CData`]
    /// once the section is complete.
    pub fn with_cdata_pieces(mut self) -> Self {
        self.state.cdata_pieces = true;
        self
    }

    /// Report references in content to external parsed entities as
    /// [`Event::ExternalEntityRef`], for the caller to read and parse with
    /// the event's [`EntityContext::parser`](crate::EntityContext::parser),
    /// instead of skipping them. An external loader, if set, still reads
    /// them itself. See [`crate::EntityContext`].
    pub fn with_external_entity_refs(mut self) -> Self {
        self.state.external_refs = true;
        self
    }

    /// A parser for the content of the external entity `context` is for.
    /// See [`crate::EntityContext::parser`].
    pub(crate) fn for_entity(context: &crate::EntityContext) -> StreamParser<'static> {
        let mut p = StreamParser::new();
        p.state.enter_entity(context);
        p
    }

    /// See [`crate::Parser::with_libexpat_leniency`].
    pub fn with_libexpat_leniency(mut self) -> Self {
        self.state.libexpat_leniency = true;
        self.decoder.lenient_bom = true;
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
        let lenient_bom = self.decoder.lenient_bom;
        self.decoder = StreamDecoder::with_encoding(encoding);
        self.decoder.lenient_bom = lenient_bom;
        self
    }

    /// Supply an [`EncodingMap`] for declared (or forced) encodings expat-rs
    /// doesn't know, as libexpat's `XML_SetUnknownEncodingHandler` does;
    /// returning `None` makes the encoding an error.
    pub fn with_unknown_encoding(mut self, handler: impl FnMut(&str) -> Option<EncodingMap> + 'l) -> Self {
        self.unknown_encoding = Some(Box::new(handler));
        self
    }

    /// Input bytes (before decoding) up to offset `at` in `buf`, measuring
    /// on from the last offset measured when `at` is past it. Past the end
    /// of `buf` (an error in input not yet decoded, such as an encoding
    /// declaration), each further byte counts as one.
    fn input_at(&self, at: usize) -> usize {
        if at > self.buf.len() {
            return self.input_at(self.buf.len()) + (at - self.buf.len());
        }
        let at = floor_char_boundary(&self.buf, at);
        let (done, bytes) = self.measured.get();
        let bytes = if at >= done {
            bytes + self.decoder.input_len(&self.buf[done..at])
        } else {
            self.decoder.input_len(&self.buf[..at])
        };
        self.measured.set((at, bytes));
        self.input_base + bytes
    }

    /// Where `pos` (a position in the decoded document, at or after
    /// [`position`](Self::position)) is in the input as given to
    /// [`feed`](Self::feed), in bytes before decoding. For UTF-8 input, its
    /// byte offset; see [`Located::input_start`].
    pub fn input_offset(&self, pos: Position) -> usize {
        self.input_at(pos.byte_offset.saturating_sub(self.base.byte_offset))
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
        let decoded = self.decoder.decode_with(bytes, last, &mut self.buf, &mut |name| unknown.as_mut().and_then(|f| f(name)));
        // Bytes the decoder dropped ahead of the text still count in input
        // offsets, as libexpat counts them.
        self.input_base += std::mem::take(&mut self.decoder.skipped);
        match decoded {
            Err(XmlError::Encoding {
                kind: kind @ (ErrorKind::InvalidToken | ErrorKind::PartialChar | ErrorKind::UnclosedToken),
                reason,
                ..
            }) => {
                let deferral = std::mem::replace(&mut self.deferral, false);
                let parsed = self.run(false, &mut *handler);
                self.deferral = deferral;
                parsed?;
                Err(XmlError::Encoding { pos: self.base.advance_bytes(self.after_cr, self.buf.as_bytes()), kind, reason })
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

    /// Reparse deferral: whether the buffer has grown enough since the last
    /// attempt stopped at an incomplete construct to try again: by at least
    /// a quarter, or past a power of two, and (unless it has doubled) by at
    /// least 1 KiB. libexpat retries about as often when large chunks keep
    /// arriving (pyexpat feeds 1 MiB at a time). The attempts for a
    /// construct of length n cost at most about 9n in all: 5n for the
    /// quarter steps, 4n for the powers of two.
    fn worth_retrying(&self) -> bool {
        if let Some((_, prefix)) = self.dtd_stall {
            // libexpat's rule, from its partial token: once it has doubled.
            return self.buffered_input().saturating_sub(prefix) >= 2 * self.stalled_at;
        }
        let (now, then) = (self.buffered_input(), self.stalled_at);
        let grown = now.saturating_sub(then);
        now >= 2 * then
            || (grown >= 1024 && (4 * grown >= then || now.next_power_of_two() > then.next_power_of_two()))
    }

    /// Whether the character before offset `at` in `buf` is `\r` (for the
    /// first, whether the text consumed before `buf` ended with one), so a
    /// `\n` at `at` completes that line end.
    fn cr_before(&self, at: usize) -> bool {
        match at.checked_sub(1) {
            Some(i) => self.buf.as_bytes()[i] == b'\r',
            None => self.after_cr,
        }
    }

    /// The input held for the next attempt, in bytes as given (as libexpat
    /// measures its buffer): `buf`, and bytes not yet decoded. Measured on
    /// from the last measure, so feeding in small pieces stays linear.
    fn buffered_input(&self) -> usize {
        let (done, bytes) = self.measured_end.get();
        let bytes = bytes + self.decoder.input_len(&self.buf[done..]);
        self.measured_end.set((self.buf.len(), bytes));
        bytes + self.decoder.pending_len()
    }

    /// Tokenise and handle as much of `buf` as is complete, then drop the
    /// consumed text.
    fn run(&mut self, last: bool, mut handler: impl FnMut(Located<'_>)) -> Result<()> {
        if !last && self.deferral && self.stalled_at > 0 && !self.stall_fresh && !self.worth_retrying() {
            return Ok(()); // reparse deferral: wait for more input
        }
        let base = self.base;
        let mut lexer = if self.at_start { Lexer::new(&self.buf) } else { Lexer::continuing(&self.buf) }
            .with_edition(self.state.edition)
            .with_libexpat_leniency(self.state.libexpat_leniency)
            .with_text_decl(self.state.entity_mode)
            .with_cdata_pieces(self.state.cdata_pieces, self.in_cdata)
            .in_subset(self.in_subset)
            .after_cr_at_start(self.after_cr);
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
                    // Whitespace after the root element whose last character
                    // is a `\r` at the end of the input so far: libexpat
                    // delivers that `\r` on its own, after the rest.
                    let mut lone_cr = None;
                    if let Token::Text(text) = tok {
                        if !self.state.in_content()
                            && !self.state.before_root()
                            && end.byte_offset == self.buf.len()
                            && text.len() > 1
                            && text.ends_with('\r')
                            && !text.contains(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'))
                        {
                            let head: &str = &text[..text.len() - 1];
                            let at = start.advance_bytes(self.cr_before(start.byte_offset), head.as_bytes());
                            lone_cr = Some((at, end));
                            (end, tok) = (at, Token::Text(head));
                        }
                    }
                    let cut_off = !last && end.byte_offset == self.buf.len();
                    if let (Token::Text(text) | Token::CDataText(text), true) = (&tok, cut_off) {
                        // Text may continue in the next chunk: deliver it up
                        // to a point the next chunk can't change (a `\r` may
                        // start a line end, `]` a `]]>`).
                        let mut keep = text.len() - text.trim_end_matches(['\r', ']']).len();
                        // Outside the root element, libexpat delivers
                        // whitespace up to the end of the chunk: after the
                        // root, `\r` too; before it, all but a last `\r`,
                        // which may begin a line end.
                        if matches!(tok, Token::Text(_)) && !self.state.in_content() {
                            keep = (self.state.before_root() && text.ends_with('\r')) as usize;
                        }
                        // Before the root, text other than whitespace is an
                        // error whose position depends on the whole run.
                        let stray = matches!(tok, Token::Text(_))
                            && self.state.before_root()
                            && text.contains(|c| !matches!(c, ' ' | '\t' | '\r' | '\n'));
                        if keep == text.len() || stray {
                            break;
                        }
                        let text: &str = &text[..text.len() - keep];
                        end = start.advance_bytes(self.cr_before(start.byte_offset), text.as_bytes());
                        tok = if matches!(tok, Token::Text(_)) { Token::Text(text) } else { Token::CDataText(text) };
                    }
                    // In the internal subset likewise: all but a last `\r`.
                    if let (Token::SubsetWhitespace(ws), true) = (&tok, cut_off) {
                        if let Some(head) = ws.strip_suffix('\r') {
                            if head.is_empty() {
                                break;
                            }
                            end = start.advance_bytes(self.cr_before(start.byte_offset), head.as_bytes());
                            tok = Token::SubsetWhitespace(head);
                        }
                    }
                    // At the very end, a `]` or `\r` that could have begun a
                    // `]]>` or a line end leaves the section unclosed there,
                    // as libexpat reports it.
                    let mut unclosed = None;
                    if let (Token::CDataText(text), true) = (&tok, last && end.byte_offset == self.buf.len()) {
                        let text: &str = text.trim_end_matches([']', '\r']);
                        if text.len() < end.byte_offset - start.byte_offset {
                            let at = start.advance_bytes(self.cr_before(start.byte_offset), text.as_bytes());
                            if text.is_empty() {
                                return Err(XmlError::UnexpectedEof { pos: at.rebase(base), context: "CDATA" });
                            }
                            (end, tok, unclosed) = (at, Token::CDataText(text), Some(at));
                        }
                    }
                    // Text in content running into a `]]>`: libexpat's text
                    // token starts after the last line end, so the text
                    // after it is part of the bad token and not delivered.
                    let mut doomed = false;
                    if let Token::Text(text) = tok {
                        if self.state.in_content() && self.buf[end.byte_offset..].starts_with("]]>") {
                            match text.rfind(['\n', '\r']) {
                                Some(i) => {
                                    let kept: &str = &text[..=i];
                                    (end, tok, doomed) = (start.advance_bytes(self.cr_before(start.byte_offset), kept.as_bytes()), Token::Text(kept), true);
                                }
                                None => continue,
                            }
                        }
                    }
                    match tok {
                        Token::CDataStart => self.in_cdata = true,
                        Token::CDataEnd => self.in_cdata = false,
                        Token::Doctype { has_internal_subset: true, .. } => self.in_subset = true,
                        Token::DoctypeEnd => self.in_subset = false,
                        _ => {}
                    }
                    let dtd: Dtd = lexer.take_dtd();
                    self.state.span = Span { start, end };
                    self.state.cr_before_token = self.cr_before(start.byte_offset);
                    self.state.direct_bytes = end.rebase(base).byte_offset as u64;
                    self.state.handle(tok, dtd, &mut out).map_err(|e| e.rebase(base))?;
                    for (event, span, entity_raw) in out.drain(..) {
                        let input_start = self.input_at(span.start.byte_offset);
                        let raw = self.buf.get(span.start.byte_offset..span.end.byte_offset).unwrap_or("");
                        let span = Span { start: span.start.rebase(base), end: span.end.rebase(base) };
                        handler(Located { event, span, raw, input_start, entity_raw });
                    }
                    consumed = Some(end);
                    if let Some((cr_start, cr_end)) = lone_cr {
                        self.state.span = Span { start: cr_start, end: cr_end };
                        self.state.cr_before_token = self.cr_before(cr_start.byte_offset);
                        self.state.direct_bytes = cr_end.rebase(base).byte_offset as u64;
                        let cr: &str = &self.buf[cr_start.byte_offset..cr_end.byte_offset];
                        self.state.handle(Token::Text(cr), Dtd::default(), &mut out).map_err(|e| e.rebase(base))?;
                        for (event, span, entity_raw) in out.drain(..) {
                            let input_start = self.input_at(span.start.byte_offset);
                            let raw = self.buf.get(span.start.byte_offset..span.end.byte_offset).unwrap_or("");
                            let span = Span { start: span.start.rebase(base), end: span.end.rebase(base) };
                            handler(Located { event, span, raw, input_start, entity_raw });
                        }
                        consumed = Some(cr_end);
                        continue;
                    }
                    if let Some(at) = unclosed {
                        return Err(XmlError::UnexpectedEof { pos: at.rebase(base), context: "CDATA" });
                    }
                    if !doomed && end.byte_offset < lexer.position().byte_offset {
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
                // Nor can a whole `]]>` in content (reported at its `>`).
                Err(e) if self.buf.as_bytes()[..(e.position().byte_offset + 1).min(self.buf.len())].ends_with(b"]]>") => {
                    return Err(e.rebase(base))
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
            self.input_base = self.input_at(end.byte_offset);
            self.measured.set((0, 0));
            self.measured_end.set((0, 0));
            self.base = end.rebase(base);
            self.after_cr = self.buf[..end.byte_offset].ends_with('\r');
            self.buf.drain(..end.byte_offset);
            self.at_start = false;
        }
        // Whatever is left is an incomplete construct (or held-back text).
        self.stalled_at = if last { 0 } else { self.buffered_input() };
        self.stall_fresh = consumed.is_some();
        // In the DOCTYPE, libexpat's tokens are smaller than a declaration
        // (keywords, names, literals): it makes progress, and measures its
        // deferral, token by token. Emulated while the declaration held is
        // short, so the re-scans stay cheap; beyond that, the rule above.
        let previous = self.dtd_stall.take();
        if !last && (self.in_subset || self.buf.starts_with("<!DOCTYPE")) && self.buf.len() <= 1024 {
            let boundary = crate::lexer::dtd_token_boundary(&self.buf);
            let prefix = self.decoder.input_len(&self.buf[..boundary]);
            self.stall_fresh |= previous.is_some_and(|(b, _)| boundary > b);
            self.stalled_at -= prefix.min(self.stalled_at);
            self.dtd_stall = Some((boundary, prefix));
        }
        Ok(())
    }
}

/// The largest character boundary in `s` at or before `at`.
fn floor_char_boundary(s: &str, mut at: usize) -> usize {
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}
