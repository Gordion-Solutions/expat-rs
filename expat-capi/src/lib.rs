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
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_ulonglong, c_void, CStr, CString};
use std::ptr;

use expat_rs::{
    AmplificationLimits, ByteMap, ContentKind, ContentModel, ErrorKind, Event, Located, Position, Quantifier, Span,
    StreamParser, XmlError,
};
use generated::*;

// ─── C types (layouts as in include/expat.h) ──────────────────────────────

pub type XML_Char = c_char;
pub type XML_LChar = c_char;
pub type XML_Bool = u8;
pub type XML_Status = c_int;
pub type XML_Error = c_int;
pub type XML_Size = c_ulong;
pub type XML_Index = c_long;
pub type XML_Parser = *mut Shim;

const XML_TRUE: XML_Bool = 1;
const XML_FALSE: XML_Bool = 0;
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
    /// `XML_GetSpecifiedAttributeCount` for the current start tag.
    specified_atts: Cell<c_int>,
    /// Set by `XML_StopParser`; checked after every callback.
    stop_requested: Cell<bool>,
    /// Span of the event being delivered.
    current_span: Cell<Option<Span>>,
    /// Where the token being delivered ends: parsing stopped by
    /// `XML_StopParser` reports this position, as libexpat does.
    token_end: Cell<Position>,
    /// Stopped during the start of an empty element (`<e/>`): its end is
    /// still delivered, since libexpat finishes the token first.
    stop_in_empty_tag: Cell<bool>,
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
            stop_requested: Cell::new(false),
            current_span: Cell::new(None),
            token_end: Cell::new(Position::start()),
            stop_in_empty_tag: Cell::new(false),
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
        let mut p = StreamParser::new()
            .with_reparse_deferral(self.reparse_deferral.get())
            .with_amplification_limits(self.amplification.get());
        if self.ns_sep.is_some() {
            p = p.with_namespaces();
        }
        if let Some(enc) = self.encoding.borrow().as_deref() {
            p = p.with_encoding(enc);
        }
        let (handler, data) = self.unknown_encoding.get();
        if let Some(handler) = handler {
            p = p.with_unknown_encoding(move |name| byte_map_from_c(handler, data, name));
        }
        p
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
        let Ok(mut core) = self.core.try_borrow_mut() else {
            // XML_Parse from inside one of this parser's own handlers.
            return XML_STATUS_ERROR;
        };
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
            }
        };
        let mut result = parser.feed_located(input, &mut deliver);
        if result.is_ok() && is_final && !self.stop_requested.get() {
            result = parser.finish_located(&mut deliver);
        }
        let parsed_to = parser.position();
        drop(core);
        if self.stop_requested.get() {
            self.pos.set(self.token_end.get());
            self.error.set(XML_ERROR_ABORTED);
            self.status.set(Status::Failed);
            return XML_STATUS_ERROR;
        }
        match result {
            Ok(()) => {
                self.pos.set(parsed_to);
                if is_final {
                    self.status.set(Status::Finished);
                }
                XML_STATUS_OK
            }
            Err(e) => {
                self.fail(&e);
                XML_STATUS_ERROR
            }
        }
    }

    fn fail(&self, e: &XmlError) {
        self.pos.set(e.position());
        self.error.set(error_code(e.kind()));
        self.status.set(Status::Failed);
    }
}

/// Ask a C unknown-encoding handler for a byte map.
fn byte_map_from_c(handler: unsafe extern "C" fn(*mut c_void, *const XML_Char, *mut XML_Encoding) -> c_int,
                   data: *mut c_void, name: &str) -> Option<ByteMap> {
    let name = CString::new(name).ok()?;
    let mut info = XML_Encoding { map: [-1; 256], data: ptr::null_mut(), convert: None, release: None };
    // SAFETY: the handler was registered for this purpose with `data`;
    // `name` is a valid C string and `info` a valid, writable XML_Encoding
    // for the duration of the call.
    let ok = unsafe { handler(data, name.as_ptr(), &mut info) };
    let map = (ok == XML_STATUS_OK).then(|| {
        let mut map: ByteMap = Box::new([None; 256]);
        for (byte, &entry) in info.map.iter().enumerate() {
            // Negative entries are invalid bytes or multi-byte lead bytes;
            // multi-byte encodings (`convert`) aren't supported yet.
            map[byte] = u32::try_from(entry).ok().and_then(char::from_u32);
        }
        map
    });
    let multibyte = info.map.iter().any(|&e| e < -1);
    if let Some(release) = info.release {
        // SAFETY: release takes the data pointer the handler stored in info.
        unsafe { release(info.data) };
    }
    if multibyte { None } else { map }
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
    fn ud(&self) -> *mut c_void {
        self.user_data.get()
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
        self.pos.set(l.span.start);
        self.current_span.set(Some(l.span));
        self.token_end.set(l.span.end);
        let h = &self.handlers;
        let ud = self.ud();
        match l.event {
            Event::XmlDecl(d) => match h.xml_decl.get() {
                Some(f) => {
                    let (v, e) = (cstr(d.version), opt_cstr(d.encoding));
                    let standalone = d.standalone.map_or(-1, |s| s as c_int);
                    // SAFETY: registered XmlDeclHandler; string arguments are
                    // valid C strings (or null) for the call.
                    unsafe { f(ud, v.as_ptr(), ptr_of(&e), standalone) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            Event::Doctype { name, system_id, public_id, has_internal_subset, .. } => match h.start_doctype.get() {
                Some(f) => {
                    let (n, s, p) = (cstr(name), opt_cstr(system_id.as_deref()), opt_cstr(public_id.as_deref()));
                    // SAFETY: registered StartDoctypeDeclHandler; valid C
                    // strings (or null) for the call.
                    unsafe { f(ud, n.as_ptr(), ptr_of(&s), ptr_of(&p), has_internal_subset as c_int) };
                }
                None => {
                    self.to_default(l.raw);
                }
            },
            Event::EndDoctype => {
                if let Some(f) = h.end_doctype.get() {
                    // SAFETY: registered EndDoctypeDeclHandler.
                    unsafe { f(ud) };
                }
            }
            Event::StartElement { name, namespace, attributes } => {
                self.specified_atts.set(2 * attributes.iter().filter(|a| a.specified).count() as c_int);
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
            Event::Text(text) => self.character_data(&text, l.raw, l.span.start),
            Event::CData(text) => {
                const OPEN: &str = "<![CDATA[";
                let content_raw = l.raw.get(OPEN.len()..l.raw.len().saturating_sub(3)).unwrap_or("");
                let content_at = l.span.start.advance(OPEN);
                match h.start_cdata.get() {
                    // SAFETY: registered StartCdataSectionHandler.
                    Some(f) => unsafe { f(ud) },
                    None => {
                        self.to_default(OPEN);
                    }
                }
                if self.stop_requested.get() {
                    return;
                }
                self.character_data(&text, content_raw, content_at);
                if self.stop_requested.get() {
                    return;
                }
                self.pos.set(content_at.advance(content_raw));
                self.token_end.set(l.span.end);
                match h.end_cdata.get() {
                    // SAFETY: registered EndCdataSectionHandler.
                    Some(f) => unsafe { f(ud) },
                    None => {
                        self.to_default("]]>");
                    }
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
                if let Some(f) = h.entity_decl.get() {
                    let (vptr, vlen) = value.as_deref().map_or((ptr::null(), 0), |v| (v.as_ptr().cast(), v.len() as c_int));
                    // SAFETY: registered EntityDeclHandler; the value is
                    // valid for its length (not NUL-terminated, as in
                    // libexpat), the rest valid C strings or null.
                    unsafe { f(ud, n.as_ptr(), parameter as c_int, vptr, vlen, ptr_of(&base), ptr_of(&s), ptr_of(&p), ptr_of(&no)) };
                } else if let (Some(f), Some(_)) = (h.unparsed_entity_decl.get(), &no) {
                    // SAFETY: registered UnparsedEntityDeclHandler; valid C
                    // strings (or null).
                    unsafe { f(ud, n.as_ptr(), ptr_of(&base), ptr_of(&s), ptr_of(&p), ptr_of(&no)) };
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
    fn character_data(&self, text: &str, raw: &str, start: Position) {
        let from_source = if raw.contains('\r') {
            raw.replace("\r\n", "\n").replace('\r', "\n") == text
        } else {
            raw == text
        };
        if from_source {
            let bytes = raw.as_bytes();
            let (mut at, mut piece_start, mut i) = (start, 0, 0);
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
                        self.deliver_chars(part, raw);
                    }
                }
            }
        }
    }

    /// One piece of character data from the source at `at`; returns the
    /// position after it.
    fn char_piece(&self, text: &str, raw: &str, at: Position) -> Position {
        let end = at.advance(raw);
        self.pos.set(at);
        self.token_end.set(end);
        self.deliver_chars(text, raw);
        end
    }

    fn deliver_chars(&self, text: &str, raw: &str) {
        match self.handlers.character_data.get() {
            Some(f) => {
                // SAFETY: registered CharacterDataHandler; `text` is valid
                // for its length during the call.
                unsafe { f(self.ud(), text.as_ptr().cast(), text.len() as c_int) };
            }
            None => {
                self.to_default(raw);
            }
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
        Status::Parsing => {
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
    unsafe { shim(parser) }.map_or(0, |p| p.pos.get().column.saturating_sub(1) as XML_Size)
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetCurrentByteIndex(parser: XML_Parser) -> XML_Index {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(-1, |p| p.pos.get().byte_offset as XML_Index)
}

#[no_mangle]
pub unsafe extern "C" fn XML_GetSpecifiedAttributeCount(parser: XML_Parser) -> c_int {
    // SAFETY: see `shim`.
    unsafe { shim(parser) }.map_or(-1, |p| p.specified_atts.get())
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
    _parser: XML_Parser,
    _context: *const XML_Char,
    _encoding: *const XML_Char,
) -> XML_Parser {
    // External entity parsers come in phase 5.
    ptr::null_mut()
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
