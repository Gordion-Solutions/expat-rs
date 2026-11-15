//! `expat_rs._pyexpat`: the API of CPython's `pyexpat` module
//! (`xml.parsers.expat`), on expat-capi's libexpat functions and so on the
//! expat-rs parser.
//!
//! Written from the `xml.parsers.expat` documentation and checked against
//! the standard library's module by running the same inputs through both.
//!
//! The C parser calls back into this module through `extern "C"` handlers
//! with the `XmlParser` as user data. Every callback runs inside `Parse`,
//! which holds the GIL, so the handlers attach to the interpreter that is
//! already running. `XmlParser` is only ever borrowed shared (handlers may
//! read and set attributes while a parse is under way): its state is in
//! `Cell`s and `RefCell`s, and no `RefCell` borrow is held across a call
//! into Python.

use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

use expat::*;
use pyo3::exceptions::{PyAttributeError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyCapsule, PyDict, PyList, PyString, PyTuple};

mod capi;

pyo3::create_exception!(pyexpat, ExpatError, pyo3::exceptions::PyException);

/// The handlers a parser can have, in the order `pyexpat` lists them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
enum H {
    StartElement,
    EndElement,
    ProcessingInstruction,
    CharacterData,
    UnparsedEntityDecl,
    NotationDecl,
    StartNamespaceDecl,
    EndNamespaceDecl,
    Comment,
    StartCdataSection,
    EndCdataSection,
    Default,
    DefaultExpand,
    NotStandalone,
    ExternalEntityRef,
    StartDoctypeDecl,
    EndDoctypeDecl,
    EntityDecl,
    XmlDecl,
    ElementDecl,
    AttlistDecl,
    SkippedEntity,
}

const HANDLERS: [(&str, H); 22] = [
    ("StartElementHandler", H::StartElement),
    ("EndElementHandler", H::EndElement),
    ("ProcessingInstructionHandler", H::ProcessingInstruction),
    ("CharacterDataHandler", H::CharacterData),
    ("UnparsedEntityDeclHandler", H::UnparsedEntityDecl),
    ("NotationDeclHandler", H::NotationDecl),
    ("StartNamespaceDeclHandler", H::StartNamespaceDecl),
    ("EndNamespaceDeclHandler", H::EndNamespaceDecl),
    ("CommentHandler", H::Comment),
    ("StartCdataSectionHandler", H::StartCdataSection),
    ("EndCdataSectionHandler", H::EndCdataSection),
    ("DefaultHandler", H::Default),
    ("DefaultHandlerExpand", H::DefaultExpand),
    ("NotStandaloneHandler", H::NotStandalone),
    ("ExternalEntityRefHandler", H::ExternalEntityRef),
    ("StartDoctypeDeclHandler", H::StartDoctypeDecl),
    ("EndDoctypeDeclHandler", H::EndDoctypeDecl),
    ("EntityDeclHandler", H::EntityDecl),
    ("XmlDeclHandler", H::XmlDecl),
    ("ElementDeclHandler", H::ElementDecl),
    ("AttlistDeclHandler", H::AttlistDecl),
    ("SkippedEntityHandler", H::SkippedEntity),
];

/// A Python handler and which one it is (for the traceback frame).
struct Handler {
    obj: Py<PyAny>,
    which: H,
}

fn handler_index(name: &str) -> Option<H> {
    HANDLERS.iter().find(|(n, _)| *n == name).map(|&(_, h)| h)
}

/// Size of the reads `ParseFile` makes.
const READ_SIZE: usize = 2048;
/// `buffer_size`'s default.
const DEFAULT_BUFFER_SIZE: usize = 8192;

#[pyclass(name = "xmlparser", module = "pyexpat", unsendable, weakref)]
pub struct XmlParser {
    itself: Cell<XML_Parser>,
    handlers: RefCell<Vec<Option<Py<PyAny>>>>,
    /// The `intern` dictionary, or None when names aren't interned.
    intern: Option<Py<PyAny>>,
    /// Character data held while `buffer_text` is on.
    buffer: RefCell<Option<String>>,
    buffer_size: Cell<usize>,
    ordered_attributes: Cell<bool>,
    specified_attributes: Cell<bool>,
    namespace_prefixes: Cell<bool>,
    reparse_deferral: Cell<bool>,
    /// An exception a handler raised; `Parse` raises it once the C parser
    /// has stopped.
    error: RefCell<Option<PyErr>>,
    /// True while a handler is running, so `Parse` can't be re-entered.
    in_callback: Cell<bool>,
    /// The parser an external entity parser was created from, kept alive
    /// as long as this one (libexpat requires the parent to outlive it).
    #[allow(dead_code)]
    parent: Option<Py<XmlParser>>,
}

impl Drop for XmlParser {
    fn drop(&mut self) {
        let p = self.itself.replace(ptr::null_mut());
        if !p.is_null() {
            // SAFETY: `p` came from XML_ParserCreate_MM or
            // XML_ExternalEntityParserCreate and is freed once, here.
            unsafe { XML_ParserFree(p) };
        }
    }
}

/// A C string from the parser as Python text. The parser hands out UTF-8.
fn text(s: *const c_char) -> Option<String> {
    if s.is_null() {
        return None;
    }
    // SAFETY: a non-null string from the parser is NUL-terminated.
    let bytes = unsafe { CStr::from_ptr(s) }.to_bytes();
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// `len` bytes from the parser as Python text.
fn text_len(s: *const c_char, len: c_int) -> String {
    if s.is_null() || len <= 0 {
        return String::new();
    }
    // SAFETY: the parser passes `len` readable bytes at `s`.
    let bytes = unsafe { std::slice::from_raw_parts(s as *const u8, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

impl XmlParser {
    fn new(itself: XML_Parser, intern: Option<Py<PyAny>>, parent: Option<Py<XmlParser>>) -> Self {
        XmlParser {
            itself: Cell::new(itself),
            handlers: RefCell::new((0..HANDLERS.len()).map(|_| None).collect()),
            intern,
            buffer: RefCell::new(None),
            buffer_size: Cell::new(DEFAULT_BUFFER_SIZE),
            ordered_attributes: Cell::new(false),
            specified_attributes: Cell::new(false),
            namespace_prefixes: Cell::new(false),
            reparse_deferral: Cell::new(true),
            error: RefCell::new(None),
            in_callback: Cell::new(false),
            parent,
        }
    }

    fn handler(&self, py: Python<'_>, h: H) -> Option<Handler> {
        self.handlers.borrow()[h as usize].as_ref().map(|o| Handler { obj: o.clone_ref(py), which: h })
    }

    fn failed(&self) -> bool {
        self.error.borrow().is_some()
    }

    /// Records a handler's exception and stops the parse.
    fn fail(&self, err: PyErr) {
        if self.error.borrow().is_none() {
            *self.error.borrow_mut() = Some(err);
        }
        // SAFETY: the parser is live while `self` is.
        unsafe { XML_StopParser(self.itself.get(), XML_FALSE) };
    }

    /// The Python handler to call for `h`, after delivering any buffered
    /// character data; None when there is none or a handler has failed.
    fn ready(&self, py: Python<'_>, h: H) -> Option<Handler> {
        if self.failed() {
            return None;
        }
        let handler = self.handler(py, h)?;
        self.flush(py).ok()?;
        if self.failed() {
            return None;
        }
        Some(handler)
    }

    /// Calls a handler, recording any exception it raises.
    fn call<'py, A>(&self, py: Python<'py>, handler: &Handler, args: A) -> Option<Bound<'py, PyAny>>
    where
        A: pyo3::call::PyCallArgs<'py>,
    {
        let was = self.in_callback.replace(true);
        let result = handler.obj.bind(py).call1(args);
        self.in_callback.set(was);
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.fail(with_handler_frame(py, handler.which, e));
                None
            }
        }
    }

    /// Delivers buffered character data to the CharacterDataHandler.
    fn flush(&self, py: Python<'_>) -> PyResult<()> {
        let pending = match self.buffer.borrow_mut().as_mut() {
            Some(b) if !b.is_empty() => std::mem::take(b),
            _ => return Ok(()),
        };
        if let Some(h) = self.handler(py, H::CharacterData) {
            if self.call(py, &h, (pending,)).is_none() {
                return Err(self.error.borrow().as_ref().map(|e| e.clone_ref(py)).unwrap());
            }
        }
        Ok(())
    }

    /// A name, interned through the `intern` dictionary if there is one.
    fn name<'py>(&self, py: Python<'py>, s: *const c_char) -> PyResult<Bound<'py, PyAny>> {
        let Some(t) = text(s) else { return Ok(py.None().into_bound(py)) };
        let value = PyString::new(py, &t).into_any();
        match &self.intern {
            Some(d) => d.bind(py).call_method1("setdefault", (value.clone(), value)),
            None => Ok(value),
        }
    }

    /// Installs or removes the C callback for `h`.
    fn set_c_handler(&self, h: H, on: bool) {
        let p = self.itself.get();
        macro_rules! pick {
            ($f:expr) => {
                if on {
                    Some($f)
                } else {
                    None
                }
            };
        }
        // SAFETY: the parser is live; each callback has the type its setter takes.
        unsafe {
            match h {
                H::StartElement => XML_SetStartElementHandler(p, pick!(cb_start_element)),
                H::EndElement => XML_SetEndElementHandler(p, pick!(cb_end_element)),
                H::ProcessingInstruction => XML_SetProcessingInstructionHandler(p, pick!(cb_pi)),
                H::CharacterData => XML_SetCharacterDataHandler(p, pick!(cb_character_data)),
                H::UnparsedEntityDecl => XML_SetUnparsedEntityDeclHandler(p, pick!(cb_unparsed_entity_decl)),
                H::NotationDecl => XML_SetNotationDeclHandler(p, pick!(cb_notation_decl)),
                H::StartNamespaceDecl => XML_SetStartNamespaceDeclHandler(p, pick!(cb_start_namespace)),
                H::EndNamespaceDecl => XML_SetEndNamespaceDeclHandler(p, pick!(cb_end_namespace)),
                H::Comment => XML_SetCommentHandler(p, pick!(cb_comment)),
                H::StartCdataSection => XML_SetStartCdataSectionHandler(p, pick!(cb_start_cdata)),
                H::EndCdataSection => XML_SetEndCdataSectionHandler(p, pick!(cb_end_cdata)),
                H::Default => XML_SetDefaultHandler(p, pick!(cb_default)),
                H::DefaultExpand => XML_SetDefaultHandlerExpand(p, pick!(cb_default_expand)),
                H::NotStandalone => XML_SetNotStandaloneHandler(p, pick!(cb_not_standalone)),
                H::ExternalEntityRef => XML_SetExternalEntityRefHandler(p, pick!(cb_external_entity_ref)),
                H::StartDoctypeDecl => XML_SetStartDoctypeDeclHandler(p, pick!(cb_start_doctype)),
                H::EndDoctypeDecl => XML_SetEndDoctypeDeclHandler(p, pick!(cb_end_doctype)),
                H::EntityDecl => XML_SetEntityDeclHandler(p, pick!(cb_entity_decl)),
                H::XmlDecl => XML_SetXmlDeclHandler(p, pick!(cb_xml_decl)),
                H::ElementDecl => XML_SetElementDeclHandler(p, pick!(cb_element_decl)),
                H::AttlistDecl => XML_SetAttlistDeclHandler(p, pick!(cb_attlist_decl)),
                H::SkippedEntity => XML_SetSkippedEntityHandler(p, pick!(cb_skipped_entity)),
            }
        }
    }

    /// An ExpatError for a bad argument, as XML_ERROR_INVALID_ARGUMENT.
    fn argument_error(&self, py: Python<'_>, message: &str) -> PyErr {
        let err = ExpatError::new_err(message.to_owned());
        let value = err.value(py);
        let _ = value.setattr("code", XML_ERROR_INVALID_ARGUMENT);
        let _ = value.setattr("lineno", self.error_line_number());
        let _ = value.setattr("offset", self.error_column_number());
        err
    }

    /// The ExpatError for the parser's current error.
    fn error_from_parser(&self, py: Python<'_>) -> PyErr {
        let p = self.itself.get();
        // SAFETY: the parser is live.
        let (code, line, column) =
            unsafe { (XML_GetErrorCode(p), XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p)) };
        let message = text(XML_ErrorString(code)).unwrap_or_default();
        let err = ExpatError::new_err(format!("{message}: line {line}, column {column}"));
        let value = err.value(py);
        let _ = value.setattr("code", code);
        let _ = value.setattr("lineno", line);
        let _ = value.setattr("offset", column);
        err
    }

    /// One call of XML_Parse, then the result as `Parse` reports it.
    fn parse_bytes(&self, py: Python<'_>, data: &[u8], is_final: bool) -> PyResult<i32> {
        let p = self.itself.get();
        let mut rest = data;
        let mut status;
        loop {
            let n = rest.len().min(i32::MAX as usize);
            let last = n == rest.len();
            // SAFETY: `rest` has at least `n` bytes; the parser is live.
            status = unsafe {
                XML_Parse(p, rest.as_ptr() as *const c_char, n as c_int, (is_final && last) as c_int)
            };
            rest = &rest[n..];
            if last || status == 0 || self.failed() {
                break;
            }
        }
        if let Some(e) = self.error.borrow_mut().take() {
            return Err(e);
        }
        if status == 0 {
            return Err(self.error_from_parser(py));
        }
        self.flush(py)?;
        Ok(status)
    }
}

/// The `XmlParser` a callback's user data points at.
///
/// # Safety
/// `ud` must be the user data set by `make_parser`: a live `XmlParser`.
unsafe fn parser_of<'a>(ud: *mut c_void) -> &'a XmlParser {
    unsafe { &*(ud as *const XmlParser) }
}

/// Runs a callback body attached to the interpreter, with the parser the
/// user data points at.
fn with_parser<R: Default>(ud: *mut c_void, f: impl FnOnce(Python<'_>, &XmlParser) -> R) -> R {
    // SAFETY: every callback is installed on a parser whose user data is its XmlParser.
    let this = unsafe { parser_of(ud) };
    Python::attach(|py| f(py, this))
}

unsafe extern "C" fn cb_start_element(ud: *mut c_void, name: *const c_char, atts: *mut *const c_char) {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::StartElement) else { return };
        let build = || -> PyResult<(Bound<'_, PyAny>, Bound<'_, PyAny>)> {
            let name = this.name(py, name)?;
            let mut count = 0usize;
            // SAFETY: `atts` is a NULL-terminated array of name/value pairs.
            while !unsafe { *atts.add(count) }.is_null() {
                count += 1;
            }
            if this.specified_attributes.get() {
                // SAFETY: the parser is live.
                count = count.min(unsafe { XML_GetSpecifiedAttributeCount(this.itself.get()) }.max(0) as usize);
            }
            let attrs: Bound<'_, PyAny> = if this.ordered_attributes.get() {
                let list = PyList::empty(py);
                for i in 0..count {
                    // SAFETY: index below the terminator.
                    let s = unsafe { *atts.add(i) };
                    if i % 2 == 0 { list.append(this.name(py, s)?)? } else { list.append(text(s))? }
                }
                list.into_any()
            } else {
                let dict = PyDict::new(py);
                for i in (0..count).step_by(2) {
                    // SAFETY: indexes below the terminator, in pairs.
                    let (n, v) = unsafe { (*atts.add(i), *atts.add(i + 1)) };
                    dict.set_item(this.name(py, n)?, text(v))?;
                }
                dict.into_any()
            };
            Ok((name, attrs))
        };
        match build() {
            Ok(args) => {
                this.call(py, &h, args);
            }
            Err(e) => this.fail(e),
        }
    })
}

/// Callbacks whose arguments are all names or strings.
macro_rules! simple_callback {
    ($fn_name:ident, $h:expr, ($($arg:ident: $conv:ident),*)) => {
        unsafe extern "C" fn $fn_name(ud: *mut c_void, $($arg: *const c_char),*) {
            with_parser(ud, |py, this| {
                let Some(h) = this.ready(py, $h) else { return };
                // A closure for `?`, which some conversions don't use.
                #[allow(clippy::redundant_closure_call)]
                let args = (|| -> PyResult<_> { Ok(($(simple_callback!(@conv this, py, $conv, $arg),)*)) })();
                match args {
                    Ok(args) => { this.call(py, &h, args); }
                    Err(e) => this.fail(e),
                }
            })
        }
    };
    (@conv $this:ident, $py:ident, name, $arg:ident) => { $this.name($py, $arg)? };
    (@conv $this:ident, $py:ident, text, $arg:ident) => { text($arg) };
}

simple_callback!(cb_end_element, H::EndElement, (name: name));
simple_callback!(cb_pi, H::ProcessingInstruction, (target: name, data: text));
simple_callback!(cb_unparsed_entity_decl, H::UnparsedEntityDecl,
    (entity: name, base: text, system_id: text, public_id: text, notation: name));
simple_callback!(cb_notation_decl, H::NotationDecl, (notation: name, base: text, system_id: text, public_id: text));
simple_callback!(cb_start_namespace, H::StartNamespaceDecl, (prefix: name, uri: name));
simple_callback!(cb_end_namespace, H::EndNamespaceDecl, (prefix: name));
simple_callback!(cb_comment, H::Comment, (data: text));

unsafe extern "C" fn cb_character_data(ud: *mut c_void, s: *const c_char, len: c_int) {
    with_parser(ud, |py, this| {
        if this.failed() || this.handlers.borrow()[H::CharacterData as usize].is_none() {
            return;
        }
        let data = text_len(s, len);
        let buffered = this.buffer.borrow().is_some();
        if !buffered {
            let Some(h) = this.handler(py, H::CharacterData) else { return };
            this.call(py, &h, (data,));
            return;
        }
        let size = this.buffer_size.get();
        let used = this.buffer.borrow().as_ref().map_or(0, |b| b.len());
        if used + data.len() > size && this.flush(py).is_err() {
            return;
        }
        if data.len() > size {
            // Too big to buffer: delivered on its own.
            let Some(h) = this.handler(py, H::CharacterData) else { return };
            this.call(py, &h, (data,));
        } else if let Some(b) = this.buffer.borrow_mut().as_mut() {
            b.push_str(&data);
        }
    })
}

unsafe extern "C" fn cb_start_cdata(ud: *mut c_void) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::StartCdataSection) {
            this.call(py, &h, ());
        }
    })
}

unsafe extern "C" fn cb_end_cdata(ud: *mut c_void) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::EndCdataSection) {
            this.call(py, &h, ());
        }
    })
}

unsafe extern "C" fn cb_end_doctype(ud: *mut c_void) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::EndDoctypeDecl) {
            this.call(py, &h, ());
        }
    })
}

unsafe extern "C" fn cb_default(ud: *mut c_void, s: *const c_char, len: c_int) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::Default) {
            this.call(py, &h, (text_len(s, len),));
        }
    })
}

unsafe extern "C" fn cb_default_expand(ud: *mut c_void, s: *const c_char, len: c_int) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::DefaultExpand) {
            this.call(py, &h, (text_len(s, len),));
        }
    })
}

/// A handler's integer result. Anything else is a TypeError, noted with
/// the handler's name from Python 3.15 as there.
fn handler_int(py: Python<'_>, this: &XmlParser, h: H, v: &Bound<'_, PyAny>) -> c_int {
    let result = if v.is_instance_of::<pyo3::types::PyInt>() {
        v.extract::<c_int>()
    } else {
        Err(PyTypeError::new_err(format!(
            "'{}' object cannot be interpreted as an integer",
            v.get_type().name().map(|n| n.to_string()).unwrap_or_default()
        )))
    };
    match result {
        Ok(rc) => rc,
        Err(e) => {
            if py.version_info() >= (3, 15) {
                let note = format!("invalid '{}' event handler return value", frame_name(h));
                let _ = e.value(py).call_method1("add_note", (note,));
            }
            this.fail(e);
            0
        }
    }
}

unsafe extern "C" fn cb_not_standalone(ud: *mut c_void) -> c_int {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::NotStandalone) else { return 0 };
        match this.call(py, &h, ()) {
            Some(v) => handler_int(py, this, H::NotStandalone, &v),
            None => 0,
        }
    })
}

unsafe extern "C" fn cb_external_entity_ref(
    parser: XML_Parser,
    context: *const c_char,
    base: *const c_char,
    system_id: *const c_char,
    public_id: *const c_char,
) -> c_int {
    // SAFETY: the user data is the first field of the parser, as libexpat's
    // XML_GetUserData macro reads it.
    let ud = unsafe { *(parser as *const *mut c_void) };
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::ExternalEntityRef) else { return 0 };
        match this.call(py, &h, (text(context), text(base), text(system_id), text(public_id))) {
            Some(v) => handler_int(py, this, H::ExternalEntityRef, &v),
            None => 0,
        }
    })
}

unsafe extern "C" fn cb_start_doctype(
    ud: *mut c_void,
    name: *const c_char,
    system_id: *const c_char,
    public_id: *const c_char,
    has_internal_subset: c_int,
) {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::StartDoctypeDecl) else { return };
        match this.name(py, name) {
            Ok(n) => {
                this.call(py, &h, (n, text(system_id), text(public_id), has_internal_subset));
            }
            Err(e) => this.fail(e),
        }
    })
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn cb_entity_decl(
    ud: *mut c_void,
    name: *const c_char,
    is_parameter: c_int,
    value: *const c_char,
    value_len: c_int,
    base: *const c_char,
    system_id: *const c_char,
    public_id: *const c_char,
    notation: *const c_char,
) {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::EntityDecl) else { return };
        let value = (!value.is_null()).then(|| text_len(value, value_len));
        let args = (|| -> PyResult<_> {
            Ok((this.name(py, name)?, is_parameter, value, text(base), text(system_id), text(public_id), this.name(py, notation)?))
        })();
        match args {
            Ok(args) => {
                this.call(py, &h, args);
            }
            Err(e) => this.fail(e),
        }
    })
}

unsafe extern "C" fn cb_xml_decl(ud: *mut c_void, version: *const c_char, encoding: *const c_char, standalone: c_int) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::XmlDecl) {
            this.call(py, &h, (text(version), text(encoding), standalone));
        }
    })
}

/// A content model as `pyexpat` gives it: (type, quantifier, name, children).
///
/// Built without recursion; a model nested deeper than Python's recursion
/// limit raises RecursionError, as pyexpat's recursive conversion does.
fn model_tuple<'py>(py: Python<'py>, this: &XmlParser, root: &XML_Content) -> PyResult<Bound<'py, PyTuple>> {
    let limit: usize = py.import("sys")?.call_method0("getrecursionlimit")?.extract()?;
    // Each frame: a node and the tuples built so far for its children.
    let mut stack: Vec<(&XML_Content, Vec<Bound<'py, PyAny>>)> = vec![(root, Vec::new())];
    loop {
        let depth = stack.len();
        let (node, done) = stack.last_mut().expect("never empty here");
        let node: &XML_Content = node;
        if done.len() < node.numchildren as usize {
            if depth >= limit {
                return Err(pyo3::exceptions::PyRecursionError::new_err(
                    "maximum recursion depth exceeded while converting a content model",
                ));
            }
            // SAFETY: `children` has `numchildren` entries.
            let child = unsafe { &*node.children.add(done.len()) };
            stack.push((child, Vec::new()));
            continue;
        }
        let (node, children) = stack.pop().expect("checked above");
        let tuple = PyTuple::new(
            py,
            [
                node.type_.into_pyobject(py)?.into_any(),
                node.quant.into_pyobject(py)?.into_any(),
                this.name(py, node.name)?,
                PyTuple::new(py, children)?.into_any(),
            ],
        )?;
        match stack.last_mut() {
            Some((_, parent_done)) => parent_done.push(tuple.into_any()),
            None => return Ok(tuple),
        }
    }
}

unsafe extern "C" fn cb_element_decl(ud: *mut c_void, name: *const c_char, model: *mut XML_Content) {
    with_parser(ud, |py, this| {
        if let Some(h) = this.ready(py, H::ElementDecl) {
            // SAFETY: `model` is a live content model owned by us until freed.
            let args = (|| -> PyResult<_> { Ok((this.name(py, name)?, model_tuple(py, this, unsafe { &*model })?)) })();
            match args {
                Ok(args) => {
                    this.call(py, &h, args);
                }
                Err(e) => this.fail(e),
            }
        }
        // SAFETY: the handler owns the model and frees it once.
        unsafe { XML_FreeContentModel(this.itself.get(), model) };
    })
}

unsafe extern "C" fn cb_attlist_decl(
    ud: *mut c_void,
    element: *const c_char,
    attribute: *const c_char,
    att_type: *const c_char,
    default: *const c_char,
    required: c_int,
) {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::AttlistDecl) else { return };
        let args =
            (|| -> PyResult<_> { Ok((this.name(py, element)?, this.name(py, attribute)?, text(att_type), text(default), required)) })();
        match args {
            Ok(args) => {
                this.call(py, &h, args);
            }
            Err(e) => this.fail(e),
        }
    })
}

unsafe extern "C" fn cb_skipped_entity(ud: *mut c_void, name: *const c_char, is_parameter: c_int) {
    with_parser(ud, |py, this| {
        let Some(h) = this.ready(py, H::SkippedEntity) else { return };
        match this.name(py, name) {
            Ok(n) => {
                this.call(py, &h, (n, is_parameter));
            }
            Err(e) => this.fail(e),
        }
    })
}

/// The byte map for an encoding Python knows: each byte decoded on its own
/// (U+FFFD for bytes the codec rejects). Multi-byte encodings aren't
/// supported, as in CPython up to 3.15.
fn byte_map(py: Python<'_>, name: &str) -> PyResult<[c_int; 256]> {
    let all: Vec<u8> = (0..=255).collect();
    let decoded = PyBytes::new(py, &all).call_method1("decode", (name, "replace"))?;
    let decoded: String = decoded.extract()?;
    if decoded.chars().count() != 256 {
        return Err(PyValueError::new_err("multi-byte encodings are not supported"));
    }
    let mut map = [0; 256];
    for (slot, c) in map.iter_mut().zip(decoded.chars()) {
        *slot = if c == '\u{FFFD}' { -1 } else { c as c_int };
    }
    Ok(map)
}

/// Fill in `info` for the encoding `name`, as `pyexpat` does: a
/// single-byte encoding Python has a codec for.
///
/// # Safety
/// `name` is NULL or a NUL-terminated string and `info` the parser's
/// XML_Encoding to fill in, as passed to an unknown encoding handler.
pub(crate) unsafe fn fill_encoding(py: Python<'_>, name: *const c_char, info: *mut XML_Encoding) -> PyResult<()> {
    let map = byte_map(py, &text(name).unwrap_or_default())?;
    // SAFETY: as the caller guarantees.
    unsafe {
        (*info).map = map;
        (*info).data = ptr::null_mut();
        (*info).convert = None;
        (*info).release = None;
    }
    Ok(())
}

unsafe extern "C" fn cb_unknown_encoding(ud: *mut c_void, name: *const c_char, info: *mut XML_Encoding) -> c_int {
    with_parser(ud, |py, this| {
        // SAFETY: the parser passes the encoding's name and its XML_Encoding.
        match unsafe { fill_encoding(py, name, info) } {
            Ok(()) => 1,
            Err(e) => {
                this.fail(e);
                0
            }
        }
    })
}

/// A non-negative int that fits C's unsigned long long, with the errors
/// CPython's argument conversion gives.
fn unsigned_long_long(v: &Bound<'_, PyAny>) -> PyResult<u64> {
    if !v.is_instance_of::<pyo3::types::PyInt>() {
        return Err(PyTypeError::new_err(format!("'{}' object cannot be interpreted as an integer", v.get_type().name()?)));
    }
    if v.lt(0)? {
        return Err(PyValueError::new_err("Cannot convert negative int"));
    }
    v.extract::<u64>()
        .map_err(|_| pyo3::exceptions::PyOverflowError::new_err("Python int too large for C unsigned long long"))
}

/// The name pyexpat gives a handler's frame in tracebacks.
fn frame_name(h: H) -> &'static str {
    let (name, _) = HANDLERS[h as usize];
    name.strip_suffix("Handler").unwrap_or(name)
}

/// Adds a frame for the handler, as `Modules/pyexpat.c`, between the
/// handler's own frames and the caller of `Parse`, as CPython's pyexpat
/// does, so tracebacks show which handler raised.
fn with_handler_frame(py: Python<'_>, h: H, err: PyErr) -> PyErr {
    let add = || -> PyResult<PyErr> {
        let name = frame_name(h);
        let source = format!("def {name}():\n    return __import__('sys')._getframe()\n");
        let builtins = py.import("builtins")?;
        let code = builtins.call_method1("compile", (source, "Modules/pyexpat.c", "exec"))?;
        let namespace = PyDict::new(py);
        builtins.call_method1("exec", (code, &namespace))?;
        let frame = namespace.get_item(name)?.expect("defined above").call0()?;
        let tb_type = py.import("types")?.getattr("TracebackType")?;
        let tb = tb_type.call1((err.traceback(py), &frame, frame.getattr("f_lasti")?, frame.getattr("f_lineno")?))?;
        let value = err.value(py).call_method1("with_traceback", (tb,))?;
        // A new PyErr from the value, so the new traceback is the one raised.
        Ok(PyErr::from_value(value))
    };
    add().unwrap_or(err)
}

/// Creates the Python object for a C parser and points the parser's user
/// data at it.
fn make_parser(
    py: Python<'_>,
    itself: XML_Parser,
    intern: Option<Py<PyAny>>,
    parent: Option<Py<XmlParser>>,
) -> PyResult<Py<XmlParser>> {
    if itself.is_null() {
        return Err(pyo3::exceptions::PyMemoryError::new_err(()));
    }
    let obj = Py::new(py, XmlParser::new(itself, intern, parent))?;
    let addr = {
        let r = obj.borrow(py);
        &*r as *const XmlParser as *mut c_void
    };
    // SAFETY: the object's data never moves while it lives, and the parser
    // is freed (in Drop) before the object goes.
    unsafe {
        XML_SetUserData(itself, addr);
        XML_SetUnknownEncodingHandler(itself, Some(cb_unknown_encoding), addr);
    }
    Ok(obj)
}

/// Parse text: str as UTF-8 (telling the parser so), or any bytes-like object.
fn data_bytes(parser: &XmlParser, data: &Bound<'_, PyAny>) -> PyResult<Vec<u8>> {
    if let Ok(s) = data.cast::<PyString>() {
        let utf8 = CString::new("utf-8").unwrap();
        // SAFETY: the parser is live.
        unsafe { XML_SetEncoding(parser.itself.get(), utf8.as_ptr()) };
        return Ok(s.to_str()?.as_bytes().to_vec());
    }
    if let Ok(b) = data.cast::<PyBytes>() {
        return Ok(b.as_bytes().to_vec());
    }
    // Any other object with the buffer protocol (bytearray, memoryview,
    // array, ...). Through memoryview: the stable ABI's buffer functions
    // start at 3.11.
    let type_name = data.get_type().name()?.to_string();
    let view = pyo3::types::PyMemoryView::from(data)
        .map_err(|_| PyTypeError::new_err(format!("a bytes-like object is required, not '{type_name}'")))?;
    let bytes = view.call_method0("tobytes")?;
    Ok(bytes.cast::<PyBytes>()?.as_bytes().to_vec())
}

#[pymethods]
impl XmlParser {
    /// Parse(data, isfinal=False): parse the contents of a string or bytes.
    #[pyo3(name = "Parse", signature = (data, isfinal = false))]
    fn parse(slf: &Bound<'_, Self>, data: &Bound<'_, PyAny>, isfinal: bool) -> PyResult<i32> {
        let this = slf.borrow();
        if this.in_callback.get() {
            return Err(PyRuntimeError::new_err("cannot be called from a handler"));
        }
        let bytes = data_bytes(&this, data)?;
        this.parse_bytes(slf.py(), &bytes, isfinal)
    }

    /// ParseFile(file): parse XML read from a file object.
    #[pyo3(name = "ParseFile")]
    fn parse_file(slf: &Bound<'_, Self>, file: &Bound<'_, PyAny>) -> PyResult<i32> {
        let py = slf.py();
        let read = file
            .getattr("read")
            .map_err(|_| PyTypeError::new_err("argument must have 'read' attribute"))?;
        let this = slf.borrow();
        loop {
            let chunk = read.call1((READ_SIZE,))?;
            let bytes = chunk.cast::<PyBytes>().map_err(|_| {
                PyTypeError::new_err(format!(
                    "read() did not return a bytes object (type={})",
                    chunk.get_type().name().map(|n| n.to_string()).unwrap_or_default()
                ))
            })?;
            let data = bytes.as_bytes();
            let last = data.is_empty();
            let status = this.parse_bytes(py, data, last)?;
            if last {
                return Ok(status);
            }
        }
    }

    /// SetBase(base): set the base URL for resolving relative URIs.
    #[pyo3(name = "SetBase")]
    fn set_base(&self, base: &str) -> PyResult<()> {
        let base = CString::new(base).map_err(|e| PyValueError::new_err(e.to_string()))?;
        // SAFETY: the parser is live; it copies the string.
        if unsafe { XML_SetBase(self.itself.get(), base.as_ptr()) } == 0 {
            return Err(pyo3::exceptions::PyMemoryError::new_err(()));
        }
        Ok(())
    }

    /// GetBase(): the base URL, or None.
    #[pyo3(name = "GetBase")]
    fn get_base(&self) -> Option<String> {
        // SAFETY: the parser is live.
        text(unsafe { XML_GetBase(self.itself.get()) })
    }

    /// GetInputContext(): the input around the current event, or None.
    #[pyo3(name = "GetInputContext")]
    fn get_input_context(&self) -> Option<Vec<u8>> {
        None
    }

    /// SetParamEntityParsing(flag): whether parameter entities (and the
    /// external DTD subset) are parsed.
    #[pyo3(name = "SetParamEntityParsing")]
    fn set_param_entity_parsing(&self, flag: c_int) -> c_int {
        // SAFETY: the parser is live.
        unsafe { XML_SetParamEntityParsing(self.itself.get(), flag) }
    }

    /// UseForeignDTD(flag=True): load an external DTD even without a DOCTYPE.
    #[pyo3(name = "UseForeignDTD", signature = (flag = true))]
    fn use_foreign_dtd(&self, py: Python<'_>, flag: bool) -> PyResult<()> {
        // SAFETY: the parser is live.
        let rc = unsafe { XML_UseForeignDTD(self.itself.get(), flag as XML_Bool) };
        if rc != 0 {
            return Err(self.error_from_parser(py));
        }
        Ok(())
    }

    /// GetReparseDeferralEnabled(): whether reparse deferral is on.
    #[pyo3(name = "GetReparseDeferralEnabled")]
    fn get_reparse_deferral_enabled(&self) -> bool {
        self.reparse_deferral.get()
    }

    /// SetReparseDeferralEnabled(enabled): turn reparse deferral on or off.
    #[pyo3(name = "SetReparseDeferralEnabled")]
    fn set_reparse_deferral_enabled(&self, enabled: bool) {
        // SAFETY: the parser is live.
        unsafe { XML_SetReparseDeferralEnabled(self.itself.get(), enabled as XML_Bool) };
        self.reparse_deferral.set(enabled);
    }

    /// SetBillionLaughsAttackProtectionActivationThreshold(threshold): input
    /// size from which the amplification limit applies.
    #[pyo3(name = "SetBillionLaughsAttackProtectionActivationThreshold")]
    fn set_bl_activation_threshold(&self, py: Python<'_>, threshold: &Bound<'_, PyAny>) -> PyResult<()> {
        let threshold = unsigned_long_long(threshold)?;
        // SAFETY: the parser is live.
        let ok = unsafe { XML_SetBillionLaughsAttackProtectionActivationThreshold(self.itself.get(), threshold) };
        if ok == 0 {
            return Err(self.argument_error(py, "parser must be a root parser"));
        }
        Ok(())
    }

    /// SetBillionLaughsAttackProtectionMaximumAmplification(max_factor):
    /// the largest allowed ratio of expanded to direct input.
    #[pyo3(name = "SetBillionLaughsAttackProtectionMaximumAmplification")]
    fn set_bl_maximum_amplification(&self, py: Python<'_>, max_factor: &Bound<'_, PyAny>) -> PyResult<()> {
        if max_factor.is_instance_of::<PyString>() || max_factor.is_none() {
            return Err(PyTypeError::new_err(format!("must be real number, not {}", max_factor.get_type().name()?)));
        }
        let factor: f64 = max_factor.extract()?;
        if self.parent.is_some() {
            return Err(self.argument_error(py, "parser must be a root parser"));
        }
        if factor.is_nan() || factor < 1.0 {
            return Err(self.argument_error(py, "'max_factor' must be at least 1.0"));
        }
        // SAFETY: the parser is live.
        unsafe { XML_SetBillionLaughsAttackProtectionMaximumAmplification(self.itself.get(), factor as f32) };
        Ok(())
    }

    /// ExternalEntityParserCreate(context, encoding=None): a parser for an
    /// external entity referenced from this document.
    #[pyo3(name = "ExternalEntityParserCreate", signature = (context, encoding = None))]
    fn external_entity_parser_create(
        slf: &Bound<'_, Self>,
        context: Option<&str>,
        encoding: Option<&str>,
    ) -> PyResult<Py<XmlParser>> {
        let py = slf.py();
        let this = slf.borrow();
        let context = context.map(CString::new).transpose().map_err(|e| PyValueError::new_err(e.to_string()))?;
        let encoding = encoding.map(CString::new).transpose().map_err(|e| PyValueError::new_err(e.to_string()))?;
        // SAFETY: the parser is live; the strings outlive the call.
        let child = unsafe {
            XML_ExternalEntityParserCreate(
                this.itself.get(),
                context.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                encoding.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            )
        };
        let intern = this.intern.as_ref().map(|d| d.clone_ref(py));
        let obj = make_parser(py, child, intern, Some(slf.clone().unbind()))?;
        {
            let c = obj.borrow(py);
            c.buffer_size.set(this.buffer_size.get());
            if this.buffer.borrow().is_some() {
                *c.buffer.borrow_mut() = Some(String::new());
            }
            c.ordered_attributes.set(this.ordered_attributes.get());
            c.reparse_deferral.set(this.reparse_deferral.get());
            c.specified_attributes.set(this.specified_attributes.get());
            // The child starts with the parent's handlers.
            for (i, h) in this.handlers.borrow().iter().enumerate() {
                if let Some(h) = h {
                    c.handlers.borrow_mut()[i] = Some(h.clone_ref(py));
                    c.set_c_handler(HANDLERS[i].1, true);
                }
            }
        }
        Ok(obj)
    }

    #[getter(buffer_text)]
    fn get_buffer_text(&self) -> bool {
        self.buffer.borrow().is_some()
    }

    #[getter(buffer_size)]
    fn get_buffer_size(&self) -> usize {
        self.buffer_size.get()
    }

    #[getter(buffer_used)]
    fn get_buffer_used(&self) -> usize {
        self.buffer.borrow().as_ref().map_or(0, |b| b.len())
    }

    #[getter(namespace_prefixes)]
    fn get_namespace_prefixes(&self) -> bool {
        self.namespace_prefixes.get()
    }

    #[getter(ordered_attributes)]
    fn get_ordered_attributes(&self) -> bool {
        self.ordered_attributes.get()
    }

    #[getter(specified_attributes)]
    fn get_specified_attributes(&self) -> bool {
        self.specified_attributes.get()
    }

    #[getter(intern)]
    fn get_intern(&self, py: Python<'_>) -> Py<PyAny> {
        self.intern.as_ref().map_or_else(|| py.None(), |d| d.clone_ref(py))
    }

    #[getter(ErrorCode)]
    fn error_code(&self) -> c_int {
        // SAFETY: the parser is live.
        unsafe { XML_GetErrorCode(self.itself.get()) }
    }

    #[getter(ErrorLineNumber)]
    fn error_line_number(&self) -> u64 {
        // SAFETY: the parser is live.
        unsafe { XML_GetCurrentLineNumber(self.itself.get()) as u64 }
    }

    #[getter(ErrorColumnNumber)]
    fn error_column_number(&self) -> u64 {
        // SAFETY: the parser is live.
        unsafe { XML_GetCurrentColumnNumber(self.itself.get()) as u64 }
    }

    #[getter(ErrorByteIndex)]
    fn error_byte_index(&self) -> i64 {
        // SAFETY: the parser is live.
        unsafe { XML_GetCurrentByteIndex(self.itself.get()) as i64 }
    }

    #[getter(CurrentLineNumber)]
    fn current_line_number(&self) -> u64 {
        self.error_line_number()
    }

    #[getter(CurrentColumnNumber)]
    fn current_column_number(&self) -> u64 {
        self.error_column_number()
    }

    #[getter(CurrentByteIndex)]
    fn current_byte_index(&self) -> i64 {
        self.error_byte_index()
    }

    /// Handler attributes, which aren't descriptors.
    fn __getattr__(&self, py: Python<'_>, name: &Bound<'_, PyString>) -> PyResult<Py<PyAny>> {
        let name = name.to_string_lossy();
        let name = name.as_ref();
        match handler_index(name) {
            Some(h) => Ok(self.handler(py, h).map_or_else(|| py.None(), |h| h.obj)),
            None => Err(PyAttributeError::new_err(format!("'xmlparser' object has no attribute '{name}'"))),
        }
    }

    fn __setattr__(&self, py: Python<'_>, name: &Bound<'_, PyString>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let name = name.to_string_lossy();
        let name = name.as_ref();
        if let Some(h) = handler_index(name) {
            let on = !value.is_none();
            // The character buffer belongs to the old handler.
            if h == H::CharacterData {
                self.flush(py)?;
            }
            self.handlers.borrow_mut()[h as usize] = on.then(|| value.clone().unbind());
            self.set_c_handler(h, on);
            return Ok(());
        }
        match name {
            "buffer_text" => {
                let on = value.is_truthy()?;
                if on != self.buffer.borrow().is_some() {
                    if !on {
                        self.flush(py)?;
                    }
                    *self.buffer.borrow_mut() = on.then(String::new);
                }
            }
            "buffer_size" => {
                if !value.is_instance_of::<pyo3::types::PyInt>() {
                    return Err(PyTypeError::new_err(format!(
                        "'{}' object cannot be interpreted as an integer",
                        value.get_type().name()?
                    )));
                }
                let size: i64 = value.extract()?;
                if size > i32::MAX as i64 {
                    return Err(PyValueError::new_err(format!("buffer_size must not be greater than {}", i32::MAX)));
                }
                if size <= 0 {
                    return Err(PyValueError::new_err("buffer_size must be greater than zero"));
                }
                self.flush(py)?;
                self.buffer_size.set(size as usize);
            }
            "namespace_prefixes" => {
                let on = value.is_truthy()?;
                self.namespace_prefixes.set(on);
                // SAFETY: the parser is live.
                unsafe { XML_SetReturnNSTriplet(self.itself.get(), on as c_int) };
            }
            "ordered_attributes" => self.ordered_attributes.set(value.is_truthy()?),
            "specified_attributes" => self.specified_attributes.set(value.is_truthy()?),
            _ => {
                return Err(PyAttributeError::new_err(format!("'xmlparser' object has no attribute '{name}'")));
            }
        }
        Ok(())
    }

    fn __dir__(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        let mut names: Vec<&str> = HANDLERS.iter().map(|(n, _)| *n).collect();
        names.extend([
            "ErrorCode", "ErrorLineNumber", "ErrorColumnNumber", "ErrorByteIndex", "CurrentLineNumber",
            "CurrentColumnNumber", "CurrentByteIndex", "buffer_size", "buffer_text", "buffer_used",
            "namespace_prefixes", "ordered_attributes", "specified_attributes", "intern", "Parse", "ParseFile",
            "SetBase", "GetBase", "GetInputContext", "ExternalEntityParserCreate", "SetParamEntityParsing",
            "UseForeignDTD", "GetReparseDeferralEnabled", "SetReparseDeferralEnabled",
        ]);
        Ok(PyList::new(py, names)?.unbind())
    }
}

/// ParserCreate(encoding=None, namespace_separator=None, intern=...)
#[pyfunction]
#[pyo3(name = "ParserCreate", signature = (encoding = None, namespace_separator = None, *args, **kwargs))]
fn parser_create(
    py: Python<'_>,
    encoding: Option<&Bound<'_, PyAny>>,
    namespace_separator: Option<&Bound<'_, PyAny>>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<XmlParser>> {
    let string_arg = |v: Option<&Bound<'_, PyAny>>, what: &str| -> PyResult<Option<String>> {
        match v {
            None => Ok(None),
            Some(v) if v.is_none() => Ok(None),
            Some(v) => match v.cast::<PyString>() {
                Ok(s) => Ok(Some(s.to_str()?.to_owned())),
                Err(_) => Err(PyTypeError::new_err(format!(
                    "ParserCreate() argument '{what}' must be str or None, not {}",
                    v.get_type().name()?
                ))),
            },
        }
    };
    let encoding = string_arg(encoding, "encoding")?;
    let separator = string_arg(namespace_separator, "namespace_separator")?;
    if separator.as_ref().is_some_and(|s| s.chars().count() > 1) {
        return Err(PyValueError::new_err("namespace_separator must be at most one character, omitted, or None"));
    }
    // intern: omitted -> a new dict; None -> no interning; else that dict.
    let intern_arg = match (args.len(), kwargs.and_then(|k| k.get_item("intern").ok().flatten())) {
        (0, None) => None,
        (1, None) => Some(args.get_item(0)?),
        (0, Some(v)) => Some(v),
        _ => return Err(PyTypeError::new_err("ParserCreate() takes at most 3 arguments")),
    };
    if let Some(k) = kwargs {
        for key in k.keys() {
            let key: String = key.extract()?;
            if key != "intern" {
                return Err(PyTypeError::new_err(format!("ParserCreate() got an unexpected keyword argument '{key}'")));
            }
        }
    }
    let intern = match intern_arg {
        None => Some(PyDict::new(py).into_any().unbind()),
        Some(v) if v.is_none() => None,
        Some(v) if v.is_instance_of::<PyDict>() => Some(v.unbind()),
        Some(_) => return Err(PyTypeError::new_err("intern must be a dictionary")),
    };
    let encoding = encoding.map(CString::new).transpose().map_err(|e| PyValueError::new_err(e.to_string()))?;
    let separator = separator.map(CString::new).transpose().map_err(|e| PyValueError::new_err(e.to_string()))?;
    // SAFETY: the strings outlive the call; the parser copies what it keeps.
    let itself = unsafe {
        XML_ParserCreate_MM(
            encoding.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
            ptr::null(),
            separator.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
        )
    };
    make_parser(py, itself, intern, None)
}

/// ErrorString(code): the message for an error code, or None.
/// `_expat_capi(real)`: the `expat_CAPI` capsule for this module, with the
/// magic and version numbers of `real`, the interpreter's own `pyexpat`'s
/// capsule (or None). Used by `expat_rs.install()`.
#[pyfunction]
#[pyo3(name = "_expat_capi")]
fn expat_capi<'py>(py: Python<'py>, real: Option<Bound<'py, PyCapsule>>) -> PyResult<Bound<'py, PyCapsule>> {
    capi::make_capsule(py, real.as_ref())
}

#[pyfunction]
#[pyo3(name = "ErrorString")]
fn error_string(code: c_int) -> Option<String> {
    text(XML_ErrorString(code))
}

#[pymodule(gil_used = true)]
fn _pyexpat(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add_class::<XmlParser>()?;
    m.add("XMLParserType", py.get_type::<XmlParser>())?;
    m.add_function(wrap_pyfunction!(parser_create, m)?)?;
    m.add_function(wrap_pyfunction!(error_string, m)?)?;
    m.add_function(wrap_pyfunction!(expat_capi, m)?)?;
    m.add("ExpatError", py.get_type::<ExpatError>())?;
    m.add("error", py.get_type::<ExpatError>())?;
    m.add("EXPAT_VERSION", text(XML_ExpatVersion()).unwrap_or_default())?;
    let v = XML_ExpatVersionInfo();
    m.add("version_info", (v.major, v.minor, v.micro))?;
    m.add("native_encoding", "UTF-8")?;
    m.add("XML_PARAM_ENTITY_PARSING_NEVER", 0)?;
    m.add("XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE", 1)?;
    m.add("XML_PARAM_ENTITY_PARSING_ALWAYS", 2)?;

    let mut features = Vec::new();
    // The list is static and ends with XML_FEATURE_END.
    let mut f = XML_GetFeatureList();
    while !f.is_null() && unsafe { (*f).feature } != XML_FEATURE_END {
        // SAFETY: entries before the end marker are valid.
        let (name, value) = unsafe { (text((*f).name).unwrap_or_default(), (*f).value) };
        features.push((name, value));
        f = unsafe { f.add(1) };
    }
    m.add("features", features)?;

    // errors: XML_ERROR_* -> message, plus codes and messages dictionaries.
    let errors = PyModule::new(py, "pyexpat.errors")?;
    let codes = PyDict::new(py);
    let messages = PyDict::new(py);
    for (name, code) in ERROR_CODES {
        if let Some(msg) = error_string(*code) {
            errors.add(*name, &msg)?;
            codes.set_item(&msg, *code)?;
            messages.set_item(*code, &msg)?;
        }
    }
    errors.add("codes", codes)?;
    errors.add("messages", messages)?;
    errors.add("__doc__", "Constants used to describe error conditions.")?;
    m.add("errors", errors)?;

    let model = PyModule::new(py, "pyexpat.model")?;
    for (name, value) in [
        ("XML_CTYPE_EMPTY", XML_CTYPE_EMPTY),
        ("XML_CTYPE_ANY", XML_CTYPE_ANY),
        ("XML_CTYPE_MIXED", XML_CTYPE_MIXED),
        ("XML_CTYPE_NAME", XML_CTYPE_NAME),
        ("XML_CTYPE_CHOICE", XML_CTYPE_CHOICE),
        ("XML_CTYPE_SEQ", XML_CTYPE_SEQ),
        ("XML_CQUANT_NONE", XML_CQUANT_NONE),
        ("XML_CQUANT_OPT", XML_CQUANT_OPT),
        ("XML_CQUANT_REP", XML_CQUANT_REP),
        ("XML_CQUANT_PLUS", XML_CQUANT_PLUS),
    ] {
        model.add(name, value)?;
    }
    model.add("__doc__", "Constants used to interpret content model information.")?;
    m.add("model", model)?;
    Ok(())
}

/// Error names and codes, as `pyexpat.errors` lists them.
const ERROR_CODES: &[(&str, c_int)] = &[
    ("XML_ERROR_NO_MEMORY", XML_ERROR_NO_MEMORY),
    ("XML_ERROR_SYNTAX", XML_ERROR_SYNTAX),
    ("XML_ERROR_NO_ELEMENTS", XML_ERROR_NO_ELEMENTS),
    ("XML_ERROR_INVALID_TOKEN", XML_ERROR_INVALID_TOKEN),
    ("XML_ERROR_UNCLOSED_TOKEN", XML_ERROR_UNCLOSED_TOKEN),
    ("XML_ERROR_PARTIAL_CHAR", XML_ERROR_PARTIAL_CHAR),
    ("XML_ERROR_TAG_MISMATCH", XML_ERROR_TAG_MISMATCH),
    ("XML_ERROR_DUPLICATE_ATTRIBUTE", XML_ERROR_DUPLICATE_ATTRIBUTE),
    ("XML_ERROR_JUNK_AFTER_DOC_ELEMENT", XML_ERROR_JUNK_AFTER_DOC_ELEMENT),
    ("XML_ERROR_PARAM_ENTITY_REF", XML_ERROR_PARAM_ENTITY_REF),
    ("XML_ERROR_UNDEFINED_ENTITY", XML_ERROR_UNDEFINED_ENTITY),
    ("XML_ERROR_RECURSIVE_ENTITY_REF", XML_ERROR_RECURSIVE_ENTITY_REF),
    ("XML_ERROR_ASYNC_ENTITY", XML_ERROR_ASYNC_ENTITY),
    ("XML_ERROR_BAD_CHAR_REF", XML_ERROR_BAD_CHAR_REF),
    ("XML_ERROR_BINARY_ENTITY_REF", XML_ERROR_BINARY_ENTITY_REF),
    ("XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF", XML_ERROR_ATTRIBUTE_EXTERNAL_ENTITY_REF),
    ("XML_ERROR_MISPLACED_XML_PI", XML_ERROR_MISPLACED_XML_PI),
    ("XML_ERROR_UNKNOWN_ENCODING", XML_ERROR_UNKNOWN_ENCODING),
    ("XML_ERROR_INCORRECT_ENCODING", XML_ERROR_INCORRECT_ENCODING),
    ("XML_ERROR_UNCLOSED_CDATA_SECTION", XML_ERROR_UNCLOSED_CDATA_SECTION),
    ("XML_ERROR_EXTERNAL_ENTITY_HANDLING", XML_ERROR_EXTERNAL_ENTITY_HANDLING),
    ("XML_ERROR_NOT_STANDALONE", XML_ERROR_NOT_STANDALONE),
    ("XML_ERROR_UNEXPECTED_STATE", XML_ERROR_UNEXPECTED_STATE),
    ("XML_ERROR_ENTITY_DECLARED_IN_PE", XML_ERROR_ENTITY_DECLARED_IN_PE),
    ("XML_ERROR_FEATURE_REQUIRES_XML_DTD", XML_ERROR_FEATURE_REQUIRES_XML_DTD),
    ("XML_ERROR_CANT_CHANGE_FEATURE_ONCE_PARSING", XML_ERROR_CANT_CHANGE_FEATURE_ONCE_PARSING),
    ("XML_ERROR_UNBOUND_PREFIX", XML_ERROR_UNBOUND_PREFIX),
    ("XML_ERROR_UNDECLARING_PREFIX", XML_ERROR_UNDECLARING_PREFIX),
    ("XML_ERROR_INCOMPLETE_PE", XML_ERROR_INCOMPLETE_PE),
    ("XML_ERROR_XML_DECL", XML_ERROR_XML_DECL),
    ("XML_ERROR_TEXT_DECL", XML_ERROR_TEXT_DECL),
    ("XML_ERROR_PUBLICID", XML_ERROR_PUBLICID),
    ("XML_ERROR_SUSPENDED", XML_ERROR_SUSPENDED),
    ("XML_ERROR_NOT_SUSPENDED", XML_ERROR_NOT_SUSPENDED),
    ("XML_ERROR_ABORTED", XML_ERROR_ABORTED),
    ("XML_ERROR_FINISHED", XML_ERROR_FINISHED),
    ("XML_ERROR_SUSPEND_PE", XML_ERROR_SUSPEND_PE),
    ("XML_ERROR_RESERVED_PREFIX_XML", XML_ERROR_RESERVED_PREFIX_XML),
    ("XML_ERROR_RESERVED_PREFIX_XMLNS", XML_ERROR_RESERVED_PREFIX_XMLNS),
    ("XML_ERROR_RESERVED_NAMESPACE_URI", XML_ERROR_RESERVED_NAMESPACE_URI),
    ("XML_ERROR_INVALID_ARGUMENT", XML_ERROR_INVALID_ARGUMENT),
    ("XML_ERROR_NO_BUFFER", XML_ERROR_NO_BUFFER),
    ("XML_ERROR_AMPLIFICATION_LIMIT_BREACH", XML_ERROR_AMPLIFICATION_LIMIT_BREACH),
    ("XML_ERROR_NOT_STARTED", XML_ERROR_NOT_STARTED),
];
