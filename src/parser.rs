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
//! - entity references expand to well-formed content (see `crate::expand`)
//!
//! Everything that must survive from one token to the next lives in
//! [`State`], which borrows nothing from the input. That lets the same
//! state drive both [`Parser`] (one `&str`) and
//! [`crate::stream::StreamParser`] (input arriving in chunks).

use std::borrow::Cow;
use std::collections::VecDeque;

use crate::chars::Edition;
use crate::entities::{AttDecl, AttlistTable, Dtd, DtdDecl, EntityTable, ExpansionLimits};
use crate::error::{Position, Result, XmlError};
use crate::event::{normalize_newlines, Attribute, Event};
use crate::expand::{Expander, ExternalLoader};
use crate::lexer::Lexer;
use crate::namespaces::Namespaces;
use crate::token::Token;

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

/// Names of the open elements, innermost last, stored in one buffer.
#[derive(Default)]
struct NameStack {
    names: String,
    ends: Vec<usize>,
}

impl NameStack {
    fn push(&mut self, name: &str) {
        self.names.push_str(name);
        self.ends.push(self.names.len());
    }

    fn top(&self) -> Option<&str> {
        let end = *self.ends.last()?;
        let start = self.ends.len().checked_sub(2).map_or(0, |i| self.ends[i]);
        Some(&self.names[start..end])
    }

    fn pop(&mut self) {
        self.ends.pop();
        self.names.truncate(self.ends.last().copied().unwrap_or(0));
    }

    fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }
}

/// Parser state that outlives any one token or piece of input.
pub(crate) struct State<'l> {
    stack: NameStack,
    phase: Phase,
    saw_xmldecl: bool,
    saw_doctype: bool,
    /// Where the token being handled starts, for error reports.
    pub last_pos: Position,
    /// Entities declared by `<!ENTITY ...>` in the DOCTYPE internal subset.
    entities: EntityTable,
    /// Attribute types and defaults from `<!ATTLIST>` declarations.
    attlists: AttlistTable,
    pub edition: Edition,
    /// `standalone` from the XML declaration, if given.
    standalone: Option<bool>,
    /// Whether §4.1 WFC Entity Declared applies: no DTD, or only an
    /// internal subset without parameter-entity references, or
    /// standalone="yes". Otherwise an undeclared entity is a validity
    /// error, not a well-formedness one.
    entity_declared_wfc: bool,
    /// Caller-supplied loader for external parsed entities. `None` (the
    /// default) never reads anything outside the document.
    pub loader: Option<Box<ExternalLoader<'l>>>,
    /// Namespace processing, if enabled.
    pub namespaces: Option<Namespaces>,
    pub expansion_limits: ExpansionLimits,
    /// Cumulative bytes of expanded entity content seen so far in this
    /// document. The per-reference budget alone doesn't catch the
    /// quadratic-blowup pattern (linear-size payload with N references to a
    /// single benign-looking entity); the cumulative budget does.
    expanded_bytes_total: usize,
}

impl<'l> State<'l> {
    pub fn new() -> Self {
        Self {
            stack: NameStack::default(),
            phase: Phase::Prolog,
            saw_xmldecl: false,
            saw_doctype: false,
            last_pos: Position::start(),
            entities: EntityTable::new(),
            attlists: AttlistTable::default(),
            edition: Edition::default(),
            standalone: None,
            entity_declared_wfc: true,
            loader: None,
            namespaces: None,
            expansion_limits: ExpansionLimits::default(),
            expanded_bytes_total: 0,
        }
    }

    pub fn set_edition(&mut self, edition: Edition) {
        self.edition = edition;
        if let Some(ns) = &mut self.namespaces {
            ns.edition = edition;
        }
    }

    pub fn enable_namespaces(&mut self) {
        self.namespaces = Some(Namespaces::new(self.edition));
    }

    fn not_wf(&self, reason: impl Into<String>) -> XmlError {
        XmlError::NotWellFormed { pos: self.last_pos, reason: reason.into() }
    }

    fn expander(&mut self) -> Expander<'_, 'l> {
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

    /// Deliver an event, through namespace processing if it is enabled.
    fn emit<'t>(&mut self, event: Event<'t>, out: &mut VecDeque<Event<'t>>) -> Result<()> {
        match &mut self.namespaces {
            Some(ns) => {
                let mut resolved = Vec::new();
                ns.process(event, self.last_pos, &mut resolved)?;
                out.extend(resolved);
            }
            None => out.push_back(event),
        }
        Ok(())
    }

    /// Add the bytes one reference expanded to the document-wide total.
    /// The per-reference budget alone doesn't catch the quadratic-blowup
    /// pattern (many references to one benign-looking entity); this does.
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.expanded_bytes_total = self.expanded_bytes_total.saturating_add(bytes);
        if self.expanded_bytes_total > self.expansion_limits.max_expanded_bytes {
            return Err(self.not_wf(format!(
                "cumulative entity expansion exceeded {} bytes — \
                 possible quadratic-blowup payload",
                self.expansion_limits.max_expanded_bytes)));
        }
        Ok(())
    }

    /// Event attributes for a start tag: normalised values (§3.3.3), entity
    /// references checked and budgeted, declared defaults added.
    fn attributes<'t>(&mut self, element: &str, attrs: &[crate::token::Attr<'t>]) -> Result<Vec<Attribute<'t>>> {
        let mut x = self.expander();
        let out = x.attributes(element, attrs)?;
        let used = x.expanded;
        self.charge(used)?;
        Ok(out)
    }

    /// Act on the internal subset's declarations in document order (§5.1),
    /// delivering the events for what it contained.
    fn process_dtd<'t>(&mut self, dtd: Dtd, out: &mut VecDeque<Event<'t>>) -> Result<()> {
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
                    let body = normalize_newlines(&body).into_owned();
                    self.emit(Event::ProcessingInstruction { target: Cow::Owned(target), body: Cow::Owned(body) }, out)?;
                }
                DtdDecl::Comment(body) => {
                    self.emit(Event::Comment(Cow::Owned(normalize_newlines(&body).into_owned())), out)?;
                }
                DtdDecl::Notation { name, public_id, system_id } => {
                    self.emit(Event::NotationDecl {
                        name: Cow::Owned(name),
                        public_id: public_id.map(Cow::Owned),
                        system_id: system_id.map(Cow::Owned),
                    }, out)?;
                }
                _ if after_unread_pe => {}
                DtdDecl::Entity { ref name, .. } if self.namespaces.is_some() && name.contains(':') => {
                    return Err(self.not_wf(format!("entity name {name:?} contains a colon (Namespaces §7)")));
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
        self.emit(Event::EndDoctype, out)
    }

    /// End of input: everything opened must have closed.
    pub fn finish(&self) -> Result<()> {
        if let Some(open) = self.stack.top() {
            return Err(self.not_wf(format!("unclosed element {open:?}")));
        }
        if self.phase == Phase::Prolog {
            return Err(self.not_wf("no root element"));
        }
        Ok(())
    }

    fn start_element_allowed(&self) -> Result<()> {
        if self.phase == Phase::Epilog {
            return Err(self.not_wf("second root element not allowed"));
        }
        Ok(())
    }

    fn close_element(&mut self) {
        self.stack.pop();
        if self.stack.is_empty() {
            self.phase = Phase::Epilog;
        }
    }

    /// Handle one token, delivering its events to `out`. `dtd` holds the
    /// declarations the lexer recorded while tokenising a DOCTYPE token.
    pub fn handle<'t>(&mut self, tok: Token<'t>, dtd: Dtd, out: &mut VecDeque<Event<'t>>) -> Result<()> {
        match tok {
            Token::XmlDecl(d) => {
                if self.saw_xmldecl {
                    return Err(self.not_wf("duplicate XML declaration"));
                }
                if self.phase != Phase::Prolog || self.saw_doctype || !self.stack.is_empty() {
                    return Err(self.not_wf("XML declaration must be the first thing in the document"));
                }
                self.saw_xmldecl = true;
                self.standalone = d.standalone;
                self.emit(Event::XmlDecl(d), out)
            }
            Token::Doctype { name, body } => {
                if self.saw_doctype {
                    return Err(self.not_wf("duplicate DOCTYPE declaration"));
                }
                if self.phase != Phase::Prolog {
                    return Err(self.not_wf("DOCTYPE must appear before the root element"));
                }
                self.saw_doctype = true;
                self.emit(Event::Doctype { name, body }, out)?;
                self.process_dtd(dtd, out)
            }
            Token::StartTag { name, attributes } => {
                self.start_element_allowed()?;
                let attributes = self.attributes(name, &attributes)?;
                self.phase = Phase::Body;
                self.stack.push(name);
                self.emit(Event::StartElement { name: Cow::Borrowed(name), namespace: None, attributes }, out)
            }
            Token::EmptyTag { name, attributes } => {
                self.start_element_allowed()?;
                let attributes = self.attributes(name, &attributes)?;
                self.phase = Phase::Body;
                self.stack.push(name);
                self.emit(Event::StartElement { name: Cow::Borrowed(name), namespace: None, attributes }, out)?;
                self.close_element();
                self.emit(Event::EndElement(Cow::Borrowed(name)), out)
            }
            Token::EndTag(name) => {
                match self.stack.top() {
                    None => return Err(self.not_wf(format!("unexpected end tag </{name}> with no matching start"))),
                    Some(top) if top != name => {
                        return Err(self.not_wf(format!("end tag </{name}> does not match start tag <{top}>")));
                    }
                    Some(_) => {}
                }
                self.close_element();
                self.emit(Event::EndElement(Cow::Borrowed(name)), out)
            }
            Token::Text(s) => {
                // Per §2.1: character data only inside the root element.
                if self.stack.is_empty() {
                    if let Some(i) = s.find(|c: char| !matches!(c, ' ' | '\t' | '\r' | '\n')) {
                        // Report the first character that isn't whitespace.
                        self.last_pos = self.last_pos.advance(&s[..i]);
                        return Err(self.not_wf("non-whitespace text outside the root element"));
                    }
                    // Whitespace in the prolog/epilog is silently absorbed.
                    return Ok(());
                }
                self.emit(Event::Text(normalize_newlines(s)), out)
            }
            Token::CData(s) => {
                if self.stack.is_empty() {
                    return Err(self.not_wf("CDATA section outside the root element"));
                }
                self.emit(Event::CData(normalize_newlines(s)), out)
            }
            Token::Comment(s) => self.emit(Event::Comment(normalize_newlines(s)), out),
            Token::ProcessingInstruction { target, body } => {
                self.emit(Event::ProcessingInstruction { target: Cow::Borrowed(target), body: normalize_newlines(body) }, out)
            }
            // Entity references: built-in (always available) or DTD-declared.
            // Replacement text is checked, budgeted against billion-laughs /
            // quadratic-blowup, and its events delivered in place.
            Token::EntityRef(name) => {
                if self.stack.is_empty() {
                    return Err(self.not_wf("entity reference outside the root element"));
                }
                if let Some(text) = crate::entities::builtin_entity(name) {
                    return self.emit(Event::Text(Cow::Borrowed(text)), out);
                }
                let mut x = self.expander();
                x.check_content_entity(name)?;
                let (used, events) = (x.expanded, std::mem::take(&mut x.events));
                self.charge(used)?;
                events.into_iter().try_for_each(|e| self.emit(e, out))
            }
            Token::CharRef(c) => {
                if self.stack.is_empty() {
                    return Err(self.not_wf("character reference outside the root element"));
                }
                self.emit(Event::Text(Cow::Owned(c.to_string())), out)
            }
        }
    }
}

/// Pull parser over a whole document held in one `&str`.
pub struct Parser<'a> {
    lexer: Lexer<'a>,
    state: State<'a>,
    /// Events produced but not yet returned by `next_event`.
    out: VecDeque<Event<'a>>,
    done: bool,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { lexer: Lexer::new(src), state: State::new(), out: VecDeque::new(), done: false }
    }

    /// Select the XML 1.0 edition whose Name rules apply. Default:
    /// [`Edition::Fifth`]; use [`Edition::Fourth`] for the stricter
    /// Appendix B character classes.
    pub fn with_edition(mut self, edition: Edition) -> Self {
        self.lexer = self.lexer.with_edition(edition);
        self.state.set_edition(edition);
        self
    }

    /// Enable namespace processing (W3C Namespaces in XML 1.0): element
    /// and attribute names are resolved to namespace names, `xmlns`
    /// declarations are reported as `StartNamespace` / `EndNamespace`
    /// events instead of attributes, and the namespace constraints are
    /// enforced. Off by default, as in libexpat.
    pub fn with_namespaces(mut self) -> Self {
        self.state.enable_namespaces();
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
        self.state.loader = Some(Box::new(load));
        self
    }

    /// Configure entity-expansion limits — defaults are conservative and
    /// suitable for adversarial input. Lower for stricter mitigation.
    pub fn with_expansion_limits(mut self, limits: ExpansionLimits) -> Self {
        self.state.expansion_limits = limits;
        self
    }

    /// Produce the next event, or `Ok(None)` at end of well-formed input.
    pub fn next_event(&mut self) -> Result<Option<Event<'a>>> {
        loop {
            if let Some(e) = self.out.pop_front() {
                return Ok(Some(e));
            }
            if self.done {
                return Ok(None);
            }
            self.state.last_pos = self.lexer.position();
            match self.lexer.next_token()? {
                Some(tok) => {
                    let dtd = self.lexer.take_dtd();
                    self.state.handle(tok, dtd, &mut self.out)?;
                }
                None => {
                    self.state.finish()?;
                    self.done = true;
                }
            }
        }
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
