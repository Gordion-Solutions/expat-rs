//! Errors and positions.

use thiserror::Error;

/// Position in the input — line and column are 1-based, byte offset 0-based,
/// matching libexpat's error reporting convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    /// Line number, from 1. Lines end at `\n`.
    pub line: u32,
    /// Column within the line, from 1, counted in characters.
    pub column: u32,
    /// Offset in bytes from the start of the (decoded, UTF-8) input.
    pub byte_offset: usize,
}

impl Position {
    /// Line 1, column 1, byte 0.
    pub const fn start() -> Self {
        Self { line: 1, column: 1, byte_offset: 0 }
    }

    /// The position just after `text`, which starts here. Columns count
    /// characters.
    pub(crate) fn advance(mut self, text: &str) -> Position {
        for b in text.bytes() {
            if b == b'\n' {
                self.line += 1;
                self.column = 1;
            } else if b & 0xC0 != 0x80 {
                // Not a UTF-8 continuation byte: a new character.
                self.column += 1;
            }
        }
        self.byte_offset += text.len();
        self
    }

    /// This position, measured within text that itself starts at `base`,
    /// expressed relative to the whole input.
    pub(crate) fn rebase(self, base: Position) -> Position {
        Position {
            line: base.line + self.line - 1,
            column: if self.line == 1 { base.column + self.column - 1 } else { self.column },
            byte_offset: base.byte_offset + self.byte_offset,
        }
    }
}

/// Why a document was rejected. Every variant carries the [`Position`]
/// where the problem was found; [`XmlError::position`] returns it.
#[non_exhaustive]
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum XmlError {
    /// Malformed XML — not well-formed per W3C XML 1.0 (or, with namespace
    /// processing on, Namespaces in XML 1.0).
    #[error("not well-formed at {pos:?}: {reason}")]
    NotWellFormed {
        /// Where the problem was found.
        pos: Position,
        /// What is wrong, in words.
        reason: String,
    },

    /// Input ended in the middle of a construct.
    #[error("unexpected end of input at {pos:?} (in {context})")]
    UnexpectedEof {
        /// The end of the input.
        pos: Position,
        /// The construct that was cut off, e.g. `"Comment"`.
        context: &'static str,
    },

    /// A character that is not allowed in XML 1.0 (per §2.2 [Production 2]).
    #[error("invalid character {char:?} at {pos:?}")]
    InvalidChar {
        /// Where the character is.
        pos: Position,
        /// The character.
        char: char,
    },

    /// The input's bytes could not be decoded (see [`crate::decode`]).
    #[error("encoding error at {pos:?}: {reason}")]
    Encoding {
        /// Always the start of the input.
        pos: Position,
        /// What is wrong, in words.
        reason: String,
    },

    /// An external entity could not be loaded (the caller's loader failed).
    #[error("cannot load external entity at {pos:?}: {reason}")]
    ExternalEntity {
        /// Where the entity was referenced.
        pos: Position,
        /// The entity, its system identifier and the loader's reason.
        reason: String,
    },
}

impl XmlError {
    /// Where the error occurred.
    pub fn position(&self) -> Position {
        match self {
            XmlError::NotWellFormed { pos, .. }
            | XmlError::UnexpectedEof { pos, .. }
            | XmlError::InvalidChar { pos, .. }
            | XmlError::Encoding { pos, .. }
            | XmlError::ExternalEntity { pos, .. } => *pos,
        }
    }

    /// The same error with its position rebased (see `Position::rebase`).
    pub(crate) fn rebase(mut self, base: Position) -> XmlError {
        match &mut self {
            XmlError::NotWellFormed { pos, .. }
            | XmlError::UnexpectedEof { pos, .. }
            | XmlError::InvalidChar { pos, .. }
            | XmlError::Encoding { pos, .. }
            | XmlError::ExternalEntity { pos, .. } => *pos = pos.rebase(base),
        }
        self
    }
}

/// `Result` with [`XmlError`] as the error type.
pub type Result<T> = std::result::Result<T, XmlError>;
