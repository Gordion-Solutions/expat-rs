//! Well-formedness checker — the parser layer.
//!
//! Consumes [`Token`]s from [`crate::lexer::Lexer`] and emits [`Event`]s after
//! enforcing the well-formedness constraints of W3C XML 1.0 §2.1:
//!
//! - exactly one root element (§2.1 #1)
//! - all start-tags have matching end-tags, properly nested (§2.1 #2, §3.1)
//! - the XML declaration, if present, is the very first thing (§2.8)
//! - the document type declaration appears at most once and only in the prolog
//! - attribute names are unique per element (§3.1)
//! - no character data appears outside the root element (§2.1)
//! - entity-reference resolution stays within the well-formedness contract
//!   (week 4+ — leases the work to the parser; the lexer surfaces references
//!   raw)

use crate::chars::Edition;
use crate::error::{Position, Result, XmlError};
use std::borrow::Cow;

use crate::event::{normalize_newlines, Attribute, Event};
use crate::lexer::Lexer;
use crate::token::Token;
use crate::entities::{AttDecl, AttlistTable, DtdDecl, EntityTable, ExpansionLimits};
use crate::expand::{Expander, ExternalLoader};
use crate::namespaces::Namespaces;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Phase {
    /// Before any element — XmlDecl, Doctype, comments, PIs allowed.
    Prolog,
    /// Inside or after the root element.
    Body,
    /// After the root element has closed — only comments/PIs/whitespace
    /// allowed (epilog, per §2.1).
    Epilog,
}

pub struct Parser<'a> {
    lexer: Lexer<'a>,
    stack: Vec<&'a str>,
    phase: Phase,
    saw_xmldecl: bool,
    saw_doctype: bool,
    /// If a previous `next_event` returned an `EndElement` for an empty-element
    /// tag like `<br/>`, the matching synthetic end is queued here.
    pending_end: Option<&'a str>,
    last_pos: Position,
    /// Entities declared by `<!ENTITY ...>` in the DOCTYPE internal subset.
    entities: EntityTable,
    /// Attribute types and defaults from `<!ATTLIST>` declarations.
    attlists: AttlistTable,
    edition: Edition,
    /// `standalone` from the XML declaration, if given.
    standalone: Option<bool>,
    /// Whether §4.1 WFC Entity Declared applies: no DTD, or only an
    /// internal subset without parameter-entity references, or
    /// standalone="yes". Otherwise an undeclared entity is a validity
    /// error, not a well-formedness one.
    entity_declared_wfc: bool,
    /// Caller-supplied loader for external parsed entities. `None` (the
    /// default) never reads anything outside the document.
    loader: Option<Box<ExternalLoader<'a>>>,
    /// Events from an expanded entity reference, delivered before the
    /// next token is read.
    queued: std::collections::VecDeque<Event<'a>>,
    /// Namespace processing, if enabled, and the events it has produced
    /// but not yet delivered.
    namespaces: Option<Namespaces>,
    ns_out: std::collections::VecDeque<Event<'a>>,
    expansion_limits: ExpansionLimits,
    /// Cumulative bytes of expanded entity content seen so far in this
    /// document. The per-reference budget alone doesn't catch the
    /// quadratic-blowup pattern (linear-size payload with N references to a
    /// single benign-looking entity); the cumulative budget does.
    expanded_bytes_total: usize,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            lexer: Lexer::new(src),
            stack: Vec::new(),
            phase: Phase::Prolog,
            saw_xmldecl: false,
            saw_doctype: false,
            pending_end: None,
            last_pos: Position::start(),
            entities: EntityTable::new(),
            attlists: AttlistTable::default(),
            edition: Edition::default(),
            standalone: None,
            entity_declared_wfc: true,
            loader: None,
            queued: std::collections::VecDeque::new(),
            namespaces: None,
            ns_out: std::collections::VecDeque::new(),
            expansion_limits: ExpansionLimits::default(),
            expanded_bytes_total: 0,
        }
    }

    /// Select the XML 1.0 edition whose Name rules apply. Default:
    /// [`Edition::Fifth`]; use [`Edition::Fourth`] for the stricter
    /// Appendix B character classes.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.lexer = self.lexer.with_edition(edition);
        self.edition = edition;
        if let Some(ns) = &mut self.namespaces {
            ns.edition = edition;
        }
        self
    }

    /// Enable namespace processing (W3C Namespaces in XML 1.0): element
    /// and attribute names are resolved to namespace names, `xmlns`
    /// declarations are reported as `StartNamespace` / `EndNamespace`
    /// events instead of attributes, and the namespace constraints are
    /// enforced. Off by default, as in libexpat.
    pub fn with_namespaces(mut self) -> Self {
        self.namespaces = Some(Namespaces::new(self.edition));
        self
    }

    /// Read external parsed entities with `load(system_id, public_id)`,
    /// which returns `Ok(Some(text))`, `Ok(None)` to leave the entity
    /// unread, or `Err(reason)` (reported as [`XmlError::ExternalEntity`]).
    ///
    /// Off by default: without a loader the parser never touches anything
    /// outside the input, which rules out XXE-style file disclosure. Only
    /// install a loader for trusted input, and resolve identifiers
    /// defensively.
    pub fn with_external_loader(
        mut self,
        load: impl FnMut(&str, Option<&str>) -> std::result::Result<Option<String>, String> + 'a,
    ) -> Self {
        self.loader = Some(Box::new(load));
        self
    }

    /// Configure entity-expansion limits — defaults are conservative and
    /// suitable for adversarial input. Lower for stricter mitigation.
    pub fn with_expansion_limits(mut self, limits: ExpansionLimits) -> Self {
        self.expansion_limits = limits;
        self
    }

    /// Produce the next event, or `Ok(None)` at end of well-formed input.
    pub fn next_event(&mut self) -> Result<Option<Event<'a>>> {
        loop {
            if let Some(e) = self.ns_out.pop_front() {
                return Ok(Some(e));
            }
            if self.namespaces.is_none() {
                return self.next_xml_event();
            }
            let Some(e) = self.next_xml_event()? else { return Ok(None) };
            let mut out = Vec::new();
            if let Some(ns) = self.namespaces.as_mut() {
                ns.process(e, self.last_pos, &mut out)?;
            }
            self.ns_out.extend(out);
        }
    }

    /// The next event from XML 1.0 processing, before namespace processing.
    fn next_xml_event(&mut self) -> Result<Option<Event<'a>>> {
        if let Some(e) = self.queued.pop_front() {
            return Ok(Some(e));
        }
        if let Some(name) = self.pending_end.take() {
            // Pop the matching push from the EmptyTag handler and advance to
            // Epilog if this closed the root.
            self.stack.pop();
            if self.stack.is_empty() {
                self.phase = Phase::Epilog;
            }
            return Ok(Some(Event::EndElement(Cow::Borrowed(name))));
        }
        loop {
            self.last_pos = self.lexer.position();
            let tok = match self.lexer.next_token()? {
                Some(t) => t,
                None    => return self.handle_eof(),
            };

            match self.handle(tok)? {
                Some(e) => return Ok(Some(e)),
                // Folded away, or expanded into queued events.
                None => if let Some(e) = self.queued.pop_front() {
                    return Ok(Some(e));
                },
            }
        }
    }

    fn expander(&mut self) -> Expander<'_, 'a> {
        Expander::new(
            &self.entities,
            &self.attlists,
            self.expansion_limits,
            self.edition,
            self.entity_declared_wfc,
            self.loader.as_deref_mut(),
            self.last_pos,
        )
    }

    /// Add the bytes one reference expanded to the document-wide total.
    /// The per-reference budget alone doesn't catch the quadratic-blowup
    /// pattern (many references to one benign-looking entity); this does.
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.expanded_bytes_total = self.expanded_bytes_total.saturating_add(bytes);
        if self.expanded_bytes_total > self.expansion_limits.max_expanded_bytes {
            return Err(XmlError::NotWellFormed {
                pos: self.last_pos,
                reason: format!(
                    "cumulative entity expansion exceeded {} bytes — \
                     possible quadratic-blowup payload",
                    self.expansion_limits.max_expanded_bytes),
            });
        }
        Ok(())
    }

    /// Event attributes for a start tag: normalised values (§3.3.3), entity
    /// references checked and budgeted, declared defaults added.
    fn attributes(&mut self, element: &str, attrs: Vec<crate::token::Attr<'a>>) -> Result<Vec<Attribute<'a>>> {
        let mut x = self.expander();
        let out = x.attributes(element, &attrs)?;
        let used = x.expanded;
        self.charge(used)?;
        Ok(out)
    }

    /// Act on the internal subset's declarations in document order (§5.1).
    fn process_dtd(&mut self) -> Result<()> {
        let dtd = self.lexer.take_dtd();
        let has_pe_refs = dtd.decls.iter().any(|d| matches!(d, DtdDecl::PeRef));
        self.entity_declared_wfc = self.standalone == Some(true)
            || (!dtd.external_subset && !has_pe_refs);
        // Parameter entities are not read yet, so per §5.1 entity and
        // attribute-list declarations after an unread one are not
        // processed. Other declarations still are.
        let mut after_unread_pe = false;
        for decl in dtd.decls {
            match decl {
                DtdDecl::PeRef => after_unread_pe = true,
                DtdDecl::Pi { target, body } => {
                    self.queued.push_back(Event::ProcessingInstruction {
                        target: Cow::Owned(target),
                        body: Cow::Owned(normalize_newlines(&body).into_owned()),
                    });
                }
                DtdDecl::Comment(body) => {
                    self.queued.push_back(Event::Comment(Cow::Owned(normalize_newlines(&body).into_owned())));
                }
                DtdDecl::Notation { name, public_id, system_id } => {
                    self.queued.push_back(Event::NotationDecl {
                        name: Cow::Owned(name),
                        public_id: public_id.map(Cow::Owned),
                        system_id: system_id.map(Cow::Owned),
                    });
                }
                _ if after_unread_pe => {}
                DtdDecl::Entity { ref name, .. } if self.namespaces.is_some() && name.contains(':') => {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: format!("entity name {name:?} contains a colon (Namespaces §7)"),
                    });
                }
                DtdDecl::Entity { name, parameter: false, def } => self.entities.declare_def(name, def),
                DtdDecl::Entity { parameter: true, .. } => {}
                DtdDecl::AttDef { element, name, cdata, default, pos } => {
                    // Defaults are normalised, and their references checked,
                    // at the point of declaration (Entity Declared requires
                    // declaration before use here).
                    let default = match default {
                        Some(raw) => {
                            let mut x = self.expander();
                            x.pos = pos;
                            Some(x.attr_value(&raw, cdata)?.into_owned())
                        }
                        None => None,
                    };
                    self.attlists.declare(element, AttDecl { name, cdata, default });
                }
            }
        }
        self.queued.push_back(Event::EndDoctype);
        Ok(())
    }

    fn handle_eof(&mut self) -> Result<Option<Event<'a>>> {
        if !self.stack.is_empty() {
            return Err(XmlError::NotWellFormed {
                pos: self.last_pos,
                reason: format!("unclosed element {:?}", self.stack.last().unwrap()),
            });
        }
        if self.phase == Phase::Prolog {
            return Err(XmlError::NotWellFormed {
                pos: self.last_pos,
                reason: "no root element".into(),
            });
        }
        Ok(None)
    }

    fn handle(&mut self, tok: Token<'a>) -> Result<Option<Event<'a>>> {
        match tok {
            Token::XmlDecl(d) => {
                if self.saw_xmldecl {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "duplicate XML declaration".into(),
                    });
                }
                if self.phase != Phase::Prolog || self.saw_doctype || !self.stack.is_empty() {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "XML declaration must be the first thing in the document".into(),
                    });
                }
                self.saw_xmldecl = true;
                self.standalone = d.standalone;
                Ok(Some(Event::XmlDecl(d)))
            }
            Token::Doctype { name, body } => {
                if self.saw_doctype {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "duplicate DOCTYPE declaration".into(),
                    });
                }
                if self.phase != Phase::Prolog {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "DOCTYPE must appear before the root element".into(),
                    });
                }
                self.saw_doctype = true;
                self.process_dtd()?;
                Ok(Some(Event::Doctype { name, body }))
            }
            Token::StartTag { name, attributes } => {
                if self.phase == Phase::Epilog {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "second root element not allowed".into(),
                    });
                }
                // QName check (Namespaces 1.0 §3) is opt-in via check_qname —
                // pure XML 1.0 allows multiple colons in Names.
                let attributes = self.attributes(name, attributes)?;
                self.phase = Phase::Body;
                self.stack.push(name);
                Ok(Some(Event::StartElement { name: Cow::Borrowed(name), namespace: None, attributes }))
            }
            Token::EmptyTag { name, attributes } => {
                if self.phase == Phase::Epilog {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "second root element not allowed".into(),
                    });
                }
                let attributes = self.attributes(name, attributes)?;
                // Push to mirror what a normal start tag does — the matching
                // pending_end will pop it on the next call.
                self.stack.push(name);
                self.pending_end = Some(name);
                self.phase = Phase::Body;
                Ok(Some(Event::StartElement { name: Cow::Borrowed(name), namespace: None, attributes }))
            }
            Token::EndTag(name) => {
                let top = self.stack.pop().ok_or_else(|| XmlError::NotWellFormed {
                    pos: self.last_pos,
                    reason: format!("unexpected end tag </{name}> with no matching start"),
                })?;
                if top != name {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: format!("end tag </{name}> does not match start tag <{top}>"),
                    });
                }
                if self.stack.is_empty() {
                    self.phase = Phase::Epilog;
                }
                Ok(Some(Event::EndElement(Cow::Borrowed(name))))
            }
            Token::Text(s) => {
                // Per §2.1: character data only inside the root element.
                if self.stack.is_empty() {
                    if !s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) {
                        return Err(XmlError::NotWellFormed {
                            pos: self.last_pos,
                            reason: "non-whitespace text outside the root element".into(),
                        });
                    }
                    // Whitespace in the prolog/epilog is silently absorbed.
                    return Ok(None);
                }
                Ok(Some(Event::Text(normalize_newlines(s))))
            }
            Token::CData(s) => {
                if self.stack.is_empty() {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "CDATA section outside the root element".into(),
                    });
                }
                Ok(Some(Event::CData(normalize_newlines(s))))
            }
            Token::Comment(s) => Ok(Some(Event::Comment(normalize_newlines(s)))),
            Token::ProcessingInstruction { target, body } => {
                Ok(Some(Event::ProcessingInstruction { target: Cow::Borrowed(target), body: normalize_newlines(body) }))
            }
            // Entity references: built-in (always available) or DTD-declared.
            // Either is validated for well-formedness; expansion size is
            // capped to defend against billion-laughs / quadratic-blowup.
            Token::EntityRef(name) => {
                if self.stack.is_empty() {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "entity reference outside the root element".into(),
                    });
                }
                if let Some(text) = crate::entities::builtin_entity(name) {
                    return Ok(Some(Event::Text(Cow::Borrowed(text))));
                }
                // Replacement text is checked, budgeted, and its events
                // queued in place of the reference.
                let mut x = self.expander();
                x.check_content_entity(name)?;
                let (used, events) = (x.expanded, std::mem::take(&mut x.events));
                self.charge(used)?;
                self.queued.extend(events);
                Ok(None)
            }
            Token::CharRef(c) => {
                if self.stack.is_empty() {
                    return Err(XmlError::NotWellFormed {
                        pos: self.last_pos,
                        reason: "character reference outside the root element".into(),
                    });
                }
                Ok(Some(Event::Text(Cow::Owned(c.to_string()))))
            }
        }
    }

    /// Per W3C XML Namespaces 1.0 §3 (Qualified Names): a QName has at most
    /// one colon, with non-empty prefix and non-empty local name. Names
    /// without a colon are unprefixed and always valid here.
    ///
    /// Note: pure XML 1.0 (without Namespaces) allows multiple colons in
    /// Names. This check is therefore not applied unconditionally — it's
    /// reserved for callers that have opted into namespace processing
    /// (a future `Parser::namespace_aware()` mode).
    #[allow(dead_code)]
    fn check_qname(name: &str, pos: Position) -> Result<()> {
        let mut parts = name.split(':');
        let first  = parts.next().unwrap_or("");
        let second = parts.next();
        let third  = parts.next();
        if third.is_some() {
            return Err(XmlError::NotWellFormed {
                pos, reason: format!("QName {name:?} has more than one colon"),
            });
        }
        if let Some(local) = second {
            if first.is_empty() || local.is_empty() {
                return Err(XmlError::NotWellFormed {
                    pos, reason: format!("QName {name:?} has empty prefix or local part"),
                });
            }
        }
        Ok(())
    }
}

/// Per §3.1 (Unique Att Spec): no element may have two attributes with the
/// same name.
pub(crate) fn check_unique_attrs(attrs: &[crate::token::Attr<'_>], pos: Position) -> Result<()> {
    for (i, a) in attrs.iter().enumerate() {
        for b in &attrs[..i] {
            if a.name == b.name {
                return Err(XmlError::NotWellFormed {
                    pos,
                    reason: format!("duplicate attribute {:?}", a.name),
                });
            }
        }
    }
    Ok(())
}

// Built-in entities are defined in `crate::entities::builtin_entity`.
