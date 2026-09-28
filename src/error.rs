use thiserror::Error;

/// Position in the input — line and column are 1-based, byte offset 0-based,
/// matching libexpat's error reporting convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub column: u32,
    pub byte_offset: usize,
}

impl Position {
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

#[non_exhaustive]
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum XmlError {
    /// Malformed XML — not well-formed per W3C XML 1.0 §2.1.
    #[error("not well-formed at {pos:?}: {reason}")]
    NotWellFormed { pos: Position, reason: String },

    /// Input ended in the middle of a construct.
    #[error("unexpected end of input at {pos:?} (in {context})")]
    UnexpectedEof { pos: Position, context: &'static str },

    /// A character that is not allowed in XML 1.0 (per §2.2 [Production 2]).
    #[error("invalid character {char:?} at {pos:?}")]
    InvalidChar { pos: Position, char: char },

    /// Encoding-related error.
    #[error("encoding error at {pos:?}: {reason}")]
    Encoding { pos: Position, reason: String },

    /// An external entity could not be loaded (the caller's loader failed).
    #[error("cannot load external entity at {pos:?}: {reason}")]
    ExternalEntity { pos: Position, reason: String },
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

pub type Result<T> = std::result::Result<T, XmlError>;
