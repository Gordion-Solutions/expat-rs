//! Entity expansion checks: what an entity reference brings in must itself
//! be well-formed where it lands.
//!
//! Per W3C XML 1.0 (Fifth Edition) §4.1, §4.3.2 and §4.4:
//!
//! - In content, an entity's replacement text must match `content`: its
//!   tags balance within the entity, and it contains no XML or text
//!   declaration (internal entities). Unparsed entities may not be
//!   referenced (WFC Parsed Entity). External parsed entities are read only
//!   if the caller supplied a loader; otherwise they are "not read", which
//!   §4.4.3 permits a non-validating processor.
//! - In attribute values, replacement text may not contain `<` (WFC No < in
//!   Attribute Values), and may not come from an external entity (WFC No
//!   External Entity References).
//! - Everywhere: no entity may refer to itself, directly or indirectly (WFC
//!   No Recursion), and an undeclared entity is an error when the Entity
//!   Declared WFC applies.
//!
//! Expansion is bounded by `ExpansionLimits` (depth and bytes) against
//! billion-laughs and quadratic-blowup payloads.

use std::borrow::Cow;

use crate::chars::{decode_char_ref, is_name_char, is_name_start_char, Edition};
use crate::entities::{builtin_entity, AttlistTable, EntityDef, EntityTable, ExpansionLimits};
use crate::error::{Position, Result, XmlError};
use crate::lexer::Lexer;
use crate::event::{normalize_newlines, Attribute, Event};
use crate::token::{Attr, Token};

fn owned(s: &str) -> Cow<'static, str> {
    Cow::Owned(s.to_string())
}

/// Loads the text of an external parsed entity from its system and public
/// identifiers: `Ok(Some(text))`, `Ok(None)` to leave it unread, or
/// `Err(reason)` if it should be read but can't be.
pub type ExternalLoader<'l> =
    dyn FnMut(&str, Option<&str>) -> std::result::Result<Option<String>, String> + 'l;

pub(crate) struct Expander<'e, 'l> {
    pub table: &'e EntityTable,
    pub attlists: &'e AttlistTable,
    pub limits: ExpansionLimits,
    pub edition: Edition,
    /// Whether §4.1 WFC Entity Declared applies to this document.
    pub entity_declared_wfc: bool,
    pub loader: Option<&'e mut ExternalLoader<'l>>,
    /// Position of the reference being checked, for error reports.
    pub pos: Position,
    /// Bytes of replacement text expanded so far.
    pub expanded: usize,
    /// Entities currently being expanded, outermost first.
    stack: Vec<String>,
    /// Events produced by expanding content entities, in document order.
    pub events: Vec<Event<'static>>,
}

impl<'e, 'l> Expander<'e, 'l> {
    pub fn new(
        table: &'e EntityTable,
        attlists: &'e AttlistTable,
        limits: ExpansionLimits,
        edition: Edition,
        entity_declared_wfc: bool,
        loader: Option<&'e mut ExternalLoader<'l>>,
        pos: Position,
    ) -> Self {
        Self {
            table, attlists, limits, edition, entity_declared_wfc, loader, pos,
            expanded: 0, stack: Vec::new(), events: Vec::new(),
        }
    }

    fn error(&self, reason: String) -> XmlError {
        XmlError::NotWellFormed { pos: self.pos, reason }
    }

    /// Event attributes for a start tag of `element`: values normalised
    /// (§3.3.3) with their entity references checked, then any declared
    /// defaults the tag doesn't specify.
    pub fn attributes<'x>(&mut self, element: &str, attrs: &[Attr<'x>]) -> Result<Vec<Attribute<'x>>> {
        crate::parser::check_unique_attrs(attrs, self.pos)?;
        let declared = self.attlists.get(element);
        let mut out = Vec::with_capacity(attrs.len());
        for a in attrs {
            let cdata = declared.and_then(|d| d.get(a.name)).is_none_or(|d| d.cdata);
            let value = self.attr_value(a.value, cdata)?;
            out.push(Attribute { name: Cow::Borrowed(a.name), namespace: None, value, specified: true });
        }
        if let Some(declared) = declared {
            let specified: std::collections::HashSet<&str> = attrs.iter().map(|a| a.name).collect();
            for d in &declared.decls {
                if let Some(default) = &d.default {
                    if !specified.contains(d.name.as_str()) {
                        out.push(Attribute {
                            name: Cow::Owned(d.name.clone()),
                            namespace: None,
                            value: Cow::Owned(default.clone()),
                            specified: false,
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    /// Normalise an attribute value as written (§3.3.3), checking the
    /// entities it references. `cdata` is false for declared types other
    /// than CDATA, which also trim and collapse spaces.
    pub fn attr_value<'x>(&mut self, raw: &'x str, cdata: bool) -> Result<Cow<'x, str>> {
        let unchanged = !raw.contains(['&', '\t', '\n', '\r'])
            && (cdata || (!raw.starts_with(' ') && !raw.ends_with(' ') && !raw.contains("  ")));
        if unchanged {
            return Ok(Cow::Borrowed(raw));
        }
        let mut out = String::with_capacity(raw.len());
        self.normalize_into(&normalize_newlines(raw), &mut out, None)?;
        if !cdata {
            out = out.split(' ').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        }
        Ok(Cow::Owned(out))
    }

    /// §3.3.3 step 2 over `text`: character references append their
    /// character, entity references are processed recursively, and each
    /// whitespace character becomes a space. `entity` names the entity
    /// whose replacement text this is, for error messages.
    fn normalize_into(&mut self, text: &str, out: &mut String, entity: Option<&str>) -> Result<()> {
        let push_text = |out: &mut String, s: &str| {
            out.extend(s.chars().map(|c| if matches!(c, '\t' | '\n' | '\r') { ' ' } else { c }));
        };
        let mut rest = text;
        while let Some(i) = rest.find('&') {
            push_text(out, &rest[..i]);
            let after = &rest[i + 1..];
            let in_entity = |reason: String| match entity {
                Some(name) => format!("in entity {name:?}: {reason}"),
                None => reason,
            };
            let Some(end) = after.find(';') else {
                return Err(self.error(in_entity("'&' does not start a reference".into())));
            };
            let body = &after[..end];
            if let Some(num) = body.strip_prefix('#') {
                out.push(decode_char_ref(num).map_err(|r| self.error(in_entity(r)))?);
            } else if is_name(body, self.edition) {
                self.attr_entity_into(body, out)?;
            } else {
                return Err(self.error(in_entity(format!("'&{body};' is not a well-formed reference"))));
            }
            rest = &after[end + 1..];
        }
        push_text(out, rest);
        Ok(())
    }

    fn attr_entity_into(&mut self, name: &str, out: &mut String) -> Result<()> {
        if let Some(text) = builtin_entity(name) {
            out.push_str(text);
            return Ok(());
        }
        let text = match self.table.get(name) {
            None => return self.undeclared(name),
            Some(EntityDef::External { unparsed: true, .. }) => {
                return Err(self.error(format!("reference to unparsed entity {name:?} in attribute value")));
            }
            Some(EntityDef::External { .. }) => {
                return Err(self.error(format!(
                    "reference to external entity {name:?} in attribute value (No External Entity References)")));
            }
            Some(EntityDef::Internal(text)) => text,
        };
        self.enter(name, text.len())?;
        if text.contains('<') {
            return Err(self.error(format!("entity {name:?} puts '<' in an attribute value")));
        }
        // Replacement text is parsed again (§4.4.5).
        self.normalize_into(text, out, Some(name))?;
        self.leave();
        Ok(())
    }

    /// Check an entity reference that appears in content, collecting the
    /// events its replacement text produces into `self.events`.
    pub fn check_content_entity(&mut self, name: &str) -> Result<()> {
        if let Some(text) = builtin_entity(name) {
            self.events.push(Event::Text(Cow::Borrowed(text)));
            return Ok(());
        }
        match self.table.get(name) {
            None => {
                self.undeclared(name)?;
                self.events.push(Event::SkippedEntity(Cow::Owned(name.to_string())));
                Ok(())
            }
            Some(EntityDef::External { unparsed: true, .. }) => {
                Err(self.error(format!("reference to unparsed entity {name:?} in content (Parsed Entity)")))
            }
            Some(EntityDef::External { system_id, public_id, .. }) => {
                let loaded = match self.loader.as_mut() {
                    Some(load) => load(system_id, public_id.as_deref()).map_err(|reason| {
                        XmlError::ExternalEntity { pos: self.pos, reason: format!("{name:?} ({system_id}): {reason}") }
                    })?,
                    None => None,
                };
                match loaded {
                    Some(text) => {
                        // §2.11 applies to the entity's input text.
                        let text = normalize_newlines(&text);
                        self.enter(name, text.len())?;
                        self.check_content_text(name, &text, true)?;
                        self.leave();
                        Ok(())
                    }
                    None => {
                        // Not read (§4.4.3).
                        self.events.push(Event::SkippedEntity(Cow::Owned(name.to_string())));
                        Ok(())
                    }
                }
            }
            Some(EntityDef::Internal(text)) => {
                self.enter(name, text.len())?;
                self.check_content_text(name, text, false)?;
                self.leave();
                Ok(())
            }
        }
    }

    /// Replacement text must match `content` (§4.3.2): tokenise it on its
    /// own, with its own tag stack, and recurse into references. Line
    /// endings in `text` are already normalised, and any CR left in it came
    /// from a character reference, so token text is used as is.
    fn check_content_text(&mut self, name: &str, text: &str, external: bool) -> Result<()> {
        let pos = self.pos;
        let in_entity = |e: XmlError| XmlError::NotWellFormed {
            pos,
            reason: format!("in replacement text of entity {name:?}: {e}"),
        };
        let mut lexer = Lexer::new(text).with_edition(self.edition);
        if external {
            lexer.skip_text_decl().map_err(in_entity)?;
        }
        let mut open: Vec<&str> = Vec::new();
        while let Some(tok) = lexer.next_token().map_err(in_entity)? {
            match tok {
                Token::StartTag { name: tag, attributes } => {
                    let attributes = self.tag_attributes(tag, &attributes)?;
                    self.events.push(Event::StartElement { name: owned(tag), namespace: None, attributes });
                    open.push(tag);
                }
                Token::EmptyTag { name: tag, attributes } => {
                    let attributes = self.tag_attributes(tag, &attributes)?;
                    self.events.push(Event::StartElement { name: owned(tag), namespace: None, attributes });
                    self.events.push(Event::EndElement(owned(tag)));
                }
                Token::EndTag(tag) => match open.pop() {
                    Some(start) if start == tag => self.events.push(Event::EndElement(owned(tag))),
                    _ => return Err(self.error(format!(
                        "end tag </{tag}> in entity {name:?} has no matching start tag in the same entity"))),
                },
                Token::EntityRef(inner) => self.check_content_entity(inner)?,
                Token::CharRef(c) => self.events.push(Event::Text(Cow::Owned(c.to_string()))),
                Token::Text(t) => self.events.push(Event::Text(owned(t))),
                Token::CData(t) => self.events.push(Event::CData(owned(t))),
                Token::Comment(t) => self.events.push(Event::Comment(owned(t))),
                Token::ProcessingInstruction { target, body } => {
                    self.events.push(Event::ProcessingInstruction { target: owned(target), body: owned(body) });
                }
                Token::XmlDecl(_) => return Err(self.error(format!(
                    "XML or text declaration not allowed in the replacement text of entity {name:?}"))),
                Token::Doctype { .. } => return Err(self.error(format!(
                    "DOCTYPE not allowed in the replacement text of entity {name:?}"))),
            }
        }
        if let Some(tag) = open.last() {
            return Err(self.error(format!("element <{tag}> is not closed within entity {name:?}")));
        }
        Ok(())
    }

    /// Event attributes for a start tag inside an entity, owned.
    fn tag_attributes(&mut self, element: &str, attributes: &[Attr<'_>]) -> Result<Vec<Attribute<'static>>> {
        Ok(self.attributes(element, attributes)?
            .into_iter()
            .map(|a| Attribute { name: owned(&a.name), namespace: None, value: owned(&a.value), specified: a.specified })
            .collect())
    }

    fn undeclared(&self, name: &str) -> Result<()> {
        if self.entity_declared_wfc {
            Err(self.error(format!("undeclared entity {name:?}")))
        } else {
            Ok(()) // a validity error only (VC Entity Declared)
        }
    }

    /// Push `name` onto the expansion stack, enforcing No Recursion and the
    /// depth and byte limits.
    fn enter(&mut self, name: &str, len: usize) -> Result<()> {
        if self.stack.iter().any(|n| n == name) {
            return Err(self.error(format!("entity {name:?} refers to itself (No Recursion)")));
        }
        if self.stack.len() >= self.limits.max_depth {
            return Err(self.error(format!(
                "entity expansion of {name:?} exceeded the maximum nesting depth — \
                 possible billion-laughs / quadratic-blowup payload")));
        }
        self.expanded = self.expanded.saturating_add(len);
        if self.expanded > self.limits.max_expanded_bytes {
            return Err(self.error(format!(
                "entity expansion exceeded {} bytes — possible billion-laughs / \
                 quadratic-blowup payload", self.limits.max_expanded_bytes)));
        }
        self.stack.push(name.to_string());
        Ok(())
    }

    fn leave(&mut self) {
        self.stack.pop();
    }
}

/// §2.3 [Production 5]: Name, under `edition`. Replacement text can contain
/// references the lexer never saw (e.g. `&#38;name;` becomes `&name;`), so
/// names are checked again here.
fn is_name(s: &str, edition: Edition) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| is_name_start_char(c, edition))
        && chars.all(|c| is_name_char(c, edition))
}
