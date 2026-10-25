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

use crate::error::Position;
use crate::token::XmlDecl;

/// Something the parser found in the document, in document order.
///
/// Strings are borrowed from the input where they can be used as written
/// and owned where the parser changed them; `'a` is the input's lifetime.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// `<?xml version="..." ...?>`. May appear at most once, as the first
    /// event in the stream. Per W3C XML 1.0 §2.8.
    XmlDecl(XmlDecl<'a>),

    /// `<!DOCTYPE name ...>`. May appear at most once, before the root
    /// element. Per §2.8 [Production 28].
    Doctype {
        /// The root element name the DOCTYPE declares.
        name: &'a str,
        /// The rest of the declaration as written, trimmed.
        body: &'a str,
        /// System identifier of the external subset, if any.
        system_id: Option<Cow<'a, str>>,
        /// Public identifier of the external subset, if any.
        public_id: Option<Cow<'a, str>>,
        /// Whether the DOCTYPE has an internal subset (`[...]`).
        has_internal_subset: bool,
    },

    /// Element start (or empty-element). Per §3.
    StartElement {
        /// The name as written (a QName when namespaces are on).
        name: Cow<'a, str>,
        /// Namespace name; set only when namespace processing is enabled
        /// and the element is in a namespace.
        namespace: Option<Cow<'a, str>>,
        /// Specified attributes, then defaults declared in the DTD.
        attributes: Vec<Attribute<'a>>,
    },

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
    ProcessingInstruction {
        /// The PI target.
        target: Cow<'a, str>,
        /// Everything after the target and its following whitespace.
        body: Cow<'a, str>,
    },

    /// End of the DOCTYPE. Events for what the internal subset contained
    /// (processing instructions, comments, notation declarations) come
    /// between `Doctype` and this, in document order. Mirrors libexpat's
    /// end-doctype handler.
    EndDoctype,

    /// An element type declaration from the internal subset (§3.2), between
    /// `Doctype` and `EndDoctype`. Mirrors libexpat's element-declaration
    /// handler.
    ElementDecl {
        /// The element type declared.
        name: Cow<'a, str>,
        /// Its content model.
        model: ContentModel,
    },

    /// One attribute definition from an attribute-list declaration in the
    /// internal subset (§3.3), between `Doctype` and `EndDoctype`. Mirrors
    /// libexpat's attribute-list handler, which is called once per
    /// attribute.
    AttlistDecl {
        /// The element type the attribute belongs to.
        element: Cow<'a, str>,
        /// The attribute's name.
        attribute: Cow<'a, str>,
        /// The declared type as libexpat spells it: `CDATA`, `ID`, …,
        /// `NMTOKENS`, an enumeration `(a|b)` or `NOTATION(x|y)`.
        att_type: Cow<'a, str>,
        /// The default value (plain or #FIXED, normalised), if any.
        default: Option<Cow<'a, str>>,
        /// `true` for #REQUIRED and #FIXED.
        required: bool,
    },

    /// A general or parameter entity declaration from the internal subset
    /// (§4.2), between `Doctype` and `EndDoctype`. Mirrors libexpat's
    /// entity-declaration handler.
    EntityDecl {
        /// The entity's name.
        name: Cow<'a, str>,
        /// A parameter entity (`<!ENTITY % ...>`).
        parameter: bool,
        /// Replacement text of an internal entity; `None` for external.
        value: Option<Cow<'a, str>>,
        /// System identifier of an external entity.
        system_id: Option<Cow<'a, str>>,
        /// Public identifier of an external entity, if given.
        public_id: Option<Cow<'a, str>>,
        /// Notation name of an unparsed entity (`NDATA`).
        notation: Option<Cow<'a, str>>,
    },

    /// A notation declaration from the DTD (§4.7), between `Doctype` and
    /// `EndDoctype`. Mirrors libexpat's `XML_SetNotationDeclHandler`.
    NotationDecl {
        /// The notation's name.
        name: Cow<'a, str>,
        /// Public identifier, if given.
        public_id: Option<Cow<'a, str>>,
        /// System identifier, if given.
        system_id: Option<Cow<'a, str>>,
    },

    /// A namespace declaration coming into scope, just before the
    /// `StartElement` that carries it (namespace processing only).
    /// `prefix` is `None` for the default namespace; `uri` is `None` when
    /// `xmlns=""` undeclares it. Mirrors libexpat's
    /// `XML_SetStartNamespaceDeclHandler`.
    StartNamespace {
        /// The prefix declared, or `None` for the default namespace.
        prefix: Option<Cow<'a, str>>,
        /// The namespace name, or `None` when `xmlns=""` undeclares the
        /// default namespace.
        uri: Option<Cow<'a, str>>,
    },

    /// A namespace declaration going out of scope, just after the matching
    /// `EndElement` (namespace processing only).
    EndNamespace {
        /// The prefix going out of scope, or `None` for the default.
        prefix: Option<Cow<'a, str>>,
    },

    /// A reference in content to an entity whose text was not read: an
    /// external entity with no loader installed, or an undeclared entity
    /// where that is only a validity error. Mirrors libexpat's
    /// `XML_SetSkippedEntityHandler`.
    SkippedEntity(Cow<'a, str>),
}

/// A content model from an element declaration (§3.2): a tree stored as a
/// flat list of nodes that refer to their children by index, like
/// libexpat's `XML_Content` array. Nothing about it is recursive, so a
/// hostile, deeply nested declaration can't exhaust the stack when the
/// model is built, cloned, compared, printed or dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentModel {
    /// All nodes of the tree.
    pub nodes: Vec<ContentNode>,
    /// Index of the root node in `nodes`.
    pub root: usize,
}

/// One node of a [`ContentModel`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentNode {
    /// What this node is.
    pub kind: ContentKind,
    /// Its occurrence indicator.
    pub quant: Quantifier,
    /// The element name, for [`ContentKind::Name`] nodes.
    pub name: Option<String>,
    /// Indices (in [`ContentModel::nodes`]) of the children of a choice,
    /// sequence or mixed group, in order (names, for mixed).
    pub children: Vec<usize>,
}

impl ContentModel {
    /// The root node.
    pub fn root(&self) -> &ContentNode {
        &self.nodes[self.root]
    }

    /// The children of `node`, in order.
    pub fn children<'m>(&'m self, node: &'m ContentNode) -> impl Iterator<Item = &'m ContentNode> {
        node.children.iter().map(|&i| &self.nodes[i])
    }
}

/// The kind of a [`ContentModel`] node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentKind {
    /// `EMPTY`.
    Empty,
    /// `ANY`.
    Any,
    /// `(#PCDATA)` or `(#PCDATA|a|b)*`.
    Mixed,
    /// An element name.
    Name,
    /// `(a|b)`.
    Choice,
    /// `(a,b)`, and a group of one, `(a)`.
    Seq,
}

/// An occurrence indicator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quantifier {
    /// Exactly once.
    None,
    /// `?`
    Opt,
    /// `*`
    Rep,
    /// `+`
    Plus,
}

/// Where an event came from in the (decoded) input: from `start` up to,
/// not including, `end`. Events produced by an entity reference carry the
/// span of the reference; events for internal-subset declarations, the
/// span of their declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Position of the event's first character.
    pub start: Position,
    /// Position just after its last character.
    pub end: Position,
}

/// An event with its [`Span`] and the source text the span covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located<'a> {
    /// The event.
    pub event: Event<'a>,
    /// Where it came from.
    pub span: Span,
    /// The source text of `span`, exactly as written (after decoding to
    /// UTF-8; line endings not normalised).
    pub raw: &'a str,
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
