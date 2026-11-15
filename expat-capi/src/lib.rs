//! libexpat-compatible C ABI over expat-rs.
//!
//! Exports the libexpat functions CPython's `pyexpat` and `_elementtree`
//! link against (see `capi/phase1/surface.py`), with libexpat 2.7.1's own
//! `include/expat.h`. Built as `libexpat.{so,dylib,a}`.
//!
//! # Safety model
//!
//! An `XML_Parser` is a pointer to a heap-allocated [`Shim`], created by
//! `XML_ParserCreate*` and destroyed by `XML_ParserFree`. Every exported
//! function trusts that the pointer it is given came from `XML_ParserCreate*`
//! and has not been freed, exactly as libexpat does; that is the contract of
//! the C API and cannot be checked.
//!
//! libexpat lets handlers call back into the parser that is calling them
//! (`XML_GetCurrentLineNumber`, `XML_StopParser`, setting handlers, ...).
//! So no `&mut Shim` exists after creation: everything a re-entrant call may
//! read or change is in a `Cell`, and the expat-rs parser itself sits in a
//! `RefCell` that is borrowed only while parsing. A re-entrant call that
//! needs the parser (`XML_Parse` from inside a handler, say) finds it
//! borrowed and fails with an error instead of aliasing it.
//!
//! Nothing here may panic: a panic cannot unwind through `extern "C"`, so it
//! would abort the process.

#![allow(non_camel_case_types, non_snake_case, clippy::missing_safety_doc)]

mod generated;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_ulonglong, c_void, CStr, CString};
use std::ptr;

use expat_rs::{
    AmplificationLimits, ContentKind, ContentModel, Edition, EncodingMap, EntityContext, ErrorKind, Event, Located, MapEntry, Position, Quantifier, Span,
    StreamParser, XmlError,
};
// Public for Rust callers in the same build (the Python package).
pub use generated::*;

// ─── C types (layouts as in include/expat.h) ──────────────────────────────

pub type XML_Char = c_char;
pub type XML_LChar = c_char;
pub type XML_Bool = u8;
pub type XML_Status = c_int;
pub type XML_Error = c_int;
pub type XML_Size = c_ulong;
pub type XML_Index = c_long;
pub type XML_Parser = *mut Shim;

const XML_PARAM_ENTITY_PARSING_NEVER: c_int = 0;
const XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE: c_int = 1;
pub const XML_TRUE: XML_Bool = 1;
pub const XML_FALSE: XML_Bool = 0;
const XML_STATUS_ERROR: XML_Status = 0;
const XML_STATUS_OK: XML_Status = 1;

type StartElementHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *mut *const XML_Char)>;
type EndElementHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char)>;
type CharacterDataHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, c_int)>;
type ProcessingInstructionHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char)>;
type CommentHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char)>;
type CdataSectionHandler = Option<unsafe extern "C" fn(*mut c_void)>;
type DefaultHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, c_int)>;
type StartDoctypeDeclHandler =
    Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char, *const XML_Char, c_int)>;
type EndDoctypeDeclHandler = Option<unsafe extern "C" fn(*mut c_void)>;
type EntityDeclHandler = Option<
    unsafe extern "C" fn(
        *mut c_void, *const XML_Char, c_int, *const XML_Char, c_int, *const XML_Char,
        *const XML_Char, *const XML_Char, *const XML_Char,
    ),
>;
type UnparsedEntityDeclHandler = Option<
    unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char, *const XML_Char, *const XML_Char, *const XML_Char),
>;
type NotationDeclHandler =
    Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char, *const XML_Char, *const XML_Char)>;
type StartNamespaceDeclHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char)>;
type EndNamespaceDeclHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char)>;
type NotStandaloneHandler = Option<unsafe extern "C" fn(*mut c_void) -> c_int>;
type ExternalEntityRefHandler = Option<
    unsafe extern "C" fn(XML_Parser, *const XML_Char, *const XML_Char, *const XML_Char, *const XML_Char) -> c_int,
>;
type SkippedEntityHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, c_int)>;
type XmlDeclHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char, c_int)>;
type ElementDeclHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *mut XML_Content)>;
type AttlistDeclHandler = Option<
    unsafe extern "C" fn(*mut c_void, *const XML_Char, *const XML_Char, *const XML_Char, *const XML_Char, c_int),
>;
type UnknownEncodingHandler = Option<unsafe extern "C" fn(*mut c_void, *const XML_Char, *mut XML_Encoding) -> c_int>;

/// `XML_Content`: one node of an element declaration's content model.
#[repr(C)]
pub struct XML_Content {
    pub type_: c_int,
    pub quant: c_int,
    pub name: *mut XML_Char,
    pub numchildren: c_uint,
    pub children: *mut XML_Content,
}

/// `XML_Encoding`, filled in by an unknown-encoding handler.
#[repr(C)]
pub struct XML_Encoding {
    pub map: [c_int; 256],
    pub data: *mut c_void,
    pub convert: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
    pub release: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// `XML_Memory_Handling_Suite`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct XML_Memory_Handling_Suite {
    pub malloc_fcn: Option<unsafe extern "C" fn(usize) -> *mut c_void>,
    pub realloc_fcn: Option<unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void>,
    pub free_fcn: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// `XML_Expat_Version`.
#[repr(C)]
pub struct XML_Expat_Version {
    pub major: c_int,
    pub minor: c_int,
    pub micro: c_int,
}

/// `XML_Feature`.
#[repr(C)]
pub struct XML_Feature {
    pub feature: c_int,
    pub name: *const XML_LChar,
    pub value: c_long,
}

extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

// ─── the parser object ────────────────────────────────────────────────────

/// Handlers registered with `XML_Set*Handler`, each a copyable function
/// pointer in a `Cell` so handlers can be changed from inside a handler.
#[derive(Default)]
struct Handlers {
    start_element: Cell<StartElementHandler>,
    end_element: Cell<EndElementHandler>,
    character_data: Cell<CharacterDataHandler>,
    processing_instruction: Cell<ProcessingInstructionHandler>,
    comment: Cell<CommentHandler>,
    start_cdata: Cell<CdataSectionHandler>,
    end_cdata: Cell<CdataSectionHandler>,
    default: Cell<DefaultHandler>,
    default_expand: Cell<bool>,
    start_doctype: Cell<StartDoctypeDeclHandler>,
    end_doctype: Cell<EndDoctypeDeclHandler>,
    entity_decl: Cell<EntityDeclHandler>,
    unparsed_entity_decl: Cell<UnparsedEntityDeclHandler>,
    notation_decl: Cell<NotationDeclHandler>,
    start_namespace: Cell<StartNamespaceDeclHandler>,
    end_namespace: Cell<EndNamespaceDeclHandler>,
    not_standalone: Cell<NotStandaloneHandler>,
    external_entity_ref: Cell<ExternalEntityRefHandler>,
    skipped_entity: Cell<SkippedEntityHandler>,
    xml_decl: Cell<XmlDeclHandler>,
    element_decl: Cell<ElementDeclHandler>,
    attlist_decl: Cell<AttlistDeclHandler>,
}

impl Handlers {
    /// Take on another parser's handlers (an external entity parser starts
    /// with its parent's).
    fn copy_from(&self, other: &Handlers) {
        macro_rules! copy { ($($f:ident)*) => { $(self.$f.set(other.$f.get());)* } }
        copy!(start_element end_element character_data processing_instruction comment start_cdata
              end_cdata default default_expand start_doctype end_doctype entity_decl
              unparsed_entity_decl notation_decl start_namespace end_namespace not_standalone
              external_entity_ref skipped_entity xml_decl element_decl attlist_decl);
    }
}

/// A DOCTYPE's source text and how far into it the default handler has
/// been given what no other handler took.
struct DtdText {
    raw: String,
    /// Where it starts in the document (decoded byte offset).
    start: usize,
    /// Offset in `raw` up to which it has been dealt with.
    cursor: usize,
}

/// What kind of parser a `Shim` is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// From `XML_ParserCreate*`: parses a document.
    Root,
    /// From `XML_ExternalEntityParserCreate` with a context: parses an
    /// external entity's content.
    Entity,
    /// From `XML_ExternalEntityParserCreate` without a context: would parse
    /// an external DTD subset or parameter entity, which expat-capi doesn't
    /// read (as a non-validating parser may, XML 1.0 §5.1).
    Dtd,
}

/// Where parsing has got to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    /// No input yet: options may still change.
    NotStarted,
    Parsing,
    /// `isFinal` input parsed successfully.
    Finished,
    /// An error (or `XML_StopParser`) ended parsing; the code is in `error`.
    Failed,
}

/// The object an `XML_Parser` points to.
///
/// `#[repr(C)]` with the user data first: expat.h's `XML_GetUserData`
/// macro reads `*(void **)parser` directly.
#[repr(C)]
pub struct Shim {
    user_data: Cell<*mut c_void>,
    handlers: Handlers,
    /// `XML_SetUnknownEncodingHandler`'s handler and its data.
    unknown_encoding: Cell<(UnknownEncodingHandler, *mut c_void)>,
    /// Expanded names of the open elements, innermost last, for end tags.
    open: RefCell<Vec<CString>>,
    status: Cell<Status>,
    error: Cell<XML_Error>,
    /// Position reported by `XML_GetCurrent*`: the start of the event being
    /// delivered, the error, or the end of what has been parsed.
    pos: Cell<Position>,
    /// `pos` and `token_end` in input bytes (before decoding), for
    /// `XML_GetCurrentByteIndex` and `XML_GetCurrentByteCount`.
    input_pos: Cell<usize>,
    input_end: Cell<usize>,
    /// `XML_GetSpecifiedAttributeCount` for the current start tag.
    specified_atts: Cell<c_int>,
    /// Set by `XML_StopParser`; checked after every callback.
    stop_requested: Cell<bool>,
    /// Span of the event being delivered.
    current_span: Cell<Option<Span>>,
    /// Where the token being delivered ends: parsing stopped by
    /// `XML_StopParser` reports this position, as libexpat does.
    token_end: Cell<Position>,
    /// The root element hasn't started: parsing stopped by `XML_StopParser`
    /// before it (in the prolog, the DOCTYPE included) reports the position
    /// the handler was given, not `token_end`, as libexpat does.
    before_root: Cell<bool>,
    /// Stopped during the start of an empty element (`<e/>`): its end is
    /// still delivered, since libexpat finishes the token first.
    stop_in_empty_tag: Cell<bool>,
    /// Stopped in a CDATA section's start handler: libexpat still delivers
    /// the section's next token (the first piece of its text, or its end),
    /// and when none is in the input yet, `XML_Parse` succeeds and the next
    /// call finds the parser finished.
    stop_in_cdata_start: Cell<bool>,
    /// Stopped in a CDATA section's end handler: the stop reports the
    /// position the handler was given, as libexpat does.
    stop_at_handler: Cell<bool>,
    /// The event being handled came from expanding an entity reference.
    in_entity: Cell<bool>,
    /// The ATTLIST declaration last reported (its text's address and
    /// length, during one XML_Parse call), where each of its attributes is
    /// reported and where it ends in the input: worked out once for all its
    /// attributes, so that a long declaration costs no more than its length.
    attlist: RefCell<Option<AttlistPlaces>>,
    /// `XML_UseParserAsHandlerArg`: handlers get the parser, not user data.
    parser_as_arg: Cell<bool>,
    /// Source text of the event being delivered, for `XML_DefaultCurrent`
    /// (kept only while a default handler is set).
    current_raw: RefCell<String>,
    /// Attribute declared ID for each element (`XML_GetIdAttributeIndex`).
    id_attributes: RefCell<std::collections::HashMap<String, String>>,
    /// `XML_GetIdAttributeIndex` for the current start tag.
    id_index: Cell<c_int>,
    /// `standalone="yes"` in the XML declaration.
    standalone: Cell<bool>,
    role: Role,
    /// Entity children: what the parser for the entity's content needs.
    entity: Option<EntityContext>,
    /// Contexts of the external entity references reported to the
    /// handler, by the context string passed with them, for
    /// `XML_ExternalEntityParserCreate`; and the latest one.
    entity_contexts: RefCell<std::collections::HashMap<CString, EntityContext>>,
    last_entity_context: RefCell<Option<EntityContext>>,
    /// While a DOCTYPE is delivered and a default handler is set: its
    /// source text, so the parts no handler takes can go to the default
    /// handler.
    dtd_text: RefCell<Option<DtdText>>,
    /// The DOCTYPE's external identifiers, once seen: (system, public).
    doctype_ids: RefCell<Option<(Option<CString>, Option<CString>)>>,
    /// The foreign DTD (`XML_UseForeignDTD`) has been asked for.
    foreign_dtd_done: Cell<bool>,
    /// The start of the input (up to 1 KiB), from which the XML declaration
    /// is read early when the document's encoding is unknown, and whether
    /// that was delivered (see `early_xml_decl`).
    prolog: RefCell<Vec<u8>>,
    xml_decl_early: Cell<bool>,
    /// The first bytes of input, until it is known whether they are a
    /// byte order mark, and whether they were (libexpat counts a byte
    /// order mark as a column on line 1; expat-rs doesn't).
    head: RefCell<Vec<u8>>,
    bom: Cell<Option<bool>>,
    /// The input is UTF-16 (from a byte order mark or a zero byte at the
    /// start, as libexpat detects it).
    utf16: Cell<bool>,
    /// A UTF-8 byte order mark followed by a declaration of another
    /// encoding: libexpat counts its three bytes as three columns.
    bom_as_bytes: Cell<bool>,
    /// Character data is delivered in pieces of at most `TEXT_PIECE` bytes:
    /// libexpat does this for input it has to convert (anything but UTF-8
    /// and US-ASCII).
    chunk_text: Cell<bool>,
    /// Error to report when a handler stops parsing (`XML_StopParser` is
    /// XML_ERROR_ABORTED; a not-standalone handler returning 0 is
    /// XML_ERROR_NOT_STANDALONE).
    stop_error: Cell<XML_Error>,
    /// Namespace separator from `XML_ParserCreateNS` (`Some(0)`: names are
    /// concatenated without one).
    ns_sep: Option<u8>,
    ns_triplets: Cell<bool>,
    base: RefCell<Option<CString>>,
    encoding: RefCell<Option<String>>,
    reparse_deferral: Cell<bool>,
    amplification: Cell<AmplificationLimits>,
    param_entity_parsing: Cell<c_int>,
    use_foreign_dtd: Cell<bool>,
    memsuite: XML_Memory_Handling_Suite,
    /// The `XML_GetBuffer` buffer.
    buffer: RefCell<Vec<u8>>,
    /// The expat-rs parser, created at the first parse so that options set
    /// before then apply; borrowed only while parsing.
    core: RefCell<Option<StreamParser<'static>>>,
}

/// The `Shim` behind a parser pointer, or `None` for a null pointer.
///
/// # Safety
/// `parser` is null or was returned by `XML_ParserCreate*` and not freed.
unsafe fn shim<'a>(parser: XML_Parser) -> Option<&'a Shim> {
    // SAFETY: per the C API contract (see the module docs) a non-null
    // parser points to a live Shim; only shared references are ever made.
    unsafe { parser.as_ref() }
}

impl Shim {
    fn new(encoding: Option<String>, memsuite: Option<XML_Memory_Handling_Suite>, ns_sep: Option<u8>) -> Self {
        Shim {
            user_data: Cell::new(ptr::null_mut()),
            handlers: Handlers::default(),
            unknown_encoding: Cell::new((None, ptr::null_mut())),
            open: RefCell::new(Vec::new()),
            status: Cell::new(Status::NotStarted),
            error: Cell::new(XML_ERROR_NONE),
            pos: Cell::new(Position::start()),
            specified_atts: Cell::new(0),
            input_pos: Cell::new(0),
            input_end: Cell::new(0),
            stop_requested: Cell::new(false),
            current_span: Cell::new(None),
            token_end: Cell::new(Position::start()),
            before_root: Cell::new(true),
            stop_in_empty_tag: Cell::new(false),
            stop_in_cdata_start: Cell::new(false),
            stop_at_handler: Cell::new(false),
            in_entity: Cell::new(false),
            attlist: RefCell::new(None),
            parser_as_arg: Cell::new(false),
            current_raw: RefCell::new(String::new()),
            id_attributes: RefCell::new(std::collections::HashMap::new()),
            id_index: Cell::new(-1),
            standalone: Cell::new(false),
            role: Role::Root,
            entity: None,
            entity_contexts: RefCell::new(std::collections::HashMap::new()),
            last_entity_context: RefCell::new(None),
            doctype_ids: RefCell::new(None),
            dtd_text: RefCell::new(None),
            foreign_dtd_done: Cell::new(false),
            head: RefCell::new(Vec::new()),
            prolog: RefCell::new(Vec::new()),
            xml_decl_early: Cell::new(false),
            bom: Cell::new(None),
            utf16: Cell::new(false),
            bom_as_bytes: Cell::new(false),
            chunk_text: Cell::new(false),
            stop_error: Cell::new(XML_ERROR_ABORTED),
            ns_sep,
            ns_triplets: Cell::new(false),
            base: RefCell::new(None),
            encoding: RefCell::new(encoding),
            reparse_deferral: Cell::new(true),
            amplification: Cell::new(AmplificationLimits::default()),
            param_entity_parsing: Cell::new(0),
            use_foreign_dtd: Cell::new(false),
            memsuite: memsuite.unwrap_or(XML_Memory_Handling_Suite {
                malloc_fcn: Some(malloc),
                realloc_fcn: None,
                free_fcn: Some(free),
            }),
            buffer: RefCell::new(Vec::new()),
            core: RefCell::new(None),
        }
    }

    fn mem_malloc(&self, size: usize) -> *mut c_void {
        match self.memsuite.malloc_fcn {
            // SAFETY: the suite's malloc (or libc's) takes any size and
            // returns null or a block of at least `size` bytes.
            Some(f) => unsafe { f(size) },
            None => ptr::null_mut(),
        }
    }

    fn mem_free(&self, p: *mut c_void) {
        if let Some(f) = self.memsuite.free_fcn {
            // SAFETY: `p` came from the same suite's malloc (mem_malloc) or
            // is null, which free accepts.
            unsafe { f(p) }
        }
    }

    /// The expat-rs parser for this shim, configured from the options set
    /// so far.
    fn make_parser(&self) -> StreamParser<'static> {
        let mut p = match &self.entity {
            // Edition, version rule, namespaces and limits come from the
            // document.
            Some(context) => context.parser().with_reparse_deferral(self.reparse_deferral.get()).with_cdata_pieces(),
            None => {
                // libexpat applies the Fourth Edition's name rules
                // (Appendix B) but accepts any version number made of name
                // characters.
                let mut p = StreamParser::new()
                    .with_edition(Edition::Fourth)
                    .with_libexpat_leniency()
                    .with_external_entity_refs()
                    .with_outside_whitespace()
                    .with_cdata_pieces()
                    .with_reparse_deferral(self.reparse_deferral.get())
                    .with_amplification_limits(self.amplification.get());
                if self.ns_sep.is_some() {
                    p = p.with_namespaces();
                }
                p
            }
        };
        if let Some(enc) = self.encoding.borrow().as_deref() {
            p = p.with_encoding(enc);
            if enc.to_ascii_uppercase().starts_with("UTF-16") {
                self.utf16.set(true);
            }
        }
        self.chunk_text.set(self.converts(None));
        let (handler, data) = self.unknown_encoding.get();
        let this: *const Shim = self;
        p.with_unknown_encoding(move |name| {
            // SAFETY: the parser this closure belongs to is owned by the
            // Shim at `this` (in `core`), which is heap-allocated and not
            // moved, and calls it only while parsing, inside a call on that
            // Shim; only shared access is used.
            unsafe { &*this }.early_xml_decl();
            handler.and_then(|handler| byte_map_from_c(handler, data, name))
        })
    }

    /// Feed input (and finish it when `is_final`), delivering callbacks.
    fn parse(&self, input: &[u8], is_final: bool) -> XML_Status {
        match self.status.get() {
            Status::Failed => return XML_STATUS_ERROR,
            Status::Finished => {
                self.error.set(XML_ERROR_FINISHED);
                self.status.set(Status::Failed);
                return XML_STATUS_ERROR;
            }
            _ => {}
        }
        if self.role == Role::Dtd {
            // External DTD subsets aren't read (see `Role::Dtd`).
            self.status.set(if is_final { Status::Finished } else { Status::Parsing });
            return XML_STATUS_OK;
        }
        let Ok(mut core) = self.core.try_borrow_mut() else {
            // XML_Parse from inside one of this parser's own handlers.
            return XML_STATUS_ERROR;
        };
        self.note_bom(input, is_final);
        if let Ok(mut prolog) = self.prolog.try_borrow_mut() {
            let room = 1024usize.saturating_sub(prolog.len());
            prolog.extend(input.iter().take(room));
        }
        let parser = core.get_or_insert_with(|| self.make_parser());
        self.status.set(Status::Parsing);
        let mut deliver = |l: Located<'_>| {
            if !self.stop_requested.get() {
                self.dispatch(l);
            } else if self.stop_in_empty_tag.get()
                && matches!(l.event, Event::EndElement(_))
                && Some(l.span) == self.current_span.get()
            {
                // The rest of the empty-element token the stop came in.
                self.stop_in_empty_tag.set(false);
                self.dispatch(l);
            } else if self.stop_in_cdata_start.get() && matches!(l.event, Event::CData(_) | Event::EndCData) {
                self.stop_in_cdata_start.set(false);
                match &l.event {
                    Event::CData(text) => self.cdata_first_token(text, l.raw, l.span.start, l.input_start),
                    _ => self.dispatch(l),
                }
            }
        };
        let mut result = parser.feed_located(input, &mut deliver);
        if result.is_ok() && is_final && !self.stop_requested.get() {
            result = parser.finish_located(&mut deliver);
        }
        // Its key is an address in this call's text.
        self.attlist.borrow_mut().take();
        let parsed_to = parser.position();
        let parsed_input = parser.input_offset(parsed_to);
        let error_input = result.as_ref().err().map(|e| parser.input_offset(e.position()));
        drop(core);
        if self.stop_requested.get() {
            if self.stop_in_cdata_start.replace(false) {
                // Nothing of the section came after the stop in this input.
                self.pos.set(self.token_end.get());
                self.input_pos.set(self.input_end.get());
                self.status.set(Status::Finished);
                return XML_STATUS_OK;
            }
            let at_handler = self.stop_at_handler.replace(false);
            // Within an entity's replacement text libexpat stays at the
            // reference.
            if !(at_handler || self.in_entity.get() || matches!(self.role, Role::Root) && self.before_root.get()) {
                self.pos.set(self.token_end.get());
                self.input_pos.set(self.input_end.get());
            }
            self.error.set(self.stop_error.get());
            // Stopped is finished, as in libexpat: a later call reports
            // XML_ERROR_FINISHED.
            self.status.set(Status::Finished);
            return XML_STATUS_ERROR;
        }
        match result {
            Ok(()) => {
                self.pos.set(parsed_to);
                self.input_pos.set(parsed_input);
                if is_final {
                    self.status.set(Status::Finished);
                }
                XML_STATUS_OK
            }
            Err(e) => {
                self.fail(&e);
                self.input_pos.set(error_input.unwrap_or(parsed_input));
                XML_STATUS_ERROR
            }
        }
    }

    /// Whether libexpat would convert this input to UTF-8 (anything but
    /// UTF-8 and US-ASCII), given the declared encoding, if any.
    fn converts(&self, declared: Option<&str>) -> bool {
        let forced = self.encoding.borrow().clone();
        match forced.as_deref().or(declared) {
            _ if self.utf16.get() && forced.is_none() => true,
            None => self.utf16.get(),
            Some(enc) => !["UTF-8", "US-ASCII", "ASCII"].iter().any(|n| enc.eq_ignore_ascii_case(n)),
        }
    }

    /// The document declares an encoding expat-capi doesn't know: as
    /// libexpat does, deliver the XML declaration (ASCII in any encoding
    /// that could work) before the unknown-encoding handler is asked, and
    /// whether or not one is set. Not for an encoding the caller chose,
    /// which is settled before any declaration is read.
    fn early_xml_decl(&self) {
        if self.encoding.borrow().is_some() || self.xml_decl_early.get() {
            return;
        }
        let Some(f) = self.handlers.xml_decl.get() else { return };
        let Ok(prolog) = self.prolog.try_borrow() else { return };
        let Some((version, encoding, standalone)) = parse_xml_decl(&prolog) else { return };
        drop(prolog);
        self.xml_decl_early.set(true);
        let (v, e) = (opt_cstr(version.as_deref()), opt_cstr(encoding.as_deref()));
        self.pos.set(Position::start());
        self.input_pos.set(0);
        // SAFETY: registered XmlDeclHandler; valid C strings (or null).
        unsafe { f(self.ud(), ptr_of(&v), ptr_of(&e), standalone) };
    }

    /// Watch the first bytes of input for a byte order mark.
    fn note_bom(&self, input: &[u8], is_final: bool) {
        if self.bom.get().is_some() {
            return;
        }
        let Ok(mut head) = self.head.try_borrow_mut() else { return };
        let want = 3usize.saturating_sub(head.len());
        head.extend(input.iter().take(want));
        let utf8 = [0xEF, 0xBB, 0xBF];
        if let [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] | [0x00, _, ..] | [_, 0x00, ..] = head.as_slice() {
            self.utf16.set(true);
        }
        let decided = match head.as_slice() {
            [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..] => Some(true),
            h if h.len() >= 3 || is_final => Some(h.starts_with(&utf8)),
            h if utf8.starts_with(h) => None, // may still be one
            _ => Some(false),
        };
        self.bom.set(decided);
    }

    /// libexpat's column for `pos`: 0-based, and counting a byte order mark
    /// on line 1: as one column, or as three when a UTF-8 mark is followed
    /// by a declaration of another encoding (libexpat reads its bytes in
    /// that encoding).
    fn column(&self, pos: Position) -> XML_Size {
        let bom = match (pos.line, self.bom.get()) {
            (1, Some(true)) if self.bom_as_bytes.get() => 3,
            (1, Some(true)) => 1,
            _ => 0,
        };
        (pos.column.saturating_sub(1) + bom) as XML_Size
    }

    fn fail(&self, e: &XmlError) {
        self.pos.set(e.position());
        self.error.set(error_code(e.kind()));
        self.status.set(Status::Failed);
    }
}

/// Ask a C unknown-encoding handler for an encoding map.
fn byte_map_from_c(handler: unsafe extern "C" fn(*mut c_void, *const XML_Char, *mut XML_Encoding) -> c_int,
                   data: *mut c_void, name: &str) -> Option<EncodingMap> {
    let name = CString::new(name).ok()?;
    let mut info = XML_Encoding { map: [-1; 256], data: ptr::null_mut(), convert: None, release: None };
    // SAFETY: the handler was registered for this purpose with `data`;
    // `name` is a valid C string and `info` a valid, writable XML_Encoding
    // for the duration of the call.
    let ok = unsafe { handler(data, name.as_ptr(), &mut info) };
    // Owns the handler's data from here: released when dropped, after the
    // map's last use (or now, if the map isn't used).
    let converter = Converter { data: info.data, convert: info.convert, release: info.release };
    if ok != XML_STATUS_OK || !acceptable_map(&info.map, converter.convert.is_some()) {
        return None;
    }
    let entries = info.map.map(|entry| match entry {
        -4..=-2 => MapEntry::Lead((-entry) as u8),
        _ => u32::try_from(entry).ok().and_then(char::from_u32).map_or(MapEntry::Invalid, MapEntry::Char),
    });
    // The converter goes with the map whenever it has something to do,
    // decoding sequences or releasing the data when the parser is done.
    if converter.release.is_some() || entries.iter().any(|e| matches!(e, MapEntry::Lead(_))) {
        Some(EncodingMap::multi_byte(entries, move |sequence| converter.convert(sequence)))
    } else {
        Some(EncodingMap::single_byte(entries.map(|e| match e {
            MapEntry::Char(c) => Some(c),
            _ => None,
        })))
    }
}

/// An unknown-encoding handler's `convert` callback with its data, which
/// its `release` callback frees when this is dropped.
struct Converter {
    data: *mut c_void,
    convert: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
    release: Option<unsafe extern "C" fn(*mut c_void)>,
}

// SAFETY: the converter is used only by the parser it was made for, which
// a C program uses from one thread at a time, as libexpat requires; the
// data pointer is the handler's, valid until `release`.
unsafe impl Send for Converter {}

impl Converter {
    /// The character a multi-byte sequence stands for. libexpat accepts
    /// only results up to U+FFFF from `convert`; anything else (or -1) is an
    /// invalid sequence. So is an ASCII result: libexpat takes a sequence
    /// as data whatever it decodes to, so one decoding to `<`, a quote or a
    /// keyword's letter never acts as markup there; once decoded, expat-rs
    /// couldn't tell it from markup, so it refuses it instead.
    fn convert(&self, sequence: &[u8]) -> Option<char> {
        let convert = self.convert?;
        // SAFETY: `convert` gets the data the handler gave it and a pointer
        // to a whole sequence of the length its map entry declared.
        let c = unsafe { convert(self.data, sequence.as_ptr().cast()) };
        u32::try_from(c).ok().filter(|&c| (0x80..=0xFFFF).contains(&c)).and_then(char::from_u32)
    }
}

impl Drop for Converter {
    fn drop(&mut self) {
        if let Some(release) = self.release {
            // SAFETY: release takes the data pointer the handler stored, once.
            unsafe { release(self.data) };
        }
    }
}

/// The pseudo-attributes of an XML declaration at the start of `bytes`:
/// (version, encoding, standalone as libexpat passes it: 1, 0 or -1).
fn parse_xml_decl(bytes: &[u8]) -> Option<(Option<String>, Option<String>, c_int)> {
    let text = String::from_utf8_lossy(bytes);
    let body = text.strip_prefix("<?xml")?;
    let body = &body[..body.find("?>")?];
    let (mut version, mut encoding, mut standalone) = (None, None, -1);
    let mut rest = body.trim_start();
    while !rest.is_empty() {
        let (name, after) = rest.split_once('=')?;
        let after = after.trim_start();
        let quote = after.chars().next().filter(|q| matches!(q, '"' | '\''))?;
        let (value, after) = after[1..].split_once(quote)?;
        match name.trim() {
            "version" => version = Some(value.to_string()),
            "encoding" => encoding = Some(value.to_string()),
            "standalone" => standalone = (value == "yes") as c_int,
            _ => return None,
        }
        rest = after.trim_start();
    }
    Some((version, encoding, standalone))
}

/// Length of a DOCTYPE's header: through the `[` that opens its internal
/// subset (outside quoted literals), or all of it.
fn doctype_header_len(raw: &str) -> usize {
    let mut quote = None;
    for (i, c) in raw.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => return i + 1,
            _ => {}
        }
    }
    raw.len()
}

/// See `Parser::attlist`.
struct AttlistPlaces {
    key: (usize, usize),
    places: HashMap<String, (Position, usize)>,
    end: usize,
}

/// Whether `raw` is one general entity reference (not a character reference
/// or a predefined entity, which libexpat treats as ordinary tokens): the
/// events from its replacement text all carry it.
fn is_entity_reference(raw: &str) -> bool {
    match raw.strip_prefix('&').and_then(|r| r.strip_suffix(';')) {
        Some(name) => {
            !name.is_empty()
                && !name.starts_with('#')
                && !name.contains(['&', ';'])
                && !matches!(name, "amp" | "lt" | "gt" | "apos" | "quot")
        }
        None => false,
    }
}

/// Whether `text` is `raw` with its line ends normalised: character data
/// from the source, not from an entity's replacement text.
fn is_source(text: &str, raw: &str) -> bool {
    if raw.contains('\r') {
        raw.replace("\r\n", "\n").replace('\r', "\n") == text
    } else {
        raw == text
    }
}

/// CDATA text in libexpat's tokens, which also end before each `]` (which
/// may start the closing `]]>`): each part after the first starts with `]`.
fn cdata_parts(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut from = 0;
    for (i, _) in s.match_indices(']') {
        if i > from {
            parts.push(&s[from..i]);
            from = i;
        }
    }
    parts.push(&s[from..]);
    parts
}

/// Where libexpat reports a DTD event's position, as a byte offset into
/// its source text: at the token that completes what the handler is told.
/// The DOCTYPE starts at its `[` or closing `>`, and ends at the closing
/// `>`; an internal entity at its value, an unparsed one at its notation's
/// name, an external one at the closing `>`; a notation at its system
/// literal, or the closing `>` without one; an element declaration at the
/// last token of its content model; each attribute of an ATTLIST at its
/// default (`#IMPLIED`, `#REQUIRED` or the value). `None` for other events,
/// which libexpat reports where they start. (Found by black-box tests of
/// libexpat 2.8.2.)
fn decl_trigger(event: &Event<'_>, raw: &str) -> Option<usize> {
    match event {
        Event::Doctype { .. } => return doctype_header_len(raw).checked_sub(1),
        Event::EndDoctype => return raw.ends_with('>').then(|| raw.len() - 1),
        Event::EntityDecl { .. } | Event::NotationDecl { .. } | Event::ElementDecl { .. } | Event::AttlistDecl { .. } => {}
        _ => return None,
    }
    let tokens: Vec<&str> = dtd_tokens(raw).into_iter().filter(|t| !t.starts_with([' ', '\t', '\r', '\n'])).collect();
    let (&close, body) = tokens.split_last().filter(|(close, _)| **close == ">")?;
    let last = body.last().copied();
    let token = match event {
        Event::EntityDecl { system_id: Some(_), notation: None, .. } => close,
        Event::EntityDecl { .. } | Event::ElementDecl { .. } => last?,
        Event::NotationDecl { system_id, .. } => if system_id.is_some() { last? } else { close },
        // Each attribute's: see `Parser::attlist_trigger`.
        Event::AttlistDecl { .. } => return None,
        _ => return None,
    };
    Some(token.as_ptr() as usize - raw.as_ptr() as usize)
}

/// Where libexpat reports each attribute of an ATTLIST declaration (`raw`):
/// at its default, as an offset into `raw`, in order. After `<!ATTLIST` and
/// the element's name, each definition is a name, a type (a keyword, an
/// enumeration in parentheses, or `NOTATION` and one), then `#IMPLIED`,
/// `#REQUIRED`, or a value with or without `#FIXED`. The first definition of
/// a name, as libexpat keeps, is the first in the list.
fn attlist_defaults(raw: &str) -> Vec<(&str, usize)> {
    let mut defaults = Vec::new();
    let tokens: Vec<&str> = dtd_tokens(raw).into_iter().filter(|t| !t.starts_with([' ', '\t', '\r', '\n'])).collect();
    let Some((_, tokens)) = tokens.split_last().filter(|(close, _)| **close == ">") else {
        return defaults;
    };
    let mut i = 2;
    while i < tokens.len() {
        let name = tokens[i];
        i += 1;
        if tokens.get(i).is_some_and(|t| t.eq_ignore_ascii_case("NOTATION")) {
            i += 1;
        }
        if tokens.get(i) == Some(&"(") {
            while tokens.get(i).is_some_and(|t| !t.starts_with(')')) {
                i += 1;
            }
        }
        i += 1;
        if tokens.get(i) == Some(&"#FIXED") {
            i += 1;
        }
        let Some(default) = tokens.get(i) else { break };
        i += 1;
        defaults.push((name, default.as_ptr() as usize - raw.as_ptr() as usize));
    }
    defaults
}

/// Split DTD markup into the tokens libexpat gives its default handler:
/// `<!KEYWORD`, whitespace runs, names (with `#` keywords and a trailing
/// `?*+`), quoted literals, `( | , [ ] >`, `)` with a trailing `?*+`,
/// `%` alone or a whole `%name;` reference, and whole comments and
/// processing instructions. (Token boundaries found by black-box tests of
/// libexpat.)
fn dtd_tokens(s: &str) -> Vec<&str> {
    let is_name = |c: char| c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '#') || !c.is_ascii();
    let mut tokens = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        let len = if rest.starts_with("<!--") {
            rest.find("-->").map_or(rest.len(), |i| i + 3)
        } else if rest.starts_with("<?") {
            rest.find("?>").map_or(rest.len(), |i| i + 2)
        } else if let Some(keyword) = rest.strip_prefix("<!") {
            2 + keyword.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(keyword.len())
        } else if matches!(c, ' ' | '\t' | '\r' | '\n') {
            rest.find(|c: char| !matches!(c, ' ' | '\t' | '\r' | '\n')).unwrap_or(rest.len())
        } else if c == '"' || c == '\'' {
            rest[1..].find(c).map_or(rest.len(), |i| i + 2)
        } else if c == '%' {
            match rest[1..].chars().next() {
                Some(n) if is_name(n) => rest.find(';').map_or(rest.len(), |i| i + 1),
                _ => 1,
            }
        } else if c == ')' || is_name(c) {
            let name = if c == ')' { 1 } else { rest.find(|c: char| !is_name(c)).unwrap_or(rest.len()) };
            name + rest[name..].starts_with(['?', '*', '+']) as usize
        } else {
            c.len_utf8()
        };
        tokens.push(&rest[..len]);
        rest = &rest[len..];
    }
    tokens
}

/// Largest piece of character data libexpat delivers for input it
/// converts to UTF-8, in bytes.
const TEXT_PIECE: usize = 1024;

/// ASCII characters with a meaning in XML's syntax: whitespace, letters,
/// digits and markup punctuation.
const XML_SYNTAX_CHARS: &[u8] =
    b"\t\n\r !\"#%&'()*+,-./0123456789:;<=>?ABCDEFGHIJKLMNOPQRSTUVWXYZ[]_abcdefghijklmnopqrstuvwxyz|";

/// Whether libexpat would accept an unknown-encoding handler's map: each
/// of XML's syntax characters is its own byte and no other byte stands for
/// one; nothing maps above U+FFFF; multi-byte sequences (entries -2 to -4,
/// the sequence length) only with a `convert` callback, and nothing below -4. Other maps are an
/// unknown encoding. (Found by black-box tests of libexpat: remapping `$`
/// or a control byte is accepted, `A`, `<` or LF is not.)
fn acceptable_map(map: &[c_int; 256], has_convert: bool) -> bool {
    let is_syntax = |c: c_int| u8::try_from(c).is_ok_and(|c| XML_SYNTAX_CHARS.contains(&c));
    map.iter().enumerate().all(|(byte, &entry)| {
        let byte = byte as c_int;
        if entry < -1 {
            // A sequence lead: needs `convert`, and may not be a syntax byte.
            (-4..=-2).contains(&entry) && has_convert && !is_syntax(byte)
        } else if is_syntax(byte) {
            entry == byte // a syntax character must be its own byte
        } else {
            // No other byte may stand for one, nor for anything above U+FFFF.
            !is_syntax(entry) && entry <= 0xFFFF
        }
    })
}

/// libexpat's error code for an expat-rs error kind.
fn error_code(kind: ErrorKind) -> XML_Error {
    match kind {
        ErrorKind::Syntax => XML_ERROR_SYNTAX,
        ErrorKind::NoElements => XML_ERROR_NO_ELEMENTS,
        ErrorKind::InvalidToken => XML_ERROR_INVALID_TOKEN,
        ErrorKind::UnclosedToken => XML_ERROR_UNCLOSED_TOKEN,
        ErrorKind::PartialChar => XML_ERROR_PARTIAL_CHAR,
        ErrorKind::TagMismatch => XML_ERROR_TAG_MISMATCH,
        ErrorKind::DuplicateAttribute => XML_ERROR_DUPLICATE_ATTRIBUTE,
        ErrorKind::JunkAfterDocElement => XML_ERROR_JUNK_AFTER_DOC_ELEMENT,
        ErrorKind::ParamEntityRef => XML_ERROR_PARAM_ENTITY_REF,
        ErrorKind::UndefinedEntity => XML_ERROR_UNDEFINED_ENTITY,
        ErrorKind::RecursiveEntityRef => XML_ERROR_RECURSIVE_ENTITY_REF,
        ErrorKind::AsyncEntity => XML_ERROR_ASYNC_ENTITY,
        ErrorKind::BadCharRef => XML_ERROR_BAD_CHAR_REF,
        ErrorKind::BinaryEntityRef => XML_ERROR_BINARY_ENTITY_REF,
        ErrorKind::AttributeExternalEntityRef => XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF,
        ErrorKind::MisplacedXmlPi => XML_ERROR_MISPLACED_XML_PI,
        ErrorKind::UnknownEncoding => XML_ERROR_UNKNOWN_ENCODING,
        ErrorKind::IncorrectEncoding => XML_ERROR_INCORRECT_ENCODING,
        ErrorKind::UnclosedCdataSection => XML_ERROR_UNCLOSED_CDATA_SECTION,
        ErrorKind::ExternalEntityHandling => XML_ERROR_EXTERNAL_ENTITY_HANDLING,
        ErrorKind::XmlDecl => XML_ERROR_XML_DECL,
        ErrorKind::TextDecl => XML_ERROR_TEXT_DECL,
        ErrorKind::PublicId => XML_ERROR_PUBLICID,
        ErrorKind::AmplificationLimitBreach => XML_ERROR_AMPLIFICATION_LIMIT_BREACH,
        ErrorKind::UnboundPrefix => XML_ERROR_UNBOUND_PREFIX,
        ErrorKind::UndeclaringPrefix => XML_ERROR_UNDECLARING_PREFIX,
        ErrorKind::ReservedPrefixXml => XML_ERROR_RESERVED_PREFIX_XML,
        ErrorKind::ReservedPrefixXmlns => XML_ERROR_RESERVED_PREFIX_XMLNS,
        ErrorKind::ReservedNamespaceUri => XML_ERROR_RESERVED_NAMESPACE_URI,
        ErrorKind::Finished => XML_ERROR_FINISHED,
        ErrorKind::InvalidArgument => XML_ERROR_INVALID_ARGUMENT,
    }
}

// ─── event dispatch ───────────────────────────────────────────────────────

/// A C string for a callback argument (XML text never contains NUL).
fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

fn opt_cstr(s: Option<&str>) -> Option<CString> {
    s.map(cstr)
}

fn ptr_of(s: &Option<CString>) -> *const c_char {
    s.as_ref().map_or(ptr::null(), |s| s.as_ptr())
}

impl Shim {
    /// The first argument for handlers.
    fn ud(&self) -> *mut c_void {
        if self.parser_as_arg.get() {
            (self as *const Shim).cast_mut().cast()
        } else {
            self.user_data.get()
        }
    }

    /// The name a namespace-processing libexpat reports: `uri<sep>local`,
    /// with `<sep>prefix` added when triplets are on; names in no namespace
    /// stay as written.
    fn expanded_name(&self, qname: &str, namespace: Option<&str>) -> CString {
        match (self.ns_sep, namespace) {
            (Some(sep), Some(uri)) => {
                let (prefix, local) = match qname.split_once(':') {
                    Some((p, l)) => (Some(p), l),
                    None => (None, qname),
                };
                let mut s = Vec::with_capacity(uri.len() + qname.len() + 2);
                s.extend_from_slice(uri.as_bytes());
                if sep != 0 {
                    s.push(sep);
                }
                s.extend_from_slice(local.as_bytes());
                if let (true, Some(prefix)) = (self.ns_triplets.get(), prefix) {
                    if sep != 0 {
                        s.push(sep);
                    }
                    s.extend_from_slice(prefix.as_bytes());
                }
                CString::new(s).unwrap_or_default()
            }
            _ => cstr(qname),
        }
    }

    /// Deliver `raw` to the default handler, if one is set; returns whether
    /// it was.
    fn to_default(&self, raw: &str) -> bool {
        match self.handlers.default.get() {
            Some(f) => {
                // SAFETY: a registered DefaultHandler; `raw` is valid for
                // `raw.len()` bytes during the call (libexpat passes text
                // that is not NUL-terminated, with its length).
                unsafe { f(self.ud(), raw.as_ptr().cast(), raw.len() as c_int) };
                true
            }
            None => false,
        }
    }

    /// Deliver one expat-rs event as libexpat callbacks.
    fn dispatch(&self, l: Located<'_>) {
        // A declaration's handlers are told where the token completing it
        // is, not where it starts.
        let (pos, input_pos, input_end) = match &l.event {
            Event::AttlistDecl { attribute, .. } => self.attlist_place(&l, attribute),
            event => {
                let at = decl_trigger(event, l.raw).unwrap_or(0);
                let before = &l.raw[..at];
                (
                    l.span.start.advance(before),
                    l.input_start + self.input_len(before),
                    l.input_start + self.input_len(l.raw),
                )
            }
        };
        self.pos.set(pos);
        self.current_span.set(Some(l.span));
        self.token_end.set(l.span.end);
        self.input_pos.set(input_pos);
        self.input_end.set(input_end);
        self.in_entity.set(is_entity_reference(l.raw));
        if matches!(l.event, Event::StartElement { .. }) {
            self.before_root.set(false);
        }
        if self.handlers.default.get().is_some() {
            if let Ok(mut raw) = self.current_raw.try_borrow_mut() {
                raw.clear();
                raw.push_str(l.raw);
            }
        }
        match &l.event {
            Event::XmlDecl(d) => {
                self.standalone.set(d.standalone == Some(true));
                let other = d.encoding.is_some_and(|e| !e.eq_ignore_ascii_case("UTF-8"));
                self.bom_as_bytes.set(self.bom.get() == Some(true) && !self.utf16.get() && other && self.encoding.borrow().is_none());
                self.chunk_text.set(self.converts(d.encoding));
            }
            Event::Doctype { system_id, public_id, .. } => {
                *self.doctype_ids.borrow_mut() = Some((opt_cstr(system_id.as_deref()), opt_cstr(public_id.as_deref())));
                // An external subset: libexpat asks before the DOCTYPE
                // handler.
                if system_id.is_some() || public_id.is_some() {
                    self.not_standalone();
                    if self.stop_requested.get() {
                        return;
                    }
                }
            }
            // A document without a DOCTYPE asks for its foreign DTD before
            // the root element.
            Event::StartElement { .. } if self.role == Role::Root && self.doctype_ids.borrow().is_none() => {
                self.external_dtd(None);
                if self.stop_requested.get() {
                    return;
                }
            }
            _ => {}
        }
        // A handler called for the DOCTYPE may stop parsing; a stop from
        // before (finishing an empty element's tag) still lets this through.
        let stopped = self.stop_requested.get();
        self.dtd_default(&l);
        if !stopped && self.stop_requested.get() {
            return;
        }
        let h = &self.handlers;
        let ud = self.ud();
        match l.event {
            // Delivered already, ahead of the unknown-encoding handler.
            Event::XmlDecl(_) if self.xml_decl_early.get() => {}
            Event::XmlDecl(d) => match h.xml_decl.get() {
                Some(f) => {
                    // A text declaration may have no version: NULL, as in
                    // libexpat.
                    let (v, e) = (opt_cstr(Some(d.version).filter(|v| !v.is_empty())), opt_cstr(d.encoding));
                    let standalone = d.standalone.map_or(-1, |s| s as c_int);
                    // SAFETY: registered XmlDeclHandler; string arguments are
                    // valid C strings (or null) for the call.
                    unsafe { f(ud, ptr_of(&v), ptr_of(&e), standalone) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            Event::Doctype { name, system_id, public_id, has_internal_subset, .. } => {
                // Without a handler, the default handler has had it token
                // by token (`dtd_default`).
                if let Some(f) = h.start_doctype.get() {
                    let (n, s, p) = (cstr(name), opt_cstr(system_id.as_deref()), opt_cstr(public_id.as_deref()));
                    // SAFETY: registered StartDoctypeDeclHandler; valid C
                    // strings (or null) for the call.
                    unsafe { f(ud, n.as_ptr(), ptr_of(&s), ptr_of(&p), has_internal_subset as c_int) };
                }
            }
            Event::Whitespace(_) => {
                self.to_default(l.raw);
            }
            Event::EndDoctype => {
                // The external subset (or a foreign DTD) is asked for after
                // the internal subset, as in libexpat.
                let ids = self.doctype_ids.borrow().clone();
                self.external_dtd(ids);
                if self.stop_requested.get() {
                    return;
                }
                if let Some(f) = h.end_doctype.get() {
                    // SAFETY: registered EndDoctypeDeclHandler.
                    unsafe { f(ud) };
                }
            }
            Event::StartElement { name, namespace, attributes } => {
                self.specified_atts.set(2 * attributes.iter().filter(|a| a.specified).count() as c_int);
                let id = self.id_attributes.try_borrow().ok().and_then(|ids| {
                    let id = ids.get(name.as_ref())?;
                    attributes.iter().position(|a| a.name == id.as_str())
                });
                self.id_index.set(id.map_or(-1, |i| 2 * i as c_int));
                let n = self.expanded_name(&name, namespace.as_deref());
                if let Ok(mut open) = self.open.try_borrow_mut() {
                    open.push(n.clone());
                }
                match h.start_element.get() {
                    Some(f) => {
                        let strings: Vec<CString> = attributes
                            .iter()
                            .flat_map(|a| [self.expanded_name(&a.name, a.namespace.as_deref()), cstr(&a.value)])
                            .collect();
                        let mut atts: Vec<*const c_char> = strings.iter().map(|s| s.as_ptr()).collect();
                        atts.push(ptr::null());
                        // SAFETY: registered StartElementHandler; the name and
                        // the null-terminated name/value array of C strings
                        // stay valid for the call.
                        unsafe { f(ud, n.as_ptr(), atts.as_mut_ptr()) };
                        if self.stop_requested.get() && l.raw.ends_with("/>") {
                            self.stop_in_empty_tag.set(true);
                        }
                    }
                    None => {
                        self.to_default(l.raw);
                    }
                }
            }
            Event::EndElement(name) => {
                // libexpat reports the end with the start tag's expanded
                // name; expat-rs's EndElement carries the QName.
                let n = self.open.try_borrow_mut().ok().and_then(|mut o| o.pop()).unwrap_or_else(|| cstr(&name));
                let empty_tag = l.raw.starts_with('<') && l.raw.ends_with("/>");
                if empty_tag && h.start_element.get().is_some() {
                    // An empty element's end comes at the end of its tag
                    // once the start has gone to a start handler.
                    self.pos.set(l.span.end);
                    self.input_pos.set(self.input_end.get());
                }
                match h.end_element.get() {
                    // SAFETY: registered EndElementHandler; valid C string.
                    Some(f) => unsafe { f(ud, n.as_ptr()) },
                    // An empty element's tag already went to the default
                    // handler with its start.
                    None if !empty_tag => {
                        self.to_default(l.raw);
                    }
                    None => {}
                }
            }
            Event::Text(text) => self.character_data(&text, l.raw, l.span.start, l.input_start),
            // CDATA sections come in pieces (`with_cdata_pieces`), their
            // text as far as the input goes, as libexpat delivers it.
            Event::StartCData => {
                match h.start_cdata.get() {
                    // SAFETY: registered StartCdataSectionHandler.
                    Some(f) => unsafe { f(ud) },
                    None => {
                        self.to_default(l.raw);
                    }
                }
                if self.stop_requested.get() {
                    self.stop_in_cdata_start.set(true);
                }
            }
            Event::CData(text) => self.cdata_text(&text, l.raw, l.span.start, l.input_start),
            Event::EndCData => {
                match h.end_cdata.get() {
                    // SAFETY: registered EndCdataSectionHandler.
                    Some(f) => unsafe { f(ud) },
                    None => {
                        self.to_default(l.raw);
                    }
                }
                if self.stop_requested.get() {
                    self.stop_at_handler.set(true);
                }
            }
            Event::Comment(text) => match h.comment.get() {
                Some(f) => {
                    let t = cstr(&text);
                    // SAFETY: registered CommentHandler; valid C string.
                    unsafe { f(ud, t.as_ptr()) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            Event::ProcessingInstruction { target, body } => match h.processing_instruction.get() {
                Some(f) => {
                    let (t, b) = (cstr(&target), cstr(&body));
                    // SAFETY: registered ProcessingInstructionHandler; valid
                    // C strings.
                    unsafe { f(ud, t.as_ptr(), b.as_ptr()) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            Event::NotationDecl { name, public_id, system_id } => {
                if let Some(f) = h.notation_decl.get() {
                    let base = self.base.borrow().clone();
                    let (n, s, p) = (cstr(&name), opt_cstr(system_id.as_deref()), opt_cstr(public_id.as_deref()));
                    // SAFETY: registered NotationDeclHandler; valid C strings
                    // (or null).
                    unsafe { f(ud, n.as_ptr(), ptr_of(&base), ptr_of(&s), ptr_of(&p)) };
                }
            }
            Event::ElementDecl { name, model } => {
                if let Some(f) = h.element_decl.get() {
                    let content = self.build_content(&model);
                    if !content.is_null() {
                        let n = cstr(&name);
                        // SAFETY: registered ElementDeclHandler; `content` is
                        // a complete XML_Content tree from the memory suite,
                        // which the handler owns and frees with
                        // XML_FreeContentModel.
                        unsafe { f(ud, n.as_ptr(), content) };
                    }
                }
            }
            Event::AttlistDecl { element, attribute, att_type, default, required } => {
                if att_type == "ID" {
                    if let Ok(mut ids) = self.id_attributes.try_borrow_mut() {
                        // The first ID declaration for an element counts.
                        ids.entry(element.to_string()).or_insert_with(|| attribute.to_string());
                    }
                }
                if let Some(f) = h.attlist_decl.get() {
                    let (e, a, t, d) = (cstr(&element), cstr(&attribute), cstr(&att_type), opt_cstr(default.as_deref()));
                    // SAFETY: registered AttlistDeclHandler; valid C strings
                    // (or null).
                    unsafe { f(ud, e.as_ptr(), a.as_ptr(), t.as_ptr(), ptr_of(&d), required as c_int) };
                }
            }
            Event::EntityDecl { name, parameter, value, system_id, public_id, notation } => {
                let base = self.base.borrow().clone();
                let (n, s, p, no) =
                    (cstr(&name), opt_cstr(system_id.as_deref()), opt_cstr(public_id.as_deref()), opt_cstr(notation.as_deref()));
                // An unparsed (NDATA) entity goes to the unparsed-entity
                // handler when there is one, as in libexpat; otherwise, like
                // every other entity, to the entity handler.
                if let (Some(f), Some(_)) = (h.unparsed_entity_decl.get(), &no) {
                    // SAFETY: registered UnparsedEntityDeclHandler; valid C
                    // strings (or null).
                    unsafe { f(ud, n.as_ptr(), ptr_of(&base), ptr_of(&s), ptr_of(&p), ptr_of(&no)) };
                } else if let Some(f) = h.entity_decl.get() {
                    let (vptr, vlen) = value.as_deref().map_or((ptr::null(), 0), |v| (v.as_ptr().cast(), v.len() as c_int));
                    // SAFETY: registered EntityDeclHandler; the value is
                    // valid for its length (not NUL-terminated, as in
                    // libexpat), the rest valid C strings or null.
                    unsafe { f(ud, n.as_ptr(), parameter as c_int, vptr, vlen, ptr_of(&base), ptr_of(&s), ptr_of(&p), ptr_of(&no)) };
                }
            }
            Event::StartNamespace { prefix, uri } => {
                if let Some(f) = h.start_namespace.get() {
                    let (p, u) = (opt_cstr(prefix.as_deref()), opt_cstr(uri.as_deref()));
                    // SAFETY: registered StartNamespaceDeclHandler; valid C
                    // strings or null.
                    unsafe { f(ud, ptr_of(&p), ptr_of(&u)) };
                }
            }
            Event::EndNamespace { prefix } => {
                if let Some(f) = h.end_namespace.get() {
                    let p = opt_cstr(prefix.as_deref());
                    // SAFETY: registered EndNamespaceDeclHandler; valid C
                    // string or null.
                    unsafe { f(ud, ptr_of(&p)) };
                }
            }
            Event::ParameterEntityRef(_) => self.not_standalone(),
            Event::ExternalEntityRef { name, system_id, public_id, context } => {
                // With no handler, libexpat passes the reference to the
                // default handler (ElementTree relies on this).
                let Some(f) = h.external_entity_ref.get() else {
                    self.to_default(l.raw);
                    return;
                };
                {
                    // The context string identifies the reference to
                    // XML_ExternalEntityParserCreate; the namespace
                    // declarations in scope travel in `context` itself.
                    let key = cstr(&name);
                    if let Ok(mut contexts) = self.entity_contexts.try_borrow_mut() {
                        contexts.insert(key.clone(), context.clone());
                    }
                    *self.last_entity_context.borrow_mut() = Some(context);
                    let base = self.base.borrow().clone();
                    let (s, p) = (cstr(&system_id), opt_cstr(public_id.as_deref()));
                    let me = (self as *const Shim).cast_mut();
                    // SAFETY: registered ExternalEntityRefHandler, given this
                    // parser and valid C strings (or null).
                    if unsafe { f(me, key.as_ptr(), ptr_of(&base), s.as_ptr(), ptr_of(&p)) } == 0 {
                        // Reported at the reference, as libexpat does.
                        self.token_end.set(self.pos.get());
                        self.input_end.set(self.input_pos.get());
                        self.stop_error.set(XML_ERROR_EXTERNAL_ENTITY_HANDLING);
                        self.stop_requested.set(true);
                    }
                }
            }
            Event::SkippedEntity(name) => match h.skipped_entity.get() {
                Some(f) => {
                    let n = cstr(&name);
                    // SAFETY: registered SkippedEntityHandler; valid C string.
                    unsafe { f(ud, n.as_ptr(), 0) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            _ => {}
        }
    }

    /// Deliver character data as libexpat does: in pieces split at line
    /// ends, each line end a piece of its own (`"\n"`). When `text` is its
    /// source `raw` (starting at `start`) with line ends normalised, each
    /// piece is reported where it is in the source; text that came from an
    /// entity is all reported at the reference. Stops early if a handler
    /// calls `XML_StopParser`.
    /// The DOCTYPE as libexpat shows it to the default handler: token by
    /// token, the parts no other handler takes (the header without a
    /// DOCTYPE handler, declarations without their handler, the whitespace
    /// between declarations and parameter-entity references always, the
    /// closing `]>` without an end handler).
    ///
    /// The DOCTYPE's text is kept even without a default handler, which a
    /// handler may set from inside the DOCTYPE (minidom does, to collect the
    /// internal subset).
    fn dtd_default(&self, l: &Located<'_>) {
        let h = &self.handlers;
        if h.default.get().is_none() && !matches!(l.event, Event::Doctype { .. }) && self.dtd_text.borrow().is_none() {
            return;
        }
        match &l.event {
            Event::Doctype { .. } => {
                let header_len = doctype_header_len(l.raw);
                let has_subset = l.raw[..header_len].ends_with('[');
                if h.start_doctype.get().is_none() {
                    let mut header = &l.raw[..header_len];
                    if !has_subset && h.end_doctype.get().is_some() {
                        header = header.strip_suffix('>').unwrap_or(header);
                    }
                    for token in dtd_tokens(header) {
                        self.to_default(token);
                    }
                }
                *self.dtd_text.borrow_mut() =
                    Some(DtdText { raw: l.raw.to_string(), start: l.span.start.byte_offset, cursor: header_len });
            }
            Event::EndDoctype => {
                let Some(dtd) = self.dtd_text.borrow_mut().take() else { return };
                let tail = dtd.raw.get(dtd.cursor..).unwrap_or("");
                let rest = tail.trim_start_matches([' ', '\t', '\r', '\n']);
                if rest.len() < tail.len() {
                    self.to_default(&tail[..tail.len() - rest.len()]);
                }
                if h.end_doctype.get().is_none() {
                    for token in dtd_tokens(rest) {
                        self.to_default(token);
                    }
                }
            }
            Event::ElementDecl { .. }
            | Event::AttlistDecl { .. }
            | Event::EntityDecl { .. }
            | Event::NotationDecl { .. }
            | Event::ProcessingInstruction { .. }
            | Event::Comment(_)
            | Event::ParameterEntityRef(_) => {
                let gap = {
                    let Ok(mut text) = self.dtd_text.try_borrow_mut() else { return };
                    let Some(dtd) = text.as_mut() else { return }; // not in a DOCTYPE
                    let Some(at) = l.span.start.byte_offset.checked_sub(dtd.start) else { return };
                    if at < dtd.cursor {
                        return; // another attribute of an ATTLIST already dealt with
                    }
                    let gap = dtd.raw.get(dtd.cursor..at).unwrap_or("").to_string();
                    dtd.cursor = at + l.raw.len();
                    gap
                };
                if !gap.is_empty() {
                    self.to_default(&gap);
                }
                let unhandled = match &l.event {
                    Event::ElementDecl { .. } => h.element_decl.get().is_none(),
                    Event::AttlistDecl { .. } => h.attlist_decl.get().is_none(),
                    Event::EntityDecl { notation, .. } => {
                        h.entity_decl.get().is_none() && !(notation.is_some() && h.unparsed_entity_decl.get().is_some())
                    }
                    Event::NotationDecl { .. } => h.notation_decl.get().is_none(),
                    Event::ParameterEntityRef(_) => {
                        self.to_default(l.raw);
                        false
                    }
                    _ => false, // processing instructions and comments: their own arms
                };
                if unhandled {
                    for token in dtd_tokens(l.raw) {
                        self.to_default(token);
                    }
                }
            }
            _ => {}
        }
    }

    /// Ask the external entity handler for the external DTD subset
    /// (`ids`: the DOCTYPE's system and public identifiers), or for the
    /// foreign DTD when the document names none, as libexpat does when
    /// parameter entities are parsed: XML_PARAM_ENTITY_PARSING_ALWAYS, or
    /// UNLESS_STANDALONE in a document that isn't standalone. Once only.
    fn external_dtd(&self, ids: Option<(Option<CString>, Option<CString>)>) {
        let (Some(f), pep) = (self.handlers.external_entity_ref.get(), self.param_entity_parsing.get()) else { return };
        if self.foreign_dtd_done.get() || pep == XML_PARAM_ENTITY_PARSING_NEVER
            || (pep == XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE && self.standalone.get())
        {
            return;
        }
        let (system, public) = match ids {
            Some((s, p)) if s.is_some() || p.is_some() => (s, p),
            _ if self.use_foreign_dtd.get() => (None, None),
            _ => return,
        };
        self.foreign_dtd_done.set(true);
        let base = self.base.borrow().clone();
        let me = (self as *const Shim).cast_mut();
        // SAFETY: registered ExternalEntityRefHandler, given this parser, a
        // null context (a DTD) and valid C strings or null.
        if unsafe { f(me, ptr::null(), ptr_of(&base), ptr_of(&system), ptr_of(&public)) } == 0 {
            self.stop_error.set(XML_ERROR_EXTERNAL_ENTITY_HANDLING);
            self.stop_requested.set(true);
        }
    }

    /// A reference the document's own content can't resolve (an external
    /// subset, a parameter entity): unless it declared itself standalone,
    /// libexpat asks the not-standalone handler whether to go on; 0 means
    /// stop with XML_ERROR_NOT_STANDALONE.
    fn not_standalone(&self) {
        if self.standalone.get() {
            return;
        }
        if let Some(f) = self.handlers.not_standalone.get() {
            // SAFETY: registered NotStandaloneHandler.
            if unsafe { f(self.ud()) } == 0 {
                self.stop_error.set(XML_ERROR_NOT_STANDALONE);
                self.stop_requested.set(true);
            }
        }
    }

    /// Where libexpat reports `attribute` of the ATTLIST declaration `l`
    /// (see `decl_trigger`): its position and input offset, and the input
    /// offset after the declaration.
    fn attlist_place(&self, l: &Located<'_>, attribute: &str) -> (Position, usize, usize) {
        let key = (l.raw.as_ptr() as usize, l.raw.len());
        let mut cache = self.attlist.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.key != key) {
            // One pass over the declaration, from default to default.
            let mut defaults = attlist_defaults(l.raw);
            defaults.sort_by_key(|&(_, at)| at);
            let (mut from, mut pos, mut input) = (0, l.span.start, l.input_start);
            let mut places = HashMap::new();
            for (name, at) in defaults {
                pos = pos.advance(&l.raw[from..at]);
                input += self.input_len(&l.raw[from..at]);
                from = at;
                places.entry(name.to_string()).or_insert((pos, input));
            }
            let end = input + self.input_len(&l.raw[from..]);
            *cache = Some(AttlistPlaces { key, places, end });
        }
        let c = cache.as_ref().expect("filled above");
        let (pos, input) = c.places.get(attribute).copied().unwrap_or((l.span.start, l.input_start));
        (pos, input, c.end)
    }

    /// After a stop in a CDATA section's start handler, the first token of
    /// its text, which libexpat still delivers: up to the first `]` after
    /// the first character or the first line end, or a line end alone.
    fn cdata_first_token(&self, text: &str, raw: &str, start: Position, start_input: usize) {
        fn first(s: &str) -> &str {
            let s = cdata_parts(s)[0];
            match s.find(['\r', '\n']) {
                Some(0) => &s[..if s.starts_with("\r\n") { 2 } else { 1 }],
                Some(eol) => &s[..eol],
                None => s,
            }
        }
        let t = first(text);
        let t = if t.starts_with('\r') { "\n" } else { t };
        if is_source(text, raw) {
            self.char_piece(t, first(raw), (start, start_input));
        } else {
            // From an entity's replacement text: at the reference.
            self.pos.set(start);
            self.input_pos.set(start_input);
            self.deliver_chars(t, raw);
        }
    }

    /// Text of a CDATA section: libexpat's tokens there also end before
    /// each `]` (which may start the closing `]]>`), so its handler gets
    /// the text in parts, each after the first starting with a `]`.
    fn cdata_text(&self, text: &str, raw: &str, start: Position, start_input: usize) {
        if !is_source(text, raw) {
            // From an entity's replacement text: every part at the reference.
            for t in cdata_parts(text) {
                if self.stop_requested.get() {
                    return;
                }
                self.character_data(t, raw, start, start_input);
            }
            return;
        }
        // Line ends are normalised in `text`, not in `raw`; `]`s line up.
        let mut at = (start, start_input);
        for (t, r) in cdata_parts(text).into_iter().zip(cdata_parts(raw)) {
            if self.stop_requested.get() {
                return;
            }
            self.character_data(t, r, at.0, at.1);
            at = (at.0.advance(r), at.1 + self.input_len(r));
        }
    }

    fn character_data(&self, text: &str, raw: &str, start: Position, start_input: usize) {
        if is_source(text, raw) {
            let bytes = raw.as_bytes();
            let (mut at, mut piece_start, mut i) = ((start, start_input), 0, 0);
            while i < bytes.len() {
                if bytes[i] != b'\n' && bytes[i] != b'\r' {
                    i += 1;
                    continue;
                }
                if piece_start < i {
                    let piece = &raw[piece_start..i];
                    at = self.char_piece(piece, piece, at);
                    if self.stop_requested.get() {
                        return;
                    }
                }
                let eol = if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                at = self.char_piece("\n", &raw[i..i + eol], at);
                if self.stop_requested.get() {
                    return;
                }
                i += eol;
                piece_start = i;
            }
            if piece_start < raw.len() {
                let piece = &raw[piece_start..];
                self.char_piece(piece, piece, at);
            }
        } else {
            for line in text.split_inclusive('\n') {
                let (body, eol) = match line.strip_suffix('\n') {
                    Some(body) => (body, "\n"),
                    None => (line, ""),
                };
                for part in [body, eol] {
                    if !part.is_empty() && !self.stop_requested.get() {
                        self.pos.set(start);
                        self.input_pos.set(start_input);
                        self.deliver_chars(part, raw);
                    }
                }
            }
        }
    }

    /// One piece of character data from the source at `at` (position,
    /// input offset); returns the place after it.
    fn char_piece(&self, text: &str, raw: &str, at: (Position, usize)) -> (Position, usize) {
        let end = (at.0.advance(raw), at.1 + self.input_len(raw));
        self.pos.set(at.0);
        self.input_pos.set(at.1);
        self.token_end.set(end.0);
        self.input_end.set(end.1);
        self.deliver_chars(text, raw);
        end
    }

    /// Bytes of input that decoded text `s` came from (see
    /// `Located::input_start`): two or four per character in UTF-16, one
    /// in the single-byte encodings libexpat converts, else its length.
    fn input_len(&self, s: &str) -> usize {
        if self.utf16.get() {
            s.chars().map(|c| if c as u32 > 0xFFFF { 4 } else { 2 }).sum()
        } else if self.chunk_text.get() {
            s.chars().count()
        } else {
            s.len()
        }
    }

    fn deliver_chars(&self, text: &str, raw: &str) {
        if self.chunk_text.get() && text.len() > TEXT_PIECE {
            // Split at character boundaries, as libexpat's conversion does.
            let (mut rest_text, mut rest_raw) = (text, raw);
            while !rest_text.is_empty() && !self.stop_requested.get() {
                let mut cut = TEXT_PIECE.min(rest_text.len());
                while !rest_text.is_char_boundary(cut) {
                    cut -= 1;
                }
                let (piece, after) = rest_text.split_at(cut);
                // Raw text is the same as the text here (no line ends, no
                // references); otherwise give the default handler all of it.
                let (raw_piece, raw_after) = if rest_raw == rest_text { rest_raw.split_at(cut) } else { (rest_raw, "") };
                self.deliver_piece(piece, raw_piece);
                rest_text = after;
                rest_raw = raw_after;
            }
            return;
        }
        self.deliver_piece(text, raw);
    }

    fn deliver_piece(&self, text: &str, raw: &str) {
        match self.handlers.character_data.get() {
            Some(f) => {
                // SAFETY: registered CharacterDataHandler; `text` is valid
                // for its length during the call.
                unsafe { f(self.ud(), text.as_ptr().cast(), text.len() as c_int) };
            }
            None if !raw.is_empty() => {
                self.to_default(raw);
            }
            None => {}
        }
    }

    /// An `XML_Content` tree in one block from the memory suite: the nodes
    /// (root first, each group's children contiguous) followed by the names.
    /// Built breadth-first with a queue, so deep models don't recurse.
    fn build_content(&self, model: &ContentModel) -> *mut XML_Content {
        // Order nodes breadth-first so each node's children are contiguous.
        let mut order = vec![model.root];
        let mut i = 0;
        while i < order.len() {
            order.extend(model.nodes[order[i]].children.iter().copied());
            i += 1;
        }
        let mut index_of = vec![0usize; model.nodes.len()];
        for (pos, &n) in order.iter().enumerate() {
            index_of[n] = pos;
        }
        let names_len: usize = model.nodes.iter().map(|n| n.name.as_ref().map_or(0, |s| s.len() + 1)).sum();
        let nodes_size = order.len() * std::mem::size_of::<XML_Content>();
        let block = self.mem_malloc(nodes_size + names_len);
        if block.is_null() {
            return ptr::null_mut();
        }
        let nodes = block.cast::<XML_Content>();
        // SAFETY: `block` holds `order.len()` XML_Content slots followed by
        // `names_len` bytes; every write below stays within those bounds,
        // and XML_Content's alignment (8) is malloc's minimum.
        unsafe {
            let mut name_at = block.cast::<u8>().add(nodes_size);
            for (pos, &n) in order.iter().enumerate() {
                let node = &model.nodes[n];
                let name = match &node.name {
                    Some(s) => {
                        ptr::copy_nonoverlapping(s.as_ptr(), name_at, s.len());
                        *name_at.add(s.len()) = 0;
                        let p = name_at.cast::<XML_Char>();
                        name_at = name_at.add(s.len() + 1);
                        p
                    }
                    None => ptr::null_mut(),
                };
                let children = match node.children.first() {
                    Some(&first) => nodes.add(index_of[first]),
                    None => ptr::null_mut(),
                };
                nodes.add(pos).write(XML_Content {
                    type_: content_type(node.kind),
                    quant: content_quant(node.quant),
                    name,
                    numchildren: node.children.len() as c_uint,
                    children,
                });
            }
        }
        nodes
    }
}

fn content_type(kind: ContentKind) -> c_int {
    match kind {
        ContentKind::Empty => XML_CTYPE_EMPTY,
        ContentKind::Any => XML_CTYPE_ANY,
        ContentKind::Mixed => XML_CTYPE_MIXED,
        ContentKind::Name => XML_CTYPE_NAME,
        ContentKind::Choice => XML_CTYPE_CHOICE,
        ContentKind::Seq => XML_CTYPE_SEQ,
    }
}

fn content_quant(q: Quantifier) -> c_int {
    match q {
        Quantifier::None => XML_CQUANT_NONE,
        Quantifier::Opt => XML_CQUANT_OPT,
        Quantifier::Rep => XML_CQUANT_REP,
        Quantifier::Plus => XML_CQUANT_PLUS,
    }
}

// ─── exported functions ───────────────────────────────────────────────────

/// Read a C string argument, or `None` for null.
///
/// # Safety
/// `s` is null or a valid NUL-terminated string.
unsafe fn c_arg(s: *const XML_Char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: per the caller, `s` is a valid NUL-terminated string.
    Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}

#[no_mangle]
pub unsafe extern "C" fn XML_ParserCreate_MM(
    encoding: *const XML_Char,
    memsuite: *const XML_Memory_Handling_Suite,
    namespace_separator: *const XML_Char,
) -> XML_Parser {
    // SAFETY: encoding and namespace_separator are null or valid C strings
    // per the API; memsuite is null or points to a suite.
    let encoding = unsafe { c_arg(encoding) };
    let memsuite = unsafe { memsuite.as_ref() }.copied();
    let ns_sep = if namespace_separator.is_null() { None } else { Some(unsafe { *namespace_separator } as u8) };
    Box::into_raw(Box::new(Shim::new(encoding, memsuite, ns_sep)))
}

#[no_mangle]
pub unsafe extern "C" fn XML_ParserCreate(encoding: *const XML_Char) -> XML_Parser {
    // SAFETY: forwards the caller's arguments unchanged.
    unsafe { XML_ParserCreate_MM(encoding, ptr::null(), ptr::null()) }
}

#[no_mangle]
pub unsafe extern "C" fn XML_ParserCreateNS(encoding: *const XML_Char, namespace_separator: XML_Char) -> XML_Parser {
    let sep = [namespace_separator, 0];
    // SAFETY: `sep` is a valid C string for the call.
    unsafe { XML_ParserCreate_MM(encoding, ptr::null(), sep.as_ptr()) }
}

#[no_mangle]
pub unsafe extern "C" fn XML_ParserFree(parser: XML_Parser) {
    if !parser.is_null() {
        // SAFETY: the parser came from Box::into_raw in XML_ParserCreate_MM
        // and is freed once, by the caller's contract.
        drop(unsafe { Box::from_raw(parser) });
    }
}

/// Make the parser as new, as from `XML_ParserCreate*` with `encoding` and
/// its original memory suite and namespace separator: handlers (but the
/// unknown-encoding handler, which libexpat keeps), user data and every
/// setting cleared, an unknown encoding's data released. As in
/// libexpat, a parser from `XML_ExternalEntityParserCreate` can't be reset;
/// nor can one from inside its own handler (libexpat leaves that undefined;
/// here it would free the parser that is calling the handler).
#[no_mangle]
pub unsafe extern "C" fn XML_ParserReset(parser: XML_Parser, encoding: *const XML_Char) -> XML_Bool {
    let fresh = {
        // SAFETY: see `shim`; encoding is null or a valid C string.
        let Some(p) = (unsafe { shim(parser) }) else { return XML_FALSE };
        if p.role != Role::Root || p.core.try_borrow_mut().is_err() {
            return XML_FALSE;
        }
        let fresh = Shim::new(unsafe { c_arg(encoding) }, Some(p.memsuite), p.ns_sep);
        // libexpat keeps the unknown-encoding handler and its data.
        fresh.unknown_encoding.set(p.unknown_encoding.get());
        fresh
    };
    // SAFETY: the parser came from XML_ParserCreate_MM and isn't in use (it
    // isn't parsing, and no reference to it is held here): the old state is
    // dropped in place and the fresh one written over it.
    unsafe { *parser = fresh };
    XML_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn XML_Parse(parser: XML_Parser, s: *const c_char, len: c_int, is_final: c_int) -> XML_Status {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_STATUS_ERROR };
    if len < 0 || (s.is_null() && len > 0) {
        p.error.set(XML_ERROR_INVALID_ARGUMENT);
        return XML_STATUS_ERROR;
    }
    let input: &[u8] = if len == 0 {
        &[]
    } else {
        // SAFETY: the caller passes `len` readable bytes at `s`.
        unsafe { std::slice::from_raw_parts(s.cast(), len as usize) }
    };
    p.parse(input, is_final != 0)
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetBuffer(parser: XML_Parser, len: c_int) -> *mut c_void {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return ptr::null_mut() };
    if len < 0 {
        p.error.set(XML_ERROR_INVALID_ARGUMENT);
        return ptr::null_mut();
    }
    let Ok(mut buf) = p.buffer.try_borrow_mut() else { return ptr::null_mut() };
    buf.clear();
    buf.resize(len as usize, 0);
    buf.as_mut_ptr().cast()
}

#[no_mangle]
pub unsafe extern "C" fn XML_ParseBuffer(parser: XML_Parser, len: c_int, is_final: c_int) -> XML_Status {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_STATUS_ERROR };
    let Ok(buf) = p.buffer.try_borrow() else { return XML_STATUS_ERROR };
    if len < 0 || len as usize > buf.len() {
        p.error.set(XML_ERROR_INVALID_ARGUMENT);
        return XML_STATUS_ERROR;
    }
    let input = buf[..len as usize].to_vec();
    drop(buf);
    p.parse(&input, is_final != 0)
}

#[no_mangle]
pub unsafe extern "C" fn XML_StopParser(parser: XML_Parser, resumable: XML_Bool) -> XML_Status {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_STATUS_ERROR };
    match p.status.get() {
        Status::NotStarted => {
            p.error.set(XML_ERROR_NOT_STARTED);
            XML_STATUS_ERROR
        }
        Status::Finished | Status::Failed => {
            p.error.set(XML_ERROR_FINISHED);
            XML_STATUS_ERROR
        }
        Status::Parsing if resumable != XML_FALSE => {
            // Suspending (and XML_ResumeParser) isn't supported yet.
            p.error.set(XML_ERROR_SUSPEND_PE);
            XML_STATUS_ERROR
        }
        // Between XML_Parse calls, not from a handler: libexpat finishes
        // the parse there and then, so the next call is XML_ERROR_FINISHED.
        Status::Parsing if p.core.try_borrow_mut().is_ok() => {
            p.status.set(Status::Finished);
            XML_STATUS_OK
        }
        Status::Parsing => {
            p.stop_error.set(XML_ERROR_ABORTED);
            p.stop_requested.set(true);
            XML_STATUS_OK
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetErrorCode(parser: XML_Parser) -> XML_Error {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(XML_ERROR_INVALID_ARGUMENT, |p| p.error.get())
}

#[no_mangle]
pub extern "C" fn XML_ErrorString(code: XML_Error) -> *const XML_LChar {
    match usize::try_from(code) {
        Ok(i) if i > 0 && i < ERROR_STRINGS.len() => ERROR_STRINGS[i].as_ptr().cast(),
        _ => ptr::null(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetCurrentLineNumber(parser: XML_Parser) -> XML_Size {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(0, |p| p.pos.get().line as XML_Size)
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetCurrentColumnNumber(parser: XML_Parser) -> XML_Size {
    // libexpat's columns are 0-based; expat-rs's are 1-based.
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(0, |p| p.column(p.pos.get()))
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetCurrentByteIndex(parser: XML_Parser) -> XML_Index {
    // In bytes of input, before decoding, as libexpat counts.
    // SAFETY: see `shim`.
    // -1 before any input, as libexpat.
    unsafe { shim(parser) }.map_or(-1, |p| if p.status.get() == Status::NotStarted { -1 } else { p.input_pos.get() as XML_Index })
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetSpecifiedAttributeCount(parser: XML_Parser) -> c_int {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(-1, |p| p.specified_atts.get())
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetIdAttributeIndex(parser: XML_Parser) -> c_int {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(-1, |p| p.id_index.get())
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetCurrentByteCount(parser: XML_Parser) -> c_int {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return 0 };
    if p.status.get() != Status::Parsing {
        return 0;
    }
    p.input_end.get().saturating_sub(p.input_pos.get()) as c_int
}

#[no_mangle]
pub unsafe extern "C" fn XML_DefaultCurrent(parser: XML_Parser) {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return };
    if p.status.get() != Status::Parsing {
        return;
    }
    // A copy, so the default handler may itself call back in.
    let raw = p.current_raw.try_borrow().map(|r| r.clone()).unwrap_or_default();
    if !raw.is_empty() {
        p.to_default(&raw);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_UseParserAsHandlerArg(parser: XML_Parser) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.parser_as_arg.set(true);
    }
}

/// libexpat 2.7.2+: limits on allocation amplification. expat-rs's
/// allocations are bounded by the input and the expansion limits, so these
/// are accepted (with libexpat's argument checks) and have no effect.
#[no_mangle]
pub unsafe extern "C" fn XML_SetAllocTrackerMaximumAmplification(parser: XML_Parser, maximum: f32) -> XML_Bool {
    (!parser.is_null() && !maximum.is_nan() && maximum >= 1.0) as XML_Bool
}

/// See [`XML_SetAllocTrackerMaximumAmplification`].
#[no_mangle]
pub unsafe extern "C" fn XML_SetAllocTrackerActivationThreshold(parser: XML_Parser, _threshold: c_ulonglong) -> XML_Bool {
    (!parser.is_null()) as XML_Bool
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetCdataSectionHandler(parser: XML_Parser, start: CdataSectionHandler, end: CdataSectionHandler) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.start_cdata.set(start);
        p.handlers.end_cdata.set(end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetDoctypeDeclHandler(
    parser: XML_Parser,
    start: StartDoctypeDeclHandler,
    end: EndDoctypeDeclHandler,
) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.start_doctype.set(start);
        p.handlers.end_doctype.set(end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetInputContext(_parser: XML_Parser, _offset: *mut c_int, _size: *mut c_int) -> *const c_char {
    // Input context isn't kept; libexpat also returns NULL without it.
    ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetUserData(parser: XML_Parser, user_data: *mut c_void) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.user_data.set(user_data);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetEncoding(parser: XML_Parser, encoding: *const XML_Char) -> XML_Status {
    // SAFETY: see `shim`; encoding is null or a valid C string.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_STATUS_ERROR };
    if p.status.get() != Status::NotStarted {
        return XML_STATUS_ERROR;
    }
    *p.encoding.borrow_mut() = unsafe { c_arg(encoding) };
    XML_STATUS_OK
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetBase(parser: XML_Parser, base: *const XML_Char) -> XML_Status {
    // SAFETY: see `shim`; base is null or a valid C string.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_STATUS_ERROR };
    let Ok(mut b) = p.base.try_borrow_mut() else { return XML_STATUS_ERROR };
    *b = if base.is_null() { None } else { Some(unsafe { CStr::from_ptr(base) }.to_owned()) };
    XML_STATUS_OK
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetBase(parser: XML_Parser) -> *const XML_Char {
    // SAFETY: see `shim`. The returned pointer stays valid until the next
    // XML_SetBase or XML_ParserFree, as in libexpat.
    let Some(p) = (unsafe { shim(parser) }) else { return ptr::null() };
    p.base.try_borrow().map_or(ptr::null(), |b| ptr_of(&b))
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetHashSalt(parser: XML_Parser, _hash_salt: c_ulong) -> c_int {
    // expat-rs's tables aren't attackable through a salt; accept and ignore.
    !parser.is_null() as c_int
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetParamEntityParsing(parser: XML_Parser, parsing: c_int) -> c_int {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return 0 };
    if p.status.get() != Status::NotStarted {
        return 0;
    }
    // Recorded for when parameter entities are read; not used yet.
    p.param_entity_parsing.set(parsing);
    1
}

#[no_mangle]
pub unsafe extern "C" fn XML_UseForeignDTD(parser: XML_Parser, use_dtd: XML_Bool) -> XML_Error {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_ERROR_INVALID_ARGUMENT };
    if p.status.get() != Status::NotStarted {
        return XML_ERROR_CANT_CHANGE_FEATURE_ONCE_PARSING;
    }
    p.use_foreign_dtd.set(use_dtd != XML_FALSE);
    XML_ERROR_NONE
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetReturnNSTriplet(parser: XML_Parser, do_nst: c_int) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        if p.status.get() == Status::NotStarted {
            p.ns_triplets.set(do_nst != 0);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetReparseDeferralEnabled(parser: XML_Parser, enabled: XML_Bool) -> XML_Bool {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_FALSE };
    if enabled > 1 {
        return XML_FALSE;
    }
    p.reparse_deferral.set(enabled == XML_TRUE);
    if let Ok(mut core) = p.core.try_borrow_mut() {
        if let Some(sp) = core.as_mut() {
            sp.set_reparse_deferral(enabled == XML_TRUE);
        }
    }
    XML_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetBillionLaughsAttackProtectionActivationThreshold(
    parser: XML_Parser,
    activation_threshold_bytes: c_ulonglong,
) -> XML_Bool {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_FALSE };
    if p.role != Role::Root {
        return XML_FALSE; // a setting for the whole document
    }
    let mut limits = p.amplification.get();
    limits.activation_threshold = activation_threshold_bytes;
    p.amplification.set(limits);
    XML_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetBillionLaughsAttackProtectionMaximumAmplification(
    parser: XML_Parser,
    maximum_amplification_factor: f32,
) -> XML_Bool {
    // SAFETY: see `shim`.
    let Some(p) = (unsafe { shim(parser) }) else { return XML_FALSE };
    if p.role != Role::Root {
        return XML_FALSE; // a setting for the whole document
    }
    if maximum_amplification_factor.is_nan() || maximum_amplification_factor < 1.0 {
        return XML_FALSE;
    }
    let mut limits = p.amplification.get();
    limits.maximum_factor = maximum_amplification_factor;
    p.amplification.set(limits);
    XML_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn XML_ExternalEntityParserCreate(
    parser: XML_Parser,
    context: *const XML_Char,
    encoding: *const XML_Char,
) -> XML_Parser {
    // SAFETY: see `shim`; context and encoding are null or valid C strings.
    let Some(parent) = (unsafe { shim(parser) }) else { return ptr::null_mut() };
    let encoding = unsafe { c_arg(encoding) };
    let (role, entity) = if context.is_null() {
        (Role::Dtd, None)
    } else {
        // SAFETY: a valid C string, per the API.
        let key = unsafe { CStr::from_ptr(context) };
        // The reference this context was given with; failing that, the
        // latest one (a handler may pass a context of its own).
        let entity = parent.entity_contexts.try_borrow().ok().and_then(|c| c.get(key).cloned())
            .or_else(|| parent.last_entity_context.try_borrow().ok().and_then(|c| c.clone()));
        match entity {
            Some(entity) => (Role::Entity, Some(entity)),
            None => (Role::Dtd, None),
        }
    };
    let mut child = Shim::new(encoding, Some(parent.memsuite), parent.ns_sep);
    child.role = role;
    child.entity = entity;
    child.handlers.copy_from(&parent.handlers);
    child.unknown_encoding.set(parent.unknown_encoding.get());
    child.user_data.set(parent.user_data.get());
    child.parser_as_arg.set(parent.parser_as_arg.get());
    child.ns_triplets.set(parent.ns_triplets.get());
    child.reparse_deferral.set(parent.reparse_deferral.get());
    child.param_entity_parsing.set(parent.param_entity_parsing.get());
    *child.base.borrow_mut() = parent.base.borrow().clone();
    Box::into_raw(Box::new(child))
}

#[no_mangle]
pub unsafe extern "C" fn XML_FreeContentModel(parser: XML_Parser, model: *mut XML_Content) {
    // SAFETY: see `shim`; `model` came from this parser's build_content,
    // allocated as one block from its memory suite.
    if let Some(p) = unsafe { shim(parser) } {
        p.mem_free(model.cast());
    }
}

#[no_mangle]
pub extern "C" fn XML_ExpatVersion() -> *const XML_LChar {
    c"expat_2.7.1".as_ptr()
}

#[no_mangle]
pub extern "C" fn XML_ExpatVersionInfo() -> XML_Expat_Version {
    XML_Expat_Version { major: 2, minor: 7, micro: 1 }
}

/// The static feature list: a wrapper so the raw name pointers can live in
/// a `static`.
struct Features([XML_Feature; 5]);
// SAFETY: the list is immutable and its pointers point at static strings.
unsafe impl Sync for Features {}

static FEATURES: Features = Features([
    XML_Feature { feature: XML_FEATURE_SIZEOF_XML_CHAR, name: c"sizeof(XML_Char)".as_ptr(), value: 1 },
    XML_Feature { feature: XML_FEATURE_SIZEOF_XML_LCHAR, name: c"sizeof(XML_LChar)".as_ptr(), value: 1 },
    XML_Feature { feature: XML_FEATURE_NS, name: c"XML_NS".as_ptr(), value: 0 },
    XML_Feature { feature: XML_FEATURE_CONTEXT_BYTES, name: c"XML_CONTEXT_BYTES".as_ptr(), value: 0 },
    XML_Feature { feature: XML_FEATURE_END, name: ptr::null(), value: 0 },
]);

#[no_mangle]
pub extern "C" fn XML_GetFeatureList() -> *const XML_Feature {
    FEATURES.0.as_ptr()
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetUnknownEncodingHandler(parser: XML_Parser, handler: UnknownEncodingHandler, data: *mut c_void) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.unknown_encoding.set((handler, data));
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetElementHandler(parser: XML_Parser, start: StartElementHandler, end: EndElementHandler) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.start_element.set(start);
        p.handlers.end_element.set(end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetNamespaceDeclHandler(
    parser: XML_Parser,
    start: StartNamespaceDeclHandler,
    end: EndNamespaceDeclHandler,
) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.start_namespace.set(start);
        p.handlers.end_namespace.set(end);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetDefaultHandler(parser: XML_Parser, handler: DefaultHandler) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.default.set(handler);
        p.handlers.default_expand.set(false);
    }
}

#[no_mangle]
pub unsafe extern "C" fn XML_SetDefaultHandlerExpand(parser: XML_Parser, handler: DefaultHandler) {
    // SAFETY: see `shim`.
    if let Some(p) = unsafe { shim(parser) } {
        p.handlers.default.set(handler);
        p.handlers.default_expand.set(true);
    }
}

/// Exported setters that store one handler.
macro_rules! setter {
    ($($fn_name:ident($field:ident: $ty:ty);)*) => {$(
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(parser: XML_Parser, handler: $ty) {
            // SAFETY: see `shim`.
            if let Some(p) = unsafe { shim(parser) } {
                p.handlers.$field.set(handler);
            }
        }
    )*};
}

setter! {
    XML_SetStartElementHandler(start_element: StartElementHandler);
    XML_SetEndElementHandler(end_element: EndElementHandler);
    XML_SetCharacterDataHandler(character_data: CharacterDataHandler);
    XML_SetProcessingInstructionHandler(processing_instruction: ProcessingInstructionHandler);
    XML_SetCommentHandler(comment: CommentHandler);
    XML_SetStartCdataSectionHandler(start_cdata: CdataSectionHandler);
    XML_SetEndCdataSectionHandler(end_cdata: CdataSectionHandler);
    XML_SetStartDoctypeDeclHandler(start_doctype: StartDoctypeDeclHandler);
    XML_SetEndDoctypeDeclHandler(end_doctype: EndDoctypeDeclHandler);
    XML_SetEntityDeclHandler(entity_decl: EntityDeclHandler);
    XML_SetUnparsedEntityDeclHandler(unparsed_entity_decl: UnparsedEntityDeclHandler);
    XML_SetNotationDeclHandler(notation_decl: NotationDeclHandler);
    XML_SetStartNamespaceDeclHandler(start_namespace: StartNamespaceDeclHandler);
    XML_SetEndNamespaceDeclHandler(end_namespace: EndNamespaceDeclHandler);
    XML_SetNotStandaloneHandler(not_standalone: NotStandaloneHandler);
    XML_SetExternalEntityRefHandler(external_entity_ref: ExternalEntityRefHandler);
    XML_SetSkippedEntityHandler(skipped_entity: SkippedEntityHandler);
    XML_SetXmlDeclHandler(xml_decl: XmlDeclHandler);
    XML_SetElementDeclHandler(element_decl: ElementDeclHandler);
    XML_SetAttlistDeclHandler(attlist_decl: AttlistDeclHandler);
}

#[cfg(test)]
mod layout {
    use super::*;

    #[test]
    fn struct_layouts_match_the_header() {
        assert_eq!(std::mem::size_of::<XML_Content>(), SIZEOF_XML_Content);
        assert_eq!(std::mem::size_of::<XML_Feature>(), SIZEOF_XML_Feature);
        assert_eq!(std::mem::size_of::<XML_Expat_Version>(), SIZEOF_XML_Expat_Version);
        assert_eq!(std::mem::size_of::<XML_Encoding>(), SIZEOF_XML_Encoding);
        assert_eq!(std::mem::size_of::<XML_Memory_Handling_Suite>(), SIZEOF_XML_Memory_Handling_Suite);
    }

    #[test]
    fn user_data_is_the_first_field() {
        // expat.h's XML_GetUserData reads *(void **)parser.
        let p = Box::new(Shim::new(None, None, None));
        let sentinel = 0x1234usize as *mut c_void;
        p.user_data.set(sentinel);
        let raw = &*p as *const Shim as *const *mut c_void;
        // SAFETY: Shim is repr(C) with a Cell<*mut c_void> (repr(transparent))
        // first, so reading a pointer at its address reads user_data.
        assert_eq!(unsafe { *raw }, sentinel);
    }
}

/// Inputs whose cost could grow faster than their size, as in the core
/// crate's tests/resource_tests.rs: the size took tens of seconds with the
/// old code; the assertions check behaviour, not timing.
#[cfg(test)]
mod resources {
    use super::*;

    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    unsafe extern "C" fn attlist(
        _: *mut c_void,
        _: *const XML_Char,
        _: *const XML_Char,
        _: *const XML_Char,
        _: *const XML_Char,
        _: c_int,
    ) {
        COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn one_attlist_declaring_many_attributes() {
        // Each attribute's position was worked out from the declaration's
        // start: 64,000 attributes took ~40 s.
        let n = 64_000;
        let mut doc = String::from("<!DOCTYPE r [<!ATTLIST r");
        for i in 0..n {
            doc.push_str(&format!("\n a{i} CDATA #IMPLIED"));
        }
        doc.push_str(">]><r/>");
        // SAFETY: a parser created, used and freed here, with a valid
        // buffer and a handler of the right type.
        unsafe {
            let p = XML_ParserCreate(ptr::null());
            XML_SetAttlistDeclHandler(p, Some(attlist));
            let status = XML_Parse(p, doc.as_ptr().cast(), doc.len() as c_int, 1);
            assert_eq!(status, XML_STATUS_OK);
            assert_eq!(XML_GetCurrentLineNumber(p), n as XML_Size + 1);
            XML_ParserFree(p);
        }
        assert_eq!(COUNT.load(std::sync::atomic::Ordering::Relaxed), n);
    }
}
