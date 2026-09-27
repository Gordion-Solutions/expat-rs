//! XML 1.0 tokeniser.
//!
//! Stream-style: borrows the input, returns successive `Token`s. No
//! allocation per token (attributes vector is the only allocation, and it
//! lives in the token).
//!
//! Implements the lexical rules of W3C XML 1.0 (Fifth Edition) §2 and §3.
//! Each non-trivial scan function carries a spec citation.

mod dtd;

use crate::entities::Dtd;
use crate::chars::{decode_char_ref, first_invalid_char, is_name_char, is_name_start_char, Edition};
use crate::error::{Position, Result, XmlError};
use crate::token::{Attr, Token, XmlDecl};

pub struct Lexer<'a> {
    src: &'a [u8],
    pos: Position,
    /// First character in the input that is not a §2.2 Char. Found with one
    /// up-front scan; reported as soon as a token reaches past it.
    first_invalid: Option<(usize, char)>,
    /// Which edition's Name rules apply (§2.3 vs. 4th ed. Appendix B).
    edition: Edition,
    /// Byte offset where the document proper starts: 0, or 3 after a UTF-8
    /// byte order mark. The XML declaration is only recognised here.
    doc_start: usize,
    /// Declarations recorded while tokenising the DOCTYPE (see `dtd.rs`).
    dtd: Dtd,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        // §4.3.3 / Appendix F: a leading byte order mark is not part of the
        // document's character data.
        let doc_start = if src.starts_with('\u{FEFF}') { '\u{FEFF}'.len_utf8() } else { 0 };
        Self {
            src: src.as_bytes(),
            pos: Position { byte_offset: doc_start, ..Position::start() },
            first_invalid: first_invalid_char(src),
            edition: Edition::default(),
            doc_start,
            dtd: Dtd::default(),
        }
    }

    /// Current position: where the next token starts.
    pub fn position(&self) -> Position {
        self.pos
    }

    /// Take the declarations recorded by the most recent DOCTYPE token.
    pub(crate) fn take_dtd(&mut self) -> Dtd {
        std::mem::take(&mut self.dtd)
    }

    /// Skip a text declaration at the very start of an external parsed
    /// entity, if there is one. §4.3.1 [Production 77]:
    ///
    ///   TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'
    ///
    /// Unlike the XML declaration, `version` is optional, `encoding` is
    /// required and `standalone` is not allowed.
    pub(crate) fn skip_text_decl(&mut self) -> Result<()> {
        let rest = &self.src[self.pos.byte_offset..];
        let is_decl = rest.starts_with(b"<?xml")
            && matches!(rest.get(5), Some(b' ' | b'\t' | b'\r' | b'\n'));
        if !is_decl {
            return Ok(());
        }
        let not_wf = |pos, reason: &str| XmlError::NotWellFormed { pos, reason: reason.into() };
        self.pos.byte_offset += 5;
        self.pos.column += 5;
        self.skip_whitespace();
        if self.try_keyword(b"version") {
            self.scan_eq()?;
            let pos = self.pos;
            let v = self.scan_attr_value()?;
            if !self.is_valid_version(v) {
                return Err(not_wf(pos, &format!("invalid XML version number {v:?}")));
            }
            if !self.skip_whitespace() {
                return Err(not_wf(self.pos, "whitespace required before encoding in text declaration"));
            }
        }
        if !self.try_keyword(b"encoding") {
            return Err(not_wf(self.pos, "text declaration requires an encoding declaration"));
        }
        self.scan_eq()?;
        let pos = self.pos;
        let name = self.scan_attr_value()?;
        if !is_valid_enc_name(name) {
            return Err(not_wf(pos, &format!("invalid encoding name {name:?}")));
        }
        self.skip_whitespace();
        if self.current() == Some(b'?') && self.peek(1) == Some(b'>') {
            self.bump(); self.bump();
            Ok(())
        } else {
            Err(not_wf(self.pos, "expected '?>' to close text declaration (standalone is not allowed here)"))
        }
    }

    /// Select the XML 1.0 edition whose Name rules apply. Default: Fifth.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.edition = edition;
        self
    }

    /// Line/column of an arbitrary byte offset, for errors reported away
    /// from the current position.
    fn position_at(&self, byte_offset: usize) -> Position {
        let before = &self.src[..byte_offset];
        let line = 1 + before.iter().filter(|&&b| b == b'\n').count() as u32;
        let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let column = 1 + String::from_utf8_lossy(&before[line_start..]).chars().count() as u32;
        Position { line, column, byte_offset }
    }

    /// Advance past one byte, updating line/column.
    fn bump(&mut self) {
        let b = self.src[self.pos.byte_offset];
        self.pos.byte_offset += 1;
        if b == b'\n' {
            self.pos.line += 1;
            self.pos.column = 1;
        } else {
            self.pos.column += 1;
        }
    }

    fn peek(&self, n: usize) -> Option<u8> {
        self.src.get(self.pos.byte_offset + n).copied()
    }

    fn is_eof(&self) -> bool {
        self.pos.byte_offset >= self.src.len()
    }

    fn current(&self) -> Option<u8> {
        self.peek(0)
    }

    /// Skip XML whitespace per §2.3 [Production 3]: S ::= (#x20 | #x9 | #xD | #xA)+
    /// Returns whether any was skipped, for productions where S is required.
    fn skip_whitespace(&mut self) -> bool {
        let start = self.pos.byte_offset;
        while let Some(c) = self.current() {
            if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
                self.bump();
            } else {
                break;
            }
        }
        self.pos.byte_offset != start
    }

    /// Return the slice from `start` to current byte offset.
    fn slice(&self, start: usize) -> &'a str {
        // SAFETY: `start..byte_offset` is always within the original `&str`
        // because `bump` only ever increments through valid UTF-8 boundaries
        // (we never bump mid-codepoint — this lexer is byte-oriented for ASCII
        // metacharacters and treats non-ASCII as opaque payload bytes).
        std::str::from_utf8(&self.src[start..self.pos.byte_offset]).unwrap_or("")
    }

    /// Decode the char at the current byte position. UTF-8 codepoints are
    /// 1–4 bytes; the lead byte gives the length. (Decoding a fixed 4-byte
    /// window instead fails whenever the window ends mid-codepoint, e.g.
    /// the `x>€` in `<x>€</x>`.)
    fn current_char(&self) -> Option<(char, usize)> {
        self.char_at(self.pos.byte_offset)
    }

    /// Decode the char starting at `offset` (see `current_char`).
    fn char_at(&self, offset: usize) -> Option<(char, usize)> {
        let bytes = self.src.get(offset..)?;
        let len = match *bytes.first()? {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _           => 4,
        };
        let s = std::str::from_utf8(bytes.get(..len)?).ok()?;
        let c = s.chars().next()?;
        Some((c, c.len_utf8()))
    }

    /// Advance past one Unicode codepoint by its UTF-8 byte length.
    fn bump_char(&mut self, byte_len: usize) {
        // \n is single-byte in UTF-8, so newline tracking works at byte level.
        let is_newline = byte_len == 1 && self.src[self.pos.byte_offset] == b'\n';
        self.pos.byte_offset += byte_len;
        if is_newline {
            self.pos.line += 1;
            self.pos.column = 1;
        } else {
            self.pos.column += 1;
        }
    }

    /// Scan a Name per §2.3 [Production 5]. Honours full Unicode
    /// NameStartChar / NameChar ranges.
    fn scan_name(&mut self) -> Result<&'a str> {
        let start = self.pos.byte_offset;
        match self.current_char() {
            Some((c, len)) if is_name_start_char(c, self.edition) => self.bump_char(len),
            Some((c, _))   => return Err(XmlError::InvalidChar { pos: self.pos, char: c }),
            None           => return Err(XmlError::UnexpectedEof { pos: self.pos, context: "Name" }),
        }
        while let Some((c, len)) = self.current_char() {
            if is_name_char(c, self.edition) { self.bump_char(len); } else { break; }
        }
        Ok(self.slice(start))
    }

    /// Scan an attribute value per §3.1 [Production 10]: AttValue ::= '"' ([^<&"] | Reference)* '"'
    /// (or single-quoted variant). References are syntax-checked (and
    /// character references validated) but not expanded; the *literal*
    /// slice between the quotes is returned.
    fn scan_attr_value(&mut self) -> Result<&'a str> {
        let quote = match self.current() {
            Some(q @ (b'"' | b'\'')) => q,
            Some(c) => return Err(XmlError::NotWellFormed {
                pos: self.pos,
                reason: format!("expected quote to start attribute value, got {:?}", c as char),
            }),
            None => return Err(XmlError::UnexpectedEof { pos: self.pos, context: "AttValue" }),
        };
        self.bump(); // consume opening quote
        let start = self.pos.byte_offset;
        loop {
            match self.current() {
                Some(c) if c == quote => {
                    let s = self.slice(start);
                    self.bump(); // consume closing quote
                    return Ok(s);
                }
                Some(b'<') => return Err(XmlError::NotWellFormed {
                    pos: self.pos, reason: "'<' not allowed in attribute value".into(),
                }),
                // A literal '&' must begin a well-formed Reference.
                Some(b'&') => { self.scan_reference()?; }
                Some(_) => self.bump(),
                None => return Err(XmlError::UnexpectedEof {
                    pos: self.pos, context: "AttValue",
                }),
            }
        }
    }

    /// Scan a `<!-- ... -->` comment per §2.5 [Production 15]. The leading
    /// `<!--` has been consumed by the caller. Returns the body (without the
    /// surrounding markers) and consumes the `-->`.
    fn scan_comment_body(&mut self) -> Result<&'a str> {
        let start = self.pos.byte_offset;
        loop {
            if self.is_eof() {
                return Err(XmlError::UnexpectedEof { pos: self.pos, context: "Comment" });
            }
            if self.current() == Some(b'-') && self.peek(1) == Some(b'-') {
                let body = self.slice(start);
                // Per §2.5: "for compatibility, the string '--' MUST NOT occur
                // within comments." If we see -- followed by anything other
                // than > we error.
                if self.peek(2) != Some(b'>') {
                    return Err(XmlError::NotWellFormed {
                        pos: self.pos,
                        reason: "'--' not allowed within a comment".into(),
                    });
                }
                self.bump(); self.bump(); self.bump(); // consume -->
                return Ok(body);
            }
            self.bump();
        }
    }

    /// Scan a `<![CDATA[...]]>` section per §2.7 [Production 18]. The leading
    /// `<![CDATA[` has been consumed.
    fn scan_cdata_body(&mut self) -> Result<&'a str> {
        let start = self.pos.byte_offset;
        loop {
            if self.is_eof() {
                return Err(XmlError::UnexpectedEof { pos: self.pos, context: "CDATA" });
            }
            if self.current() == Some(b']') && self.peek(1) == Some(b']') && self.peek(2) == Some(b'>') {
                let body = self.slice(start);
                self.bump(); self.bump(); self.bump();
                return Ok(body);
            }
            self.bump();
        }
    }

    /// Scan a processing instruction body per §2.6 [Production 16]:
    /// `<?target ...?>`. The leading `<?` has been consumed; the target is
    /// scanned by the caller.
    fn scan_pi_body(&mut self) -> Result<&'a str> {
        // PI ::= '<?' PITarget (S (Char* - (Char* '?>' Char*)))? '?>'
        // so the target is followed by either '?>' or whitespace.
        let at_close = self.current() == Some(b'?') && self.peek(1) == Some(b'>');
        if !at_close && !self.skip_whitespace() {
            return Err(XmlError::NotWellFormed {
                pos: self.pos,
                reason: "expected whitespace or '?>' after processing instruction target".into(),
            });
        }
        let start = self.pos.byte_offset;
        loop {
            if self.is_eof() {
                return Err(XmlError::UnexpectedEof { pos: self.pos, context: "PI" });
            }
            if self.current() == Some(b'?') && self.peek(1) == Some(b'>') {
                let body = self.slice(start);
                self.bump(); self.bump();
                return Ok(body);
            }
            self.bump();
        }
    }

    /// Top-level: produce the next token, or `Ok(None)` at end of input.
    pub fn next_token(&mut self) -> Result<Option<Token<'a>>> {
        let tok = self.scan_token()?;
        // Per §2.2: every character in the document must match Char.
        if let Some((offset, c)) = self.first_invalid {
            if offset < self.pos.byte_offset {
                return Err(XmlError::InvalidChar { pos: self.position_at(offset), char: c });
            }
        }
        Ok(tok)
    }

    fn scan_token(&mut self) -> Result<Option<Token<'a>>> {
        if self.is_eof() {
            return Ok(None);
        }
        // After the prolog, characters between tags are Text. Inside the
        // prolog (before the root element), only whitespace is allowed.
        match self.current().unwrap() {
            b'<' => self.scan_open_construct().map(Some),
            b'&' => self.scan_reference().map(Some),
            _    => self.scan_text().map(Some),
        }
    }

    /// Dispatch on whatever follows a `<`.
    fn scan_open_construct(&mut self) -> Result<Token<'a>> {
        debug_assert_eq!(self.current(), Some(b'<'));
        let start = self.pos.byte_offset;
        match self.peek(1) {
            Some(b'/') => {
                self.bump(); self.bump(); // </
                let name = self.scan_name()?;
                self.skip_whitespace();
                if self.current() != Some(b'>') {
                    return Err(XmlError::NotWellFormed {
                        pos: self.pos, reason: "expected '>' to close end tag".into(),
                    });
                }
                self.bump();
                Ok(Token::EndTag(name))
            }
            Some(b'!') => {
                // Could be <!--, <![CDATA[, <!DOCTYPE
                if self.src[self.pos.byte_offset..].starts_with(b"<!--") {
                    self.pos.byte_offset += 4; // shortcut bump
                    self.pos.column += 4;
                    Ok(Token::Comment(self.scan_comment_body()?))
                } else if self.src[self.pos.byte_offset..].starts_with(b"<![CDATA[") {
                    self.pos.byte_offset += 9;
                    self.pos.column += 9;
                    Ok(Token::CData(self.scan_cdata_body()?))
                } else if self.src[self.pos.byte_offset..].starts_with(b"<!DOCTYPE") {
                    self.scan_doctype()
                } else {
                    Err(XmlError::NotWellFormed {
                        pos: self.pos, reason: "unrecognised <! construct".into(),
                    })
                }
            }
            Some(b'?') => {
                self.bump(); self.bump(); // <?
                let target = self.scan_name()?;
                // §2.8: `<?xml` is the XML declaration, and only at the very
                // start of the document. §2.6: any other target matching
                // [Xx][Mm][Ll] is reserved.
                if target == "xml" && start == self.doc_start {
                    self.scan_xmldecl_after_target()
                } else if target.eq_ignore_ascii_case("xml") {
                    Err(XmlError::NotWellFormed {
                        pos: self.pos,
                        reason: if target == "xml" {
                            "XML declaration allowed only at the start of the document".into()
                        } else {
                            format!("processing instruction target {target:?} is reserved")
                        },
                    })
                } else {
                    let body = self.scan_pi_body()?;
                    Ok(Token::ProcessingInstruction { target, body })
                }
            }
            Some(_) => self.scan_start_or_empty(),
            None    => Err(XmlError::UnexpectedEof { pos: self.pos, context: "tag" }),
        }
    }

    fn scan_start_or_empty(&mut self) -> Result<Token<'a>> {
        debug_assert_eq!(self.current(), Some(b'<'));
        self.bump(); // <
        let name = self.scan_name()?;
        let mut attributes = Vec::new();
        loop {
            // §3.1 [Production 40]: STag ::= '<' Name (S Attribute)* S? '>'
            let had_space = self.skip_whitespace();
            match self.current() {
                Some(b'>') => {
                    self.bump();
                    return Ok(Token::StartTag { name, attributes });
                }
                Some(b'/') => {
                    self.bump();
                    if self.current() != Some(b'>') {
                        return Err(XmlError::NotWellFormed {
                            pos: self.pos, reason: "expected '>' after '/'".into(),
                        });
                    }
                    self.bump();
                    return Ok(Token::EmptyTag { name, attributes });
                }
                Some(_) if self.current_char().is_some_and(|(c, _)| is_name_start_char(c, self.edition)) => {
                    if !had_space {
                        return Err(XmlError::NotWellFormed {
                            pos: self.pos, reason: "whitespace required before attribute".into(),
                        });
                    }
                    let attr_pos = self.pos;
                    let aname = self.scan_name()?;
                    self.skip_whitespace();
                    if self.current() != Some(b'=') {
                        return Err(XmlError::NotWellFormed {
                            pos: self.pos, reason: "expected '=' after attribute name".into(),
                        });
                    }
                    self.bump();
                    self.skip_whitespace();
                    let value = self.scan_attr_value()?;
                    attributes.push(Attr { name: aname, value, pos: attr_pos });
                }
                Some(c) => return Err(XmlError::NotWellFormed {
                    pos: self.pos,
                    reason: format!("unexpected {:?} in start tag", c as char),
                }),
                None => return Err(XmlError::UnexpectedEof {
                    pos: self.pos, context: "start tag",
                }),
            }
        }
    }

    /// XML declaration per §2.8 [Productions 23–26, 32, 80, 81]; the
    /// leading `<?xml` has been consumed.
    ///
    ///   XMLDecl      ::= '<?xml' VersionInfo EncodingDecl? SDDecl? S? '?>'
    ///   VersionInfo  ::= S 'version' Eq ("'" VersionNum "'" | '"' VersionNum '"')
    ///   EncodingDecl ::= S 'encoding' Eq ('"' EncName '"' | "'" EncName "'")
    ///   SDDecl       ::= S 'standalone' Eq (("'" ('yes' | 'no') "'") | ('"' ('yes' | 'no') '"'))
    ///
    /// The three pseudo-attributes are each preceded by required S and must
    /// appear in this order.
    fn scan_xmldecl_after_target(&mut self) -> Result<Token<'a>> {
        let not_wf = |pos, reason: &str| XmlError::NotWellFormed { pos, reason: reason.into() };

        if !self.skip_whitespace() || !self.try_keyword(b"version") {
            return Err(not_wf(self.pos, "XML declaration must start with version"));
        }
        self.scan_eq()?;
        let version_pos = self.pos;
        let version = self.scan_attr_value()?;
        if !self.is_valid_version(version) {
            return Err(not_wf(version_pos, &format!("invalid XML version number {version:?}")));
        }

        let mut had_space = self.skip_whitespace();
        let mut encoding = None;
        if had_space && self.try_keyword(b"encoding") {
            self.scan_eq()?;
            let enc_pos = self.pos;
            let name = self.scan_attr_value()?;
            if !is_valid_enc_name(name) {
                return Err(not_wf(enc_pos, &format!("invalid encoding name {name:?}")));
            }
            encoding = Some(name);
            had_space = self.skip_whitespace();
        }

        let mut standalone = None;
        if had_space && self.try_keyword(b"standalone") {
            self.scan_eq()?;
            let sd_pos = self.pos;
            standalone = Some(match self.scan_attr_value()? {
                "yes" => true,
                "no"  => false,
                other => return Err(not_wf(sd_pos, &format!("standalone must be 'yes' or 'no', got {other:?}"))),
            });
            self.skip_whitespace();
        }

        if self.current() == Some(b'?') && self.peek(1) == Some(b'>') {
            self.bump(); self.bump();
            return Ok(Token::XmlDecl(XmlDecl { version, encoding, standalone }));
        }
        if self.is_eof() {
            return Err(XmlError::UnexpectedEof { pos: self.pos, context: "XML declaration" });
        }
        Err(not_wf(self.pos, "unexpected content in XML declaration \
                              (expected version, encoding, standalone in that order, then '?>')"))
    }

    /// §2.3 [Production 25]: Eq ::= S? '=' S?
    fn scan_eq(&mut self) -> Result<()> {
        self.skip_whitespace();
        if self.current() != Some(b'=') {
            return Err(XmlError::NotWellFormed { pos: self.pos, reason: "expected '='".into() });
        }
        self.bump();
        self.skip_whitespace();
        Ok(())
    }

    /// §2.8 [Production 26]: VersionNum. Fifth Edition: '1.' [0-9]+;
    /// Fourth Edition: exactly '1.0'.
    fn is_valid_version(&self, v: &str) -> bool {
        match self.edition {
            Edition::Fourth => v == "1.0",
            Edition::Fifth  => v.strip_prefix("1.")
                .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())),
        }
    }

    fn try_keyword(&mut self, kw: &[u8]) -> bool {
        if self.src[self.pos.byte_offset..].starts_with(kw) {
            // Make sure it's not a prefix of a longer name
            let after = self.pos.byte_offset + kw.len();
            if !self.char_at(after).is_some_and(|(c, _)| is_name_char(c, self.edition)) {
                self.pos.byte_offset += kw.len();
                self.pos.column += kw.len() as u32;
                return true;
            }
        }
        false
    }

    /// Scan character data per §2.4 [Production 14] until the next `<` or `&`.
    fn scan_text(&mut self) -> Result<Token<'a>> {
        let start = self.pos.byte_offset;
        while let Some(c) = self.current() {
            if c == b'<' || c == b'&' { break; }
            // Per §2.4: `]]>` MUST NOT occur in character data
            if c == b']' && self.peek(1) == Some(b']') && self.peek(2) == Some(b'>') {
                return Err(XmlError::NotWellFormed {
                    pos: self.pos,
                    reason: "']]>' not allowed in character data".into(),
                });
            }
            self.bump();
        }
        Ok(Token::Text(self.slice(start)))
    }

    /// Reference per §4.1 [Productions 66–68]: `&name;` or `&#nnn;` / `&#xhh;`.
    fn scan_reference(&mut self) -> Result<Token<'a>> {
        debug_assert_eq!(self.current(), Some(b'&'));
        self.bump();
        if self.current() == Some(b'#') {
            self.bump();
            let body_start = self.pos.byte_offset;
            while let Some(c) = self.current() {
                if c == b';' { break; }
                if !(c.is_ascii_alphanumeric()) {
                    return Err(XmlError::NotWellFormed {
                        pos: self.pos,
                        reason: format!("invalid character {:?} in numeric character reference", c as char),
                    });
                }
                self.bump();
            }
            if self.current() != Some(b';') {
                return Err(XmlError::NotWellFormed { pos: self.pos, reason: "expected ';' to close character reference".into() });
            }
            let body = self.slice(body_start);
            let c = decode_char_ref(body).map_err(|reason| XmlError::NotWellFormed { pos: self.pos, reason })?;
            self.bump();
            Ok(Token::CharRef(c))
        } else {
            let name = self.scan_name()?;
            if self.current() != Some(b';') {
                return Err(XmlError::NotWellFormed { pos: self.pos, reason: "expected ';' to close entity reference".into() });
            }
            self.bump();
            Ok(Token::EntityRef(name))
        }
    }
}

/// §4.3.3 [Production 81]: EncName ::= [A-Za-z] ([A-Za-z0-9._] | '-')*
fn is_valid_enc_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
