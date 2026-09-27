//! expat-rs — memory-safe XML 1.0 parser, libexpat-compatible conformance.
//!
//! Built clean-room from the W3C XML 1.0 Recommendation. See
//! [`METHODOLOGY.md`](../METHODOLOGY.md) at the crate root for the
//! clean-room declaration and audit trail.

mod chars;
mod edition4;
mod expand;
pub mod error;
pub mod token;
pub mod lexer;
pub mod event;
pub mod parser;
pub mod entities;
pub mod encoding;

pub use chars::Edition;
pub use encoding::decode;
pub use error::{Position, XmlError, Result};
pub use event::{Attribute, Event};
pub use lexer::Lexer;
pub use parser::Parser;
pub use token::{Attr, Token, XmlDecl};
pub use entities::{EntityTable, ExpansionLimits};
