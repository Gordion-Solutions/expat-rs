//! External parsed entities read by the caller.
//!
//! By default a reference in content to an external parsed entity is
//! either read by the external loader (see
//! [`crate::StreamParser::with_external_loader`]) or skipped. With
//! [`crate::StreamParser::with_external_entity_refs`], it is reported
//! instead as [`crate::Event::ExternalEntityRef`], carrying an
//! [`EntityContext`]: the caller fetches the entity however it likes and
//! parses it with [`EntityContext::parser`], a parser for entity content
//! (§4.3.2) that knows the document's entities, attribute defaults and the
//! namespace declarations in scope at the reference. This is how libexpat's
//! external entity reference handler and `XML_ExternalEntityParserCreate`
//! work.

use std::fmt;
use std::sync::Arc;

use crate::chars::Edition;
use crate::entities::{AmplificationLimits, AttlistTable, EntityTable, ExpansionLimits};
use crate::stream::StreamParser;

/// What a parser for an external entity's content needs from the document
/// that refers to it. Cheap to clone: the declaration tables are shared.
#[derive(Clone)]
pub struct EntityContext {
    pub(crate) name: String,
    pub(crate) entities: Arc<EntityTable>,
    pub(crate) attlists: Arc<AttlistTable>,
    pub(crate) edition: Edition,
    pub(crate) libexpat_versions: bool,
    pub(crate) entity_declared_wfc: bool,
    pub(crate) expansion_limits: ExpansionLimits,
    pub(crate) amplification: AmplificationLimits,
    /// Namespace declarations in scope at the reference, as (prefix, name);
    /// `None` when namespace processing is off.
    pub(crate) namespaces: Option<Vec<(Option<String>, Option<String>)>>,
    /// Entities being read where the reference occurs, outermost first,
    /// including this one: a reference to any of them from inside is
    /// recursive (§4.1, No Recursion).
    pub(crate) open: Vec<String>,
}

impl EntityContext {
    /// The name of the entity referred to.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// A push parser for the entity's content: an optional text
    /// declaration (§4.3.1), then content (§4.3.2), which may hold any
    /// number of elements and character data and must close every element
    /// it opens. Entity references resolve against the referring
    /// document's declarations, prefixes against the namespace declarations
    /// in scope at the reference, and further external entity references
    /// are reported in the same way.
    pub fn parser(&self) -> StreamParser<'static> {
        StreamParser::for_entity(self)
    }
}

impl fmt::Debug for EntityContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EntityContext")
            .field("name", &self.name)
            .field("namespaces", &self.namespaces)
            .field("open", &self.open)
            .finish_non_exhaustive()
    }
}

/// Two contexts are equal when they are for the same entity, at the same
/// place in the same document (the same declaration tables and namespace
/// declarations).
impl PartialEq for EntityContext {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && Arc::ptr_eq(&self.entities, &other.entities)
            && Arc::ptr_eq(&self.attlists, &other.attlists)
            && self.namespaces == other.namespaces
            && self.open == other.open
    }
}

impl Eq for EntityContext {}
