//! XML token model.
//!
//! Per the W3C XML 1.0 Recommendation §2 (Documents), the lexical structure
//! of an XML document consists of element start/end tags, character data,
//! comments, processing instructions, CDATA sections, the XML and doctype
//! declarations, and references (entity, character).
//!
//! Each `Token` variant corresponds to one production rule from the spec.
//! Spec section is cited on each variant.

use crate::error::Position;

/// A token recognised by the lexer. Each variant carries a borrowed slice
/// of the input — no string allocation on the hot path.
///
/// Lifetime `'a` is the lifetime of the input buffer.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token<'a> {
    /// XML declaration: `<?xml version="1.0" ...?>`. Per §2.8 [Production 23].
    XmlDecl(XmlDecl<'a>),

    /// `<!DOCTYPE name ...>`. Per §2.8 [Production 28]. The internal
    /// subset is tokenised and checked by the lexer; `body` is the raw text
    /// after the name.
    Doctype {
        /// The root element name the DOCTYPE declares.
        name: &'a str,
        /// The rest of the declaration as written (external ID and
        /// internal subset), trimmed.
        body: &'a str,
    },

    /// Element start tag: `<name attr="value" ...>`. Per §3.1 [Production 40].
    StartTag {
        /// Element name as written.
        name: &'a str,
        /// Attributes as written (values not yet normalised).
        attributes: Vec<Attr<'a>>,
    },

    /// Empty-element tag: `<name attr="value" .../>`. Per §3.1 [Production 44].
    EmptyTag {
        /// Element name as written.
        name: &'a str,
        /// Attributes as written (values not yet normalised).
        attributes: Vec<Attr<'a>>,
    },

    /// Element end tag: `</name>`. Per §3.1 [Production 42].
    EndTag(&'a str),

    /// Character data between tags. Per §2.4 [Production 14]. Whitespace-only
    /// text is preserved (the parser layer decides whether to surface it as
    /// significant whitespace).
    Text(&'a str),

    /// `<![CDATA[ ... ]]>`. Per §2.7 [Production 18].
    CData(&'a str),

    /// `<!-- ... -->`. Per §2.5 [Production 15].
    Comment(&'a str),

    /// `<?target body?>`. Per §2.6 [Production 16].
    ProcessingInstruction {
        /// The PI target.
        target: &'a str,
        /// Everything after the target and its following whitespace.
        body: &'a str,
    },

    /// `&name;` entity reference. Per §4.1 [Production 68].
    EntityRef(&'a str),

    /// `&#nnn;` or `&#xhhh;` character reference. Per §4.1 [Production 66].
    /// Returns the resolved Unicode scalar.
    CharRef(char),
}

/// A single attribute on an element. Per §3.1 [Production 41].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr<'a> {
    /// Attribute name as written.
    pub name: &'a str,
    /// Value between the quotes, as written: references are checked but
    /// not expanded, and whitespace is not normalised.
    pub value: &'a str,
    /// The opening position of the attribute name, for error reporting.
    pub pos: Position,
}

/// XML declaration components per §2.8 [Production 23–25].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlDecl<'a> {
    /// The `version` value, e.g. `"1.0"`.
    pub version: &'a str,
    /// The `encoding` value, if given.
    pub encoding: Option<&'a str>,
    /// `standalone="yes"` (`Some(true)`) or `"no"` (`Some(false)`), if given.
    pub standalone: Option<bool>,
}
