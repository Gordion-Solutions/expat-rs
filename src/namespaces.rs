//! Namespace processing: W3C Namespaces in XML 1.0 (Third Edition).
//!
//! A layer over the parser's event stream, enabled with
//! `Parser::with_namespaces`. It tracks `xmlns` declarations as elements
//! open and close, resolves element and attribute prefixes to namespace
//! names, and enforces the namespace constraints:
//!
//! - element and attribute names are QNames (§3); PI targets, entity names
//!   and notation names contain no colon (§7);
//! - every prefix used is declared (§5, Prefix Declared);
//! - `xml` is bound only to its namespace and that namespace to no other
//!   prefix; `xmlns` is never declared and its namespace never bound
//!   (§3, Reserved Prefixes and Namespace Names);
//! - `xmlns:p=""` is not allowed (§3; it is legal only in Namespaces 1.1);
//! - no element has two attributes with the same local name and namespace
//!   name (§6.3, Attributes Unique).
//!
//! Namespace declarations are reported as `StartNamespace` / `EndNamespace`
//! events rather than as attributes, as libexpat does.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::chars::{is_name_char, is_name_start_char, Edition};
use crate::error::{Position, Result, XmlError};
use crate::event::{Attribute, Event};

pub(crate) const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
pub(crate) const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";

/// One element's declarations: (prefix, namespace name). `None` prefix is
/// the default namespace; `None` name undeclares it.
type Scope = Vec<(Option<String>, Option<String>)>;

pub(crate) struct Namespaces {
    /// Declarations made by each open element, innermost last.
    scopes: Vec<Scope>,
    /// Current binding stack for each prefix (innermost last), so a lookup
    /// doesn't have to walk every open scope. Keyed by prefix, with "" for
    /// the default namespace (a real prefix is never empty).
    bindings: HashMap<String, Vec<Option<String>>>,
    pub edition: Edition,
}

fn error(pos: Position, reason: String) -> XmlError {
    XmlError::NotWellFormed { pos, reason }
}

impl Namespaces {
    pub fn new(edition: Edition) -> Self {
        Self { scopes: Vec::new(), bindings: HashMap::new(), edition }
    }

    /// Namespace name bound to `prefix` (`None`: the default namespace).
    fn lookup(&self, prefix: Option<&str>) -> Option<&str> {
        if prefix == Some("xml") {
            return Some(XML_NS);
        }
        self.bindings
            .get(prefix.unwrap_or(""))
            .and_then(|stack| stack.last())
            .and_then(|uri| uri.as_deref())
    }

    /// §3: a QName is `NCName` or `NCName ':' NCName`. Returns the prefix.
    fn check_qname<'n>(&self, name: &'n str, what: &str, pos: Position) -> Result<Option<&'n str>> {
        let ncname = |s: &str| {
            let mut chars = s.chars();
            chars.next().is_some_and(|c| c != ':' && is_name_start_char(c, self.edition))
                && chars.all(|c| c != ':' && is_name_char(c, self.edition))
        };
        match name.split_once(':') {
            None if ncname(name) => Ok(None),
            Some((prefix, local)) if ncname(prefix) && ncname(local) => Ok(Some(prefix)),
            _ => Err(error(pos, format!("{what} name {name:?} is not a valid QName"))),
        }
    }

    /// Process one event: resolve names on element starts, pop scopes on
    /// element ends, and check the other names that may not contain a
    /// colon. Pushes the resulting event(s) onto `out`.
    pub fn process<'a>(&mut self, event: Event<'a>, pos: Position, out: &mut VecDeque<Event<'a>>) -> Result<()> {
        match event {
            Event::StartElement { name, attributes, .. } => self.start(name, attributes, pos, out),
            Event::EndElement(name) => {
                let scope = self.scopes.pop().unwrap_or_default();
                out.push_back(Event::EndElement(name));
                for (prefix, _) in scope.into_iter().rev() {
                    if let Some(stack) = self.bindings.get_mut(prefix.as_deref().unwrap_or("")) {
                        stack.pop();
                    }
                    out.push_back(Event::EndNamespace { prefix: prefix.map(Cow::Owned) });
                }
                Ok(())
            }
            Event::ProcessingInstruction { ref target, .. } if target.contains(':') => {
                Err(error(pos, format!("processing instruction target {target:?} contains a colon")))
            }
            Event::NotationDecl { ref name, .. } if name.contains(':') => {
                Err(error(pos, format!("notation name {name:?} contains a colon")))
            }
            Event::SkippedEntity(ref name) if name.contains(':') => {
                Err(error(pos, format!("entity name {name:?} contains a colon")))
            }
            other => {
                out.push_back(other);
                Ok(())
            }
        }
    }

    fn start<'a>(
        &mut self,
        name: Cow<'a, str>,
        attributes: Vec<Attribute<'a>>,
        pos: Position,
        out: &mut VecDeque<Event<'a>>,
    ) -> Result<()> {
        // Declarations first: they apply to the element's own name and
        // attributes (including declarations defaulted from the DTD).
        let mut scope: Scope = Vec::new();
        let is_decl = |a: &Attribute<'_>| a.name == "xmlns" || a.name.starts_with("xmlns:");
        let mut attributes = attributes;
        for a in attributes.iter().filter(|a| is_decl(a)) {
            let prefix = if a.name == "xmlns" {
                None
            } else {
                self.check_qname(&a.name, "attribute", pos)?;
                a.name.strip_prefix("xmlns:").map(str::to_string)
            };
            let uri = a.value.as_ref();
            match prefix.as_deref() {
                Some("xmlns") => return Err(error(pos, "the prefix 'xmlns' must not be declared".into())),
                Some("xml") if uri != XML_NS => {
                    return Err(error(pos, format!("the prefix 'xml' can only be bound to {XML_NS}")));
                }
                Some(p) if uri.is_empty() => {
                    return Err(error(pos, format!(
                        "prefix {p:?} cannot be undeclared in Namespaces 1.0 (xmlns:{p}=\"\")")));
                }
                p if uri == XML_NS && p != Some("xml") => {
                    return Err(error(pos, format!("{XML_NS} can only be bound to the prefix 'xml'")));
                }
                _ if uri == XMLNS_NS => {
                    return Err(error(pos, format!("{XMLNS_NS} must not be declared")));
                }
                _ => {}
            }
            let uri = (!uri.is_empty()).then(|| uri.to_string());
            out.push_back(Event::StartNamespace {
                prefix: prefix.clone().map(Cow::Owned),
                uri: uri.clone().map(Cow::Owned),
            });
            self.bindings.entry(prefix.clone().unwrap_or_default()).or_default().push(uri.clone());
            scope.push((prefix, uri));
        }
        if !scope.is_empty() {
            attributes.retain(|a| !is_decl(a));
        }
        self.scopes.push(scope);

        // The element: prefixed names need a declared prefix; unprefixed
        // ones take the default namespace.
        let prefix = self.check_qname(&name, "element", pos)?;
        let namespace = match prefix {
            Some(p) => Some(self.lookup(Some(p))
                .ok_or_else(|| error(pos, format!("element prefix {p:?} is not declared")))?),
            None => self.lookup(None),
        }
        .map(|uri| Cow::Owned(uri.to_string()));

        // Attributes: unprefixed ones are in no namespace.
        let mut expanded_names: HashSet<(String, String)> = HashSet::new();
        for a in attributes.iter_mut() {
            if let Some(p) = self.check_qname(&a.name, "attribute", pos)? {
                let uri = self.lookup(Some(p))
                    .ok_or_else(|| error(pos, format!("attribute prefix {p:?} is not declared")))?;
                a.namespace = Some(Cow::Owned(uri.to_string()));
            }
            // §6.3 Attributes Unique: same local name and namespace name.
            // (Unprefixed attributes are in no namespace and were already
            // checked for duplicate names.)
            if let Some(ns) = &a.namespace {
                if !expanded_names.insert((ns.to_string(), a.local_name().to_string())) {
                    return Err(error(pos, format!(
                        "attribute {:?} duplicates another attribute's local name and namespace", a.name)));
                }
            }
        }
        out.push_back(Event::StartElement { name, namespace, attributes });
        Ok(())
    }
}
