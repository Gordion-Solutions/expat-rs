//! DTD entity declarations + safe expansion.
//!
//! The XML 1.0 spec [§4.2] permits internal general entity declarations of
//! the form:
//!
//!   <!ENTITY name "value">       — double-quoted
//!   <!ENTITY name 'value'>       — single-quoted
//!
//! and external ones (`<!ENTITY name SYSTEM "uri">`, optionally unparsed
//! with `NDATA`). Entity values may themselves reference other entities.
//! Naïve recursive expansion is the **billion laughs** vulnerability class:
//!
//!   <!ENTITY a "AAA">
//!   <!ENTITY b "&a;&a;&a;&a;&a;">     <!-- 5× a -->
//!   <!ENTITY c "&b;&b;&b;&b;&b;">     <!-- 5×b = 25×a -->
//!   ...                                <!-- exponential -->
//!
//! Mitigation (in `crate::expand`) is a hard recursion-depth cap and a hard
//! cumulative expansion-size cap. Either limit triggers an error rather
//! than letting the parser do unbounded work.

use std::collections::HashMap;
use crate::chars::decode_char_ref;
use crate::error::Position;

/// Hard limits on entity expansion. Defaults match the conservative end of
/// what's reasonable for adversarial input; library callers will be able to
/// tune these in a future config API.
#[derive(Clone, Copy, Debug)]
pub struct ExpansionLimits {
    /// Maximum depth of nested entity references during a single expansion.
    pub max_depth: usize,
    /// Maximum *total* number of bytes any single entity reference may
    /// expand to (sums across all nested references).
    pub max_expanded_bytes: usize,
}

impl Default for ExpansionLimits {
    fn default() -> Self {
        Self {
            max_depth: 20,
            max_expanded_bytes: 1024 * 1024, // 1 MiB
        }
    }
}

/// What a general entity was declared as (§4.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EntityDef {
    /// Internal entity. Holds the *replacement text*: the literal value with
    /// character references already expanded (§4.5) and general entity
    /// references left in place, to be expanded where the entity is used.
    Internal(String),
    /// External entity (§4.2.2). Unparsed if declared with `NDATA`.
    External { system_id: String, public_id: Option<String>, unparsed: bool },
}

/// One declaration from the internal subset that the parser acts on, in
/// document order. Order matters: §4.1 WFC Entity Declared requires an
/// entity to be declared before it is referenced in an attribute default.
#[derive(Clone, Debug)]
pub(crate) enum DtdDecl {
    Entity { name: String, parameter: bool, def: EntityDef },
    /// One attribute definition from an attribute-list declaration (§3.3).
    AttDef {
        element: String,
        name: String,
        /// Declared type is CDATA. Other types get extra whitespace
        /// normalisation (§3.3.3).
        cdata: bool,
        /// Default value as written between the quotes (plain or #FIXED);
        /// `None` for #REQUIRED and #IMPLIED.
        default: Option<String>,
        pos: Position,
    },
    /// A processing instruction in the internal subset (§2.6).
    Pi { target: String, body: String },
    /// A comment in the internal subset (§2.5).
    Comment(String),
    /// A notation declaration (§4.7).
    Notation { name: String, public_id: Option<String>, system_id: Option<String> },
    /// A parameter-entity reference between declarations (§2.8 DeclSep).
    /// Parameter entities are not expanded yet, so per §5.1 declarations
    /// after one are not processed.
    PeRef,
}

/// The parts of a DOCTYPE the parser needs after tokenising.
#[derive(Clone, Debug, Default)]
pub(crate) struct Dtd {
    pub decls: Vec<DtdDecl>,
    /// Whether the DOCTYPE names an external subset (SYSTEM / PUBLIC).
    pub external_subset: bool,
}

/// Declared general entities, by name.
#[derive(Default, Debug)]
pub(crate) struct EntityTable {
    entities: HashMap<String, EntityDef>,
}

impl EntityTable {
    pub(crate) fn new() -> Self { Self::default() }

    /// Declare an internal entity whose literal value is `value`.
    #[allow(dead_code)]
    pub(crate) fn declare(&mut self, name: String, value: String) {
        let text = expand_char_refs(&value);
        self.declare_def(name, EntityDef::Internal(text));
    }

    /// Per §4.2, the first declaration of an entity is binding; later ones
    /// are ignored.
    pub(crate) fn declare_def(&mut self, name: String, def: EntityDef) {
        self.entities.entry(name).or_insert(def);
    }

    #[allow(dead_code)]
    pub(crate) fn is_declared(&self, name: &str) -> bool {
        self.entities.contains_key(name)
    }

    pub(crate) fn get(&self, name: &str) -> Option<&EntityDef> {
        self.entities.get(name)
    }
}

/// A declared attribute, from `<!ATTLIST>` (§3.3).
#[derive(Clone, Debug)]
pub(crate) struct AttDecl {
    pub name: String,
    /// Declared type is CDATA (no extra whitespace normalisation).
    pub cdata: bool,
    /// Normalised default value (plain or #FIXED), or `None` for
    /// #REQUIRED / #IMPLIED.
    pub default: Option<String>,
}

/// Attribute declarations by element name.
#[derive(Default, Debug)]
pub(crate) struct AttlistTable {
    by_element: HashMap<String, ElementAtts>,
}

/// One element's declared attributes, in declaration order, with an index
/// by name so lookups stay constant-time however many are declared.
#[derive(Default, Debug)]
pub(crate) struct ElementAtts {
    pub decls: Vec<AttDecl>,
    index: HashMap<String, usize>,
}

impl ElementAtts {
    pub fn get(&self, name: &str) -> Option<&AttDecl> {
        self.index.get(name).map(|&i| &self.decls[i])
    }
}

impl AttlistTable {
    /// Per §3.3, the first definition of an attribute for an element is
    /// binding; later ones are ignored.
    pub fn declare(&mut self, element: String, decl: AttDecl) {
        let atts = self.by_element.entry(element).or_default();
        if !atts.index.contains_key(&decl.name) {
            atts.index.insert(decl.name.clone(), atts.decls.len());
            atts.decls.push(decl);
        }
    }

    pub fn get(&self, element: &str) -> Option<&ElementAtts> {
        self.by_element.get(element)
    }
}

/// Built-in entities defined by the XML spec (always available, no DTD needed).
/// Per §4.6 [Production 67].
pub fn builtin_entity(name: &str) -> Option<&'static str> {
    match name {
        "lt"   => Some("<"),
        "gt"   => Some(">"),
        "amp"  => Some("&"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        _      => None,
    }
}

/// Replacement text of an internal entity from its literal value (§4.5):
/// character references are replaced by the characters they name; general
/// entity references are kept as written. The lexer has already checked
/// every reference in `literal`, so malformed ones are copied through.
pub(crate) fn expand_char_refs(literal: &str) -> String {
    let mut out = String::with_capacity(literal.len());
    let mut rest = literal;
    while let Some(i) = rest.find("&#") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find(';').map(|end| (end, decode_char_ref(&after[..end]))) {
            Some((end, Ok(c))) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str("&#");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
