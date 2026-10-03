//! expat-rs — a memory-safe XML 1.0 parser, built clean-room from the W3C
//! specifications, aiming for libexpat's conformance without C's
//! memory-safety bugs. See `METHODOLOGY.md` at the crate root for the
//! clean-room declaration.
//!
//! # Parsing a document
//!
//! [`Parser`] pulls [`Event`]s from a document held in one string. Text is
//! borrowed from the input where it can be used as written.
//!
//! ```
//! use expat_rs::{Event, Parser};
//!
//! let mut parser = Parser::new("<greeting lang='en'>Hello, &amp; welcome</greeting>");
//! let mut text = String::new();
//! while let Some(event) = parser.next_event()? {
//!     match event {
//!         Event::StartElement { name, attributes, .. } => {
//!             assert_eq!(name, "greeting");
//!             assert_eq!(attributes[0].value, "en");
//!         }
//!         Event::Text(t) => text.push_str(&t),
//!         _ => {}
//!     }
//! }
//! assert_eq!(text, "Hello, & welcome");
//! # Ok::<(), expat_rs::XmlError>(())
//! ```
//!
//! Character data may arrive in several consecutive [`Event::Text`]
//! pieces; join them if you need the whole run.
//!
//! # Bytes and encodings
//!
//! [`decode`] turns a document's bytes into text, detecting UTF-8, UTF-16
//! (either byte order), US-ASCII and ISO-8859-1 from the byte order mark
//! and the encoding declaration.
//!
//! ```
//! let bytes = b"<?xml version='1.0' encoding='ISO-8859-1'?><p>caf\xe9</p>";
//! let text = expat_rs::decode(bytes)?;
//! let mut parser = expat_rs::Parser::new(&text);
//! # while parser.next_event()?.is_some() {}
//! # Ok::<(), expat_rs::XmlError>(())
//! ```
//!
//! # Input in chunks
//!
//! [`StreamParser`] takes input as it arrives, like libexpat's `XML_Parse`,
//! and passes events to a handler as soon as they are complete. Results
//! are the same however the input is split.
//!
//! ```
//! use expat_rs::{Event, StreamParser};
//!
//! let mut parser = StreamParser::new();
//! let mut elements = 0;
//! for chunk in [&b"<list><item/><it"[..], b"em/></li", b"st>"] {
//!     parser.feed(chunk, |e| if let Event::StartElement { .. } = e { elements += 1 })?;
//! }
//! parser.finish(|_| {})?;
//! assert_eq!(elements, 3);
//! # Ok::<(), expat_rs::XmlError>(())
//! ```
//!
//! # Namespaces
//!
//! Namespace processing (W3C Namespaces in XML 1.0) is opt-in, as in
//! libexpat. With it on, elements and attributes carry their namespace
//! name and `xmlns` declarations arrive as [`Event::StartNamespace`] /
//! [`Event::EndNamespace`].
//!
//! ```
//! use expat_rs::{Event, Parser};
//!
//! let mut parser = Parser::new("<feed xmlns='http://www.w3.org/2005/Atom'><entry/></feed>")
//!     .with_namespaces();
//! while let Some(event) = parser.next_event()? {
//!     if let Event::StartElement { name, namespace, .. } = event {
//!         assert_eq!(namespace.as_deref(), Some("http://www.w3.org/2005/Atom"));
//!         # let _ = name;
//!     }
//! }
//! # Ok::<(), expat_rs::XmlError>(())
//! ```
//!
//! # Safety against hostile input
//!
//! - **Entity expansion is bounded** ([`ExpansionLimits`]): depth and
//!   total size, per reference and per document, against billion-laughs
//!   and quadratic-blowup payloads.
//! - **External entities are never read** unless you install a loader
//!   with [`Parser::with_external_loader`], which rules out XXE-style file
//!   disclosure by default.
//! - **No input costs more than linear time** in its size: deep nesting,
//!   very many attributes and huge constructs fed in tiny chunks are all
//!   handled without quadratic blow-up (the class of libexpat
//!   CVE-2023-52425 is avoided by reparse deferral), and nothing recurses
//!   on input structure.
//!
//! ```
//! let bomb = r#"<!DOCTYPE b [<!ENTITY a "aaaaaaaaaa">
//!   <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"> <!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
//!   <!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;"> <!ENTITY e "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;">
//!   <!ENTITY f "&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;"> <!ENTITY g "&f;&f;&f;&f;&f;&f;&f;&f;&f;&f;">]>
//!   <b>&g;</b>"#;
//! let mut parser = expat_rs::Parser::new(bomb);
//! let result = (|| { while parser.next_event()?.is_some() {} Ok::<_, expat_rs::XmlError>(()) })();
//! assert!(result.is_err(), "10^7-byte expansion is refused");
//! ```
//!
//! # Conformance
//!
//! On the W3C XML Conformance Test Suite, 1441 of the 1501 well-formedness
//! tests that apply to the XML 1.0 Fifth Edition pass, every well-formed
//! test is accepted, and all 48 Namespaces 1.0 tests pass. XML 1.0 Fourth
//! Edition name rules are available with [`Edition::Fourth`]. See
//! `conformance/STATUS.md`.
//!
//! **Not yet supported:** reading the external DTD subset and expanding
//! parameter entities (declarations there are not processed, as a
//! non-validating processor is permitted), DTD validation, XML 1.1, and a
//! libexpat-compatible C API.

mod chars;
mod edition4;
mod expand;
mod namespaces;
pub mod error;
pub mod token;
pub mod lexer;
pub mod event;
pub mod parser;
pub mod stream;
pub mod entities;
pub mod encoding;

pub use chars::Edition;
pub use encoding::decode;
pub use error::{Position, XmlError, Result};
pub use event::{local_name, Attribute, Event};
pub use lexer::Lexer;
pub use parser::Parser;
pub use stream::StreamParser;
pub use encoding::StreamDecoder;
pub use token::{Attr, Token, XmlDecl};
pub use entities::{EntityTable, ExpansionLimits};
