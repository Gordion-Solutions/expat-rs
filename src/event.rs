//! Parser events — the layer above tokens.
//!
//! Where `Token` is a lexical unit, `Event` is a semantic unit emitted by the
//! [`crate::parser::Parser`] after applying well-formedness rules. This is the
//! shape callers actually consume — equivalent to libexpat's
//! `XML_StartElementHandler` / `XML_EndElementHandler` callbacks.
//!
//! Text is borrowed from the input where it can be used as written, and
//! owned where the parser had to change it: line endings normalised
//! (§2.11), references replaced by what they stand for, attribute values
//! normalised (§3.3.3), or content brought in by an entity.

use std::borrow::Cow;

use crate::token::XmlDecl;

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// `<?xml version="..." ...?>`. May appear at most once, as the first
    /// event in the stream. Per W3C XML 1.0 §2.8.
    XmlDecl(XmlDecl<'a>),

    /// `<!DOCTYPE name ...>`. May appear at most once, before the root
    /// element. Per §2.8 [Production 28].
    Doctype { name: &'a str, body: &'a str },

    /// Element start (or empty-element). Per §3. `name` is the name as
    /// written (a QName when namespaces are on); `namespace` is its
    /// namespace name, set only when namespace processing is enabled.
    StartElement { name: Cow<'a, str>, namespace: Option<Cow<'a, str>>, attributes: Vec<Attribute<'a>> },

    /// Element end. For empty-element tags (`<x/>`), the parser emits both
    /// a `StartElement` and an `EndElement` so callers see a uniform stream.
    EndElement(Cow<'a, str>),

    /// Character data per §2.4, with line endings normalised and
    /// references replaced. May arrive in several consecutive pieces.
    Text(Cow<'a, str>),

    /// `<![CDATA[...]]>`. Surfaces separately from `Text` so callers that
    /// care about the original source representation can distinguish them.
    CData(Cow<'a, str>),

    /// `<!-- ... -->`. Per §2.5.
    Comment(Cow<'a, str>),

    /// `<?target body?>`, excluding the XML declaration. Per §2.6.
    ProcessingInstruction { target: Cow<'a, str>, body: Cow<'a, str> },

    /// End of the DOCTYPE. Events for what the internal subset contained
    /// (processing instructions, comments, notation declarations) come
    /// between `Doctype` and this, in document order. Mirrors libexpat's
    /// end-doctype handler.
    EndDoctype,

    /// A notation declaration from the DTD (§4.7), between `Doctype` and
    /// `EndDoctype`. Mirrors libexpat's `XML_SetNotationDeclHandler`.
    NotationDecl { name: Cow<'a, str>, public_id: Option<Cow<'a, str>>, system_id: Option<Cow<'a, str>> },

    /// A namespace declaration coming into scope, just before the
    /// `StartElement` that carries it (namespace processing only).
    /// `prefix` is `None` for the default namespace; `uri` is `None` when
    /// `xmlns=""` undeclares it. Mirrors libexpat's
    /// `XML_SetStartNamespaceDeclHandler`.
    StartNamespace { prefix: Option<Cow<'a, str>>, uri: Option<Cow<'a, str>> },

    /// A namespace declaration going out of scope, just after the matching
    /// `EndElement` (namespace processing only).
    EndNamespace { prefix: Option<Cow<'a, str>> },

    /// A reference in content to an entity whose text was not read: an
    /// external entity with no loader installed, or an undeclared entity
    /// where that is only a validity error. Mirrors libexpat's
    /// `XML_SetSkippedEntityHandler`.
    SkippedEntity(Cow<'a, str>),
}

/// An attribute on a start tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    /// The name as written (a QName when namespaces are on).
    pub name: Cow<'a, str>,
    /// Namespace name, when namespace processing is enabled and the
    /// attribute is prefixed. Unprefixed attributes are in no namespace.
    pub namespace: Option<Cow<'a, str>>,
    /// The normalised value (§3.3.3).
    pub value: Cow<'a, str>,
    /// `false` for a value supplied by an attribute-list default in the DTD.
    pub specified: bool,
}

impl Attribute<'_> {
    /// The part of the name after the prefix, if any.
    pub fn local_name(&self) -> &str {
        local_name(&self.name)
    }
}

/// The local part of a QName: everything after the first colon, or the
/// whole name if it has none.
pub fn local_name(qname: &str) -> &str {
    qname.split_once(':').map_or(qname, |(_, local)| local)
}

/// Normalise line endings per §2.11: `\r\n` and a lone `\r` become `\n`.
pub(crate) fn normalize_newlines(s: &str) -> Cow<'_, str> {
    if !s.contains('\r') {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
}
