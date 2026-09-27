//! Document type declaration: syntax of the DOCTYPE and its internal subset.
//!
//! Implements the well-formedness side of W3C XML 1.0 (Fifth Edition)
//! §2.8 and §3.2–§4.7: every markup declaration in the internal subset is
//! tokenised and checked against its production. Nothing here validates a
//! document against its DTD (that is a validity concern, not
//! well-formedness). The declarations the parser needs (entities,
//! attribute defaults, parameter-entity references) are recorded in
//! `Lexer::dtd`, in document order, for the parser to collect with
//! `take_dtd`.

use super::Lexer;
use crate::entities::{expand_char_refs, Dtd, DtdDecl, EntityDef};
use crate::error::{Result, XmlError};
use crate::token::Token;

impl<'a> Lexer<'a> {
    /// §2.8 [Production 28]:
    ///
    ///   doctypedecl ::= '<!DOCTYPE' S Name (S ExternalID)? S?
    ///                   ('[' intSubset ']' S?)? '>'
    pub(super) fn scan_doctype(&mut self) -> Result<Token<'a>> {
        debug_assert!(self.src[self.pos.byte_offset..].starts_with(b"<!DOCTYPE"));
        self.advance_ascii(b"<!DOCTYPE".len());
        self.require_whitespace("after '<!DOCTYPE'")?;
        let name = self.scan_name()?;
        let body_start = self.pos.byte_offset;
        self.dtd = Dtd::default();

        let had_space = self.skip_whitespace();
        if self.at_keyword(b"SYSTEM") || self.at_keyword(b"PUBLIC") {
            if !had_space {
                return Err(self.not_wf("whitespace required before external ID"));
            }
            self.scan_external_id(false)?;
            self.dtd.external_subset = true;
            self.skip_whitespace();
        }
        if self.current() == Some(b'[') {
            self.bump();
            self.scan_internal_subset()?;
            self.bump(); // ]
            self.skip_whitespace();
        }
        if self.current() != Some(b'>') {
            return Err(self.eof_or_not_wf("DOCTYPE", "expected '>' to close DOCTYPE"));
        }
        let body = self.slice(body_start).trim();
        self.bump();
        Ok(Token::Doctype { name, body })
    }

    /// §2.8 [Productions 28a, 28b]:
    ///
    ///   intSubset ::= (markupdecl | DeclSep)*
    ///   DeclSep   ::= PEReference | S
    ///   markupdecl ::= elementdecl | AttlistDecl | EntityDecl
    ///                | NotationDecl | PI | Comment
    ///
    /// Stops at the closing ']' without consuming it.
    fn scan_internal_subset(&mut self) -> Result<()> {
        loop {
            self.skip_whitespace();
            let rest = &self.src[self.pos.byte_offset..];
            if rest.is_empty() {
                return Err(XmlError::UnexpectedEof { pos: self.pos, context: "DOCTYPE internal subset" });
            } else if rest[0] == b']' {
                return Ok(());
            } else if rest[0] == b'%' {
                self.scan_pe_reference()?;
            } else if rest.starts_with(b"<?") {
                self.advance_ascii(2);
                let target = self.scan_name()?;
                // §2.6: targets matching [Xx][Mm][Ll] are reserved.
                if target.eq_ignore_ascii_case("xml") {
                    return Err(self.not_wf("processing instruction target 'xml' is reserved"));
                }
                self.scan_pi_body()?;
            } else if rest.starts_with(b"<!--") {
                self.advance_ascii(4);
                self.scan_comment_body()?;
            } else if rest.starts_with(b"<!ELEMENT") {
                self.advance_ascii(b"<!ELEMENT".len());
                self.scan_element_decl()?;
            } else if rest.starts_with(b"<!ATTLIST") {
                self.advance_ascii(b"<!ATTLIST".len());
                self.scan_attlist_decl()?;
            } else if rest.starts_with(b"<!ENTITY") {
                self.advance_ascii(b"<!ENTITY".len());
                self.scan_entity_decl()?;
            } else if rest.starts_with(b"<!NOTATION") {
                self.advance_ascii(b"<!NOTATION".len());
                self.scan_notation_decl()?;
            } else if rest.starts_with(b"<![") {
                // §3.4: conditional sections occur only in the external subset.
                return Err(self.not_wf("conditional section not allowed in the internal subset"));
            } else {
                return Err(self.not_wf("expected markup declaration, PI, comment, \
                                        parameter-entity reference or ']' in internal subset"));
            }
        }
    }

    /// §4.1 [Production 69]: PEReference ::= '%' Name ';'
    fn scan_pe_reference(&mut self) -> Result<()> {
        self.bump(); // %
        self.scan_name()?;
        self.expect_byte(b';', "expected ';' to close parameter-entity reference")?;
        self.dtd.decls.push(DtdDecl::PeRef);
        Ok(())
    }

    /// §3.2 [Production 45]:
    ///
    ///   elementdecl ::= '<!ELEMENT' S Name S contentspec S? '>'
    ///   contentspec ::= 'EMPTY' | 'ANY' | Mixed | children
    fn scan_element_decl(&mut self) -> Result<()> {
        self.require_whitespace("after '<!ELEMENT'")?;
        self.scan_name()?;
        self.require_whitespace("after element name")?;
        if !(self.try_keyword(b"EMPTY") || self.try_keyword(b"ANY")) {
            self.expect_byte(b'(', "expected EMPTY, ANY or '(' in element declaration")?;
            self.skip_whitespace();
            if self.try_literal(b"#PCDATA") {
                self.scan_mixed()?;
            } else {
                self.scan_group_body()?;
            }
        }
        self.skip_whitespace();
        self.expect_byte(b'>', "expected '>' to close element declaration")
    }

    /// §3.2.2 [Production 51], after `'(' S? '#PCDATA'`:
    ///
    ///   Mixed ::= '(' S? '#PCDATA' (S? '|' S? Name)* S? ')*'
    ///           | '(' S? '#PCDATA' S? ')'
    fn scan_mixed(&mut self) -> Result<()> {
        let mut has_names = false;
        loop {
            self.skip_whitespace();
            match self.current() {
                Some(b')') => {
                    self.bump();
                    if self.current() == Some(b'*') {
                        self.bump();
                    } else if has_names {
                        return Err(self.not_wf("mixed content with element names must end in ')*'"));
                    }
                    return Ok(());
                }
                Some(b'|') => {
                    self.bump();
                    self.skip_whitespace();
                    self.scan_name()?;
                    has_names = true;
                }
                _ => return Err(self.eof_or_not_wf("element declaration",
                                                   "expected '|' or ')' in mixed content")),
            }
        }
    }

    /// §3.2.1 [Productions 47–50], after the opening '(':
    ///
    ///   cp     ::= (Name | choice | seq) ('?' | '*' | '+')?
    ///   choice ::= '(' S? cp ( S? '|' S? cp )+ S? ')'
    ///   seq    ::= '(' S? cp ( S? ',' S? cp )* S? ')'
    ///
    /// Also consumes the group's trailing occurrence indicator.
    fn scan_group_body(&mut self) -> Result<()> {
        let mut separator = None;
        loop {
            self.skip_whitespace();
            self.scan_content_particle()?;
            self.skip_whitespace();
            match self.current() {
                Some(b')') => {
                    self.bump();
                    self.skip_occurrence();
                    return Ok(());
                }
                Some(c @ (b'|' | b',')) => {
                    if separator.is_some_and(|s| s != c) {
                        return Err(self.not_wf("cannot mix '|' and ',' in one content-model group"));
                    }
                    separator = Some(c);
                    self.bump();
                }
                _ => return Err(self.eof_or_not_wf("element declaration",
                                                   "expected '|', ',' or ')' in content model")),
            }
        }
    }

    fn scan_content_particle(&mut self) -> Result<()> {
        if self.current() == Some(b'(') {
            self.bump();
            self.scan_group_body()
        } else {
            self.scan_name()?;
            self.skip_occurrence();
            Ok(())
        }
    }

    fn skip_occurrence(&mut self) {
        if matches!(self.current(), Some(b'?' | b'*' | b'+')) {
            self.bump();
        }
    }

    /// §3.3 [Productions 52, 53]:
    ///
    ///   AttlistDecl ::= '<!ATTLIST' S Name AttDef* S? '>'
    ///   AttDef      ::= S Name S AttType S DefaultDecl
    fn scan_attlist_decl(&mut self) -> Result<()> {
        self.require_whitespace("after '<!ATTLIST'")?;
        self.scan_name()?;
        loop {
            let had_space = self.skip_whitespace();
            if self.current() == Some(b'>') {
                self.bump();
                return Ok(());
            }
            if !had_space {
                return Err(self.eof_or_not_wf("attribute-list declaration",
                                              "whitespace required before attribute definition"));
            }
            self.scan_name()?;
            self.require_whitespace("after attribute name")?;
            self.scan_att_type()?;
            self.require_whitespace("after attribute type")?;
            self.scan_default_decl()?;
        }
    }

    /// §3.3.1 [Productions 54–59]:
    ///
    ///   AttType        ::= StringType | TokenizedType | EnumeratedType
    ///   StringType     ::= 'CDATA'
    ///   TokenizedType  ::= 'ID' | 'IDREF' | 'IDREFS' | 'ENTITY' | 'ENTITIES'
    ///                    | 'NMTOKEN' | 'NMTOKENS'
    ///   NotationType   ::= 'NOTATION' S '(' S? Name (S? '|' S? Name)* S? ')'
    ///   Enumeration    ::= '(' S? Nmtoken (S? '|' S? Nmtoken)* S? ')'
    fn scan_att_type(&mut self) -> Result<()> {
        const TYPES: [&[u8]; 8] = [
            b"CDATA", b"ID", b"IDREF", b"IDREFS", b"ENTITY", b"ENTITIES", b"NMTOKEN", b"NMTOKENS",
        ];
        if TYPES.iter().any(|t| self.try_keyword(t)) {
            return Ok(());
        }
        let notation = self.try_keyword(b"NOTATION");
        if notation {
            self.require_whitespace("after NOTATION")?;
        }
        self.expect_byte(b'(', "expected attribute type")?;
        loop {
            self.skip_whitespace();
            if notation { self.scan_name()?; } else { self.scan_nmtoken()?; }
            self.skip_whitespace();
            match self.current() {
                Some(b'|') => self.bump(),
                Some(b')') => { self.bump(); return Ok(()); }
                _ => return Err(self.eof_or_not_wf("attribute-list declaration",
                                                   "expected '|' or ')' in enumerated type")),
            }
        }
    }

    /// §2.3 [Production 7]: Nmtoken ::= (NameChar)+
    fn scan_nmtoken(&mut self) -> Result<()> {
        let start = self.pos.byte_offset;
        while let Some((c, len)) = self.current_char() {
            if crate::chars::is_name_char(c, self.edition) { self.bump_char(len); } else { break; }
        }
        if self.pos.byte_offset == start {
            return Err(self.eof_or_not_wf("attribute-list declaration", "expected name token"));
        }
        Ok(())
    }

    /// §3.3.2 [Production 60]:
    ///
    ///   DefaultDecl ::= '#REQUIRED' | '#IMPLIED' | (('#FIXED' S)? AttValue)
    fn scan_default_decl(&mut self) -> Result<()> {
        if self.try_literal(b"#REQUIRED") || self.try_literal(b"#IMPLIED") {
            return Ok(());
        }
        if self.try_literal(b"#FIXED") {
            self.require_whitespace("after #FIXED")?;
        }
        let pos = self.pos;
        let value = self.scan_attr_value()?;
        self.dtd.decls.push(DtdDecl::AttDefault { value: value.to_string(), pos });
        Ok(())
    }

    /// §4.2 [Productions 70–74, 76]:
    ///
    ///   GEDecl    ::= '<!ENTITY' S Name S EntityDef S? '>'
    ///   PEDecl    ::= '<!ENTITY' S '%' S Name S PEDef S? '>'
    ///   EntityDef ::= EntityValue | (ExternalID NDataDecl?)
    ///   PEDef     ::= EntityValue | ExternalID
    ///   NDataDecl ::= S 'NDATA' S Name
    fn scan_entity_decl(&mut self) -> Result<()> {
        self.require_whitespace("after '<!ENTITY'")?;
        let parameter = self.current() == Some(b'%');
        if parameter {
            self.bump();
            self.require_whitespace("after '%' in parameter-entity declaration")?;
        }
        let name = self.scan_name()?;
        self.require_whitespace("after entity name")?;
        let def = if matches!(self.current(), Some(b'"' | b'\'')) {
            EntityDef::Internal(expand_char_refs(self.scan_entity_value()?))
        } else {
            let (public_id, system_id) = self.scan_external_id(false)?;
            let mut unparsed = false;
            let had_space = self.skip_whitespace();
            if self.at_keyword(b"NDATA") {
                if parameter {
                    return Err(self.not_wf("NDATA not allowed on a parameter entity"));
                }
                if !had_space {
                    return Err(self.not_wf("whitespace required before NDATA"));
                }
                self.advance_ascii(b"NDATA".len());
                self.require_whitespace("after NDATA")?;
                self.scan_name()?;
                unparsed = true;
            }
            EntityDef::External {
                system_id: system_id.unwrap_or_default().to_string(),
                public_id: public_id.map(str::to_string),
                unparsed,
            }
        };
        self.skip_whitespace();
        self.expect_byte(b'>', "expected '>' to close entity declaration")?;
        self.dtd.decls.push(DtdDecl::Entity { name: name.to_string(), parameter, def });
        Ok(())
    }

    /// §2.3 [Production 9]:
    ///
    ///   EntityValue ::= '"' ([^%&"] | PEReference | Reference)* '"'
    ///                |  "'" ([^%&'] | PEReference | Reference)* "'"
    ///
    /// In the internal subset, §2.8 WFC "PEs in Internal Subset" forbids
    /// parameter-entity references inside markup declarations, so '%' is
    /// always an error here. Returns the literal value between the quotes.
    fn scan_entity_value(&mut self) -> Result<&'a str> {
        let quote = self.current().unwrap();
        self.bump();
        let start = self.pos.byte_offset;
        loop {
            match self.current() {
                None => return Err(XmlError::UnexpectedEof { pos: self.pos, context: "entity value" }),
                Some(c) if c == quote => {
                    let value = self.slice(start);
                    self.bump();
                    return Ok(value);
                }
                Some(b'%') => return Err(self.not_wf(
                    "parameter-entity reference not allowed within a declaration in the internal subset")),
                Some(b'&') => self.check_reference_syntax()?,
                Some(_) => self.bump(),
            }
        }
    }

    /// §4.1 [Productions 66–68]: `&Name;` or a valid character reference.
    /// Consumes the reference; expansion is not this function's job.
    fn check_reference_syntax(&mut self) -> Result<()> {
        self.scan_reference().map(|_| ())
    }

    /// §4.7 [Production 82]:
    ///
    ///   NotationDecl ::= '<!NOTATION' S Name S (ExternalID | PublicID) S? '>'
    fn scan_notation_decl(&mut self) -> Result<()> {
        self.require_whitespace("after '<!NOTATION'")?;
        self.scan_name()?;
        self.require_whitespace("after notation name")?;
        self.scan_external_id(true)?;
        self.skip_whitespace();
        self.expect_byte(b'>', "expected '>' to close notation declaration")
    }

    /// §4.2.2 [Productions 75, 83]:
    ///
    ///   ExternalID ::= 'SYSTEM' S SystemLiteral
    ///                | 'PUBLIC' S PubidLiteral S SystemLiteral
    ///   PublicID   ::= 'PUBLIC' S PubidLiteral
    ///
    /// With `public_id_ok` (notation declarations), the system literal after
    /// a public ID is optional. Returns `(public ID, system ID)`.
    fn scan_external_id(&mut self, public_id_ok: bool) -> Result<(Option<&'a str>, Option<&'a str>)> {
        if self.try_keyword(b"SYSTEM") {
            self.require_whitespace("after SYSTEM")?;
            return Ok((None, Some(self.scan_system_literal()?)));
        }
        if !self.try_keyword(b"PUBLIC") {
            return Err(self.eof_or_not_wf("declaration", "expected SYSTEM or PUBLIC"));
        }
        self.require_whitespace("after PUBLIC")?;
        let public_id = self.scan_pubid_literal()?;
        let had_space = self.skip_whitespace();
        if matches!(self.current(), Some(b'"' | b'\'')) {
            if !had_space {
                return Err(self.not_wf("whitespace required between public and system literals"));
            }
            Ok((Some(public_id), Some(self.scan_system_literal()?)))
        } else if public_id_ok {
            Ok((Some(public_id), None))
        } else {
            Err(self.eof_or_not_wf("declaration", "expected system literal after public ID"))
        }
    }

    /// §2.3 [Production 11]: SystemLiteral ::= ('"' [^"]* '"') | ("'" [^']* "'")
    fn scan_system_literal(&mut self) -> Result<&'a str> {
        self.scan_quoted(|_| true, "system literal")
    }

    /// §2.3 [Productions 12, 13]:
    ///
    ///   PubidLiteral ::= '"' PubidChar* '"' | "'" (PubidChar - "'")* "'"
    ///   PubidChar    ::= #x20 | #xD | #xA | [a-zA-Z0-9] | [-'()+,./:=?;!*#@$_%]
    fn scan_pubid_literal(&mut self) -> Result<&'a str> {
        self.scan_quoted(|b| b.is_ascii_alphanumeric() || b" \r\n-'()+,./:=?;!*#@$_%".contains(&b),
                         "public ID literal")
    }

    /// Scan a quoted literal whose bytes satisfy `allowed`; returns the text
    /// between the quotes.
    fn scan_quoted(&mut self, allowed: impl Fn(u8) -> bool, context: &'static str) -> Result<&'a str> {
        let quote = match self.current() {
            Some(q @ (b'"' | b'\'')) => q,
            _ => return Err(self.eof_or_not_wf(context, "expected quoted literal")),
        };
        self.bump();
        let start = self.pos.byte_offset;
        loop {
            match self.current() {
                None => return Err(XmlError::UnexpectedEof { pos: self.pos, context }),
                Some(c) if c == quote => {
                    let value = self.slice(start);
                    self.bump();
                    return Ok(value);
                }
                Some(c) if !allowed(c) => return Err(self.not_wf(
                    &format!("character {:?} not allowed in {context}", c as char))),
                Some(_) => self.bump(),
            }
        }
    }

    // ─── small helpers ───────────────────────────────────────────────────

    fn not_wf(&self, reason: &str) -> XmlError {
        XmlError::NotWellFormed { pos: self.pos, reason: reason.into() }
    }

    /// `UnexpectedEof` at end of input, otherwise `NotWellFormed`.
    fn eof_or_not_wf(&self, context: &'static str, reason: &str) -> XmlError {
        if self.is_eof() {
            XmlError::UnexpectedEof { pos: self.pos, context }
        } else {
            self.not_wf(reason)
        }
    }

    fn require_whitespace(&mut self, context: &str) -> Result<()> {
        if self.skip_whitespace() {
            Ok(())
        } else {
            Err(self.eof_or_not_wf("DOCTYPE", &format!("whitespace required {context}")))
        }
    }

    fn expect_byte(&mut self, b: u8, reason: &str) -> Result<()> {
        if self.current() == Some(b) {
            self.bump();
            Ok(())
        } else {
            Err(self.eof_or_not_wf("DOCTYPE", reason))
        }
    }

    /// Advance past `n` ASCII bytes already matched by the caller.
    fn advance_ascii(&mut self, n: usize) {
        self.pos.byte_offset += n;
        self.pos.column += n as u32;
    }

    /// Whether a keyword (not a prefix of a longer name) starts here.
    fn at_keyword(&self, kw: &[u8]) -> bool {
        self.src[self.pos.byte_offset..].starts_with(kw)
            && !self.char_at(self.pos.byte_offset + kw.len())
                .is_some_and(|(c, _)| crate::chars::is_name_char(c, self.edition))
    }

    /// Consume an exact literal such as `#PCDATA` if it starts here.
    fn try_literal(&mut self, lit: &[u8]) -> bool {
        if self.src[self.pos.byte_offset..].starts_with(lit) {
            self.advance_ascii(lit.len());
            true
        } else {
            false
        }
    }
}
