//! Errors and positions.

use thiserror::Error;

/// Line breaks in `run` (`\n`, `\r\n` and a lone `\r` each count once),
/// and the offset just after the last one (0 if none). `after_cr`: the byte
/// before `run` was `\r`, so a leading `\n` completes that break.
pub(crate) fn line_breaks(after_cr: bool, run: &[u8]) -> (u32, usize) {
    let mut n = 0;
    let mut line_start = 0;
    let mut prev_cr = after_cr;
    for (i, &b) in run.iter().enumerate() {
        match b {
            b'\n' if prev_cr => line_start = i + 1,
            b'\n' | b'\r' => {
                n += 1;
                line_start = i + 1;
            }
            _ => {}
        }
        prev_cr = b == b'\r';
    }
    (n, line_start)
}

/// Characters in UTF-8 `bytes` (bytes that aren't continuation bytes).
fn chars(bytes: &[u8]) -> u32 {
    bytes.iter().filter(|&&b| b & 0xC0 != 0x80).count() as u32
}

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
    /// characters; a line ends at `\n`, `\r\n` or a lone `\r` (§2.11).
    pub fn advance(self, text: &str) -> Position {
        self.advance_bytes(false, text.as_bytes())
    }

    /// As `advance`, over UTF-8 bytes; `after_cr` says whether the byte
    /// just before `run` was `\r` (so a leading `\n` ends no new line).
    pub(crate) fn advance_bytes(mut self, after_cr: bool, run: &[u8]) -> Position {
        match line_breaks(after_cr, run) {
            (0, _) => self.column += chars(run),
            (n, line_start) => {
                self.line += n;
                self.column = 1 + chars(&run[line_start..]);
            }
        }
        self.byte_offset += run.len();
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
        /// The class of problem; see [`ErrorKind`].
        kind: ErrorKind,
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
        /// The class of problem; see [`ErrorKind`].
        kind: ErrorKind,
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

/// The class of an error, with one variant per distinct error libexpat
/// reports (its `XML_Error` codes), so that callers that need libexpat's
/// codes can map one-to-one. Each variant names the libexpat code it
/// corresponds to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// Malformed markup in the DTD (`XML_ERROR_SYNTAX`).
    Syntax,
    /// The input ended with no root element, an unclosed element or an
    /// unclosed internal subset (`XML_ERROR_NO_ELEMENTS`).
    NoElements,
    /// Malformed markup or a character not allowed where it appears
    /// (`XML_ERROR_INVALID_TOKEN`).
    InvalidToken,
    /// The input ended inside a construct (`XML_ERROR_UNCLOSED_TOKEN`).
    UnclosedToken,
    /// The input ended inside a multi-byte character
    /// (`XML_ERROR_PARTIAL_CHAR`).
    PartialChar,
    /// An end tag doesn't match its start tag (`XML_ERROR_TAG_MISMATCH`).
    TagMismatch,
    /// An attribute is given twice (`XML_ERROR_DUPLICATE_ATTRIBUTE`).
    DuplicateAttribute,
    /// Content after the root element (`XML_ERROR_JUNK_AFTER_DOC_ELEMENT`).
    JunkAfterDocElement,
    /// A parameter-entity reference where none is allowed
    /// (`XML_ERROR_PARAM_ENTITY_REF`).
    ParamEntityRef,
    /// A reference to an undeclared entity (`XML_ERROR_UNDEFINED_ENTITY`).
    UndefinedEntity,
    /// An entity refers to itself (`XML_ERROR_RECURSIVE_ENTITY_REF`).
    RecursiveEntityRef,
    /// An entity's markup isn't balanced within it
    /// (`XML_ERROR_ASYNC_ENTITY`).
    AsyncEntity,
    /// A character reference to a character that isn't allowed
    /// (`XML_ERROR_BAD_CHAR_REF`).
    BadCharRef,
    /// A reference to an unparsed entity (`XML_ERROR_BINARY_ENTITY_REF`).
    BinaryEntityRef,
    /// A reference to an external entity in an attribute value
    /// (`XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF`).
    AttributeExternalEntityRef,
    /// An XML or text declaration away from the start
    /// (`XML_ERROR_MISPLACED_XML_PI`).
    MisplacedXmlPi,
    /// The declared encoding isn't supported (`XML_ERROR_UNKNOWN_ENCODING`).
    UnknownEncoding,
    /// The bytes contradict the declared encoding
    /// (`XML_ERROR_INCORRECT_ENCODING`).
    IncorrectEncoding,
    /// The input ended inside a CDATA section
    /// (`XML_ERROR_UNCLOSED_CDATA_SECTION`).
    UnclosedCdataSection,
    /// An external entity couldn't be loaded
    /// (`XML_ERROR_EXTERNAL_ENTITY_HANDLING`).
    ExternalEntityHandling,
    /// A malformed XML declaration (`XML_ERROR_XML_DECL`).
    XmlDecl,
    /// A malformed text declaration (`XML_ERROR_TEXT_DECL`).
    TextDecl,
    /// A character not allowed in a public identifier
    /// (`XML_ERROR_PUBLICID`).
    PublicId,
    /// Entity expansion exceeded the configured limits
    /// (`XML_ERROR_AMPLIFICATION_LIMIT_BREACH`).
    AmplificationLimitBreach,
    /// A namespace prefix that isn't declared (`XML_ERROR_UNBOUND_PREFIX`).
    UnboundPrefix,
    /// `xmlns:p=""`, not allowed in Namespaces 1.0
    /// (`XML_ERROR_UNDECLARING_PREFIX`).
    UndeclaringPrefix,
    /// The `xml` prefix bound to the wrong namespace
    /// (`XML_ERROR_RESERVED_PREFIX_XML`).
    ReservedPrefixXml,
    /// The `xmlns` prefix declared (`XML_ERROR_RESERVED_PREFIX_XMLNS`).
    ReservedPrefixXmlns,
    /// A reserved namespace name bound to the wrong prefix
    /// (`XML_ERROR_RESERVED_NAMESPACE_URI`).
    ReservedNamespaceUri,
    /// The parser was used after it finished (`XML_ERROR_FINISHED`).
    Finished,
    /// An API call made with arguments it can't accept
    /// (`XML_ERROR_INVALID_ARGUMENT`).
    InvalidArgument,
}

impl XmlError {
    /// The class of the error.
    pub fn kind(&self) -> ErrorKind {
        match self {
            XmlError::NotWellFormed { kind, .. } | XmlError::Encoding { kind, .. } => *kind,
            XmlError::UnexpectedEof { context: "CDATA", .. } => ErrorKind::UnclosedCdataSection,
            XmlError::UnexpectedEof { context: "DOCTYPE internal subset", .. } => ErrorKind::NoElements,
            XmlError::UnexpectedEof { .. } => ErrorKind::UnclosedToken,
            XmlError::InvalidChar { .. } => ErrorKind::InvalidToken,
            XmlError::ExternalEntity { .. } => ErrorKind::ExternalEntityHandling,
        }
    }

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
