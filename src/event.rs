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

    /// Element start (or empty-element). Per §3.
    StartElement { name: Cow<'a, str>, attributes: Vec<Attribute<'a>> },

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

    /// A reference in content to an entity whose text was not read: an
    /// external entity with no loader installed, or an undeclared entity
    /// where that is only a validity error. Mirrors libexpat's
    /// `XML_SetSkippedEntityHandler`.
    SkippedEntity(Cow<'a, str>),
}

/// An attribute on a start tag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute<'a> {
    pub name: Cow<'a, str>,
    /// The normalised value (§3.3.3).
    pub value: Cow<'a, str>,
    /// `false` for a value supplied by an attribute-list default in the DTD.
    pub specified: bool,
}

/// Normalise line endings per §2.11: `\r\n` and a lone `\r` become `\n`.
pub(crate) fn normalize_newlines(s: &str) -> Cow<'_, str> {
    if !s.contains('\r') {
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.replace("\r\n", "\n").replace('\r', "\n"))
}
