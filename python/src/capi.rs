//! `pyexpat.expat_CAPI`: the table of libexpat functions that CPython's
//! `pyexpat` module exports in a capsule for `_elementtree`, the C
//! accelerator of `xml.etree.ElementTree`.
//!
//! `_elementtree` imports the capsule through `sys.modules["pyexpat"]`, so
//! once `expat_rs.install()` has put this module there it gets this table,
//! whose functions are expat-capi's, and parses with expat-rs.
//!
//! The layout is `struct PyExpat_CAPI` of CPython's `Include/pyexpat.h`,
//! which only ever grows at the end: this one has every field up to CPython
//! 3.15, and `_elementtree` accepts a table at least as large as the one it
//! was built with. It also checks the magic string and that the version
//! numbers equal those of the libexpat header it was built with, which for
//! the interpreter's `_elementtree` are those in its own `pyexpat`'s
//! capsule: they are copied from there.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

use expat::*;
use pyo3::prelude::*;
use pyo3::types::PyCapsule;

const NAME: &CStr = c"pyexpat.expat_CAPI";
const MAGIC: &CStr = c"pyexpat.expat_CAPI 1.1";

/// `struct PyExpat_CAPI`. The functions are stored untyped: the table is
/// only ever read by C.
#[repr(C)]
#[allow(dead_code)] // Read by C only.
struct ExpatCapi {
    magic: *const c_char,
    size: c_int,
    major_version: c_int,
    minor_version: c_int,
    micro_version: c_int,
    error_string: *const c_void,
    get_error_code: *const c_void,
    get_error_column_number: *const c_void,
    get_error_line_number: *const c_void,
    parse: *const c_void,
    parser_create_mm: *const c_void,
    parser_free: *const c_void,
    set_character_data_handler: *const c_void,
    set_comment_handler: *const c_void,
    set_default_handler_expand: *const c_void,
    set_element_handler: *const c_void,
    set_namespace_decl_handler: *const c_void,
    set_processing_instruction_handler: *const c_void,
    set_unknown_encoding_handler: *const c_void,
    set_user_data: *const c_void,
    set_start_doctype_decl_handler: *const c_void,
    set_encoding: *const c_void,
    default_unknown_encoding_handler: *const c_void,
    set_hash_salt: *const c_void,
    set_reparse_deferral_enabled: *const c_void,
    set_alloc_tracker_activation_threshold: *const c_void,
    set_alloc_tracker_maximum_amplification: *const c_void,
    set_billion_laughs_attack_protection_activation_threshold: *const c_void,
    set_billion_laughs_attack_protection_maximum_amplification: *const c_void,
    /// NULL, as for libexpat before 2.8.0: `_elementtree` then salts its
    /// hash tables with `set_hash_salt`, which expat-rs, having none, ignores.
    set_hash_salt_16_bytes: *const c_void,
}

/// The table is read-only once made, and its pointers are to functions and
/// to a string that live as long as the process.
#[allow(dead_code)] // Read by C, through the capsule.
struct Table(ExpatCapi);
// SAFETY: see above; nothing in it is ever written after creation.
unsafe impl Send for Table {}

/// The libexpat header fields of the interpreter's own capsule: magic and
/// version numbers.
fn interpreter_header(real: Option<&Bound<'_, PyCapsule>>) -> Option<(CString, [c_int; 3])> {
    let real = real?;
    let p = real.pointer_checked(Some(NAME)).ok()?.cast::<ExpatCapi>();
    // SAFETY: a capsule named "pyexpat.expat_CAPI" holds a struct
    // PyExpat_CAPI; the header fields read here are in every version of it,
    // and its magic is a NUL-terminated string.
    unsafe {
        let head = p.as_ref();
        if head.magic.is_null() {
            return None;
        }
        Some((CStr::from_ptr(head.magic).to_owned(), [head.major_version, head.minor_version, head.micro_version]))
    }
}

/// Make the `expat_CAPI` capsule, with the magic and version numbers of
/// `real` (the interpreter's own `pyexpat.expat_CAPI`) when given.
pub fn make_capsule<'py>(py: Python<'py>, real: Option<&Bound<'py, PyCapsule>>) -> PyResult<Bound<'py, PyCapsule>> {
    let (magic, [major, minor, micro]) = interpreter_header(real).unwrap_or_else(|| {
        let v = XML_ExpatVersionInfo();
        (MAGIC.to_owned(), [v.major, v.minor, v.micro])
    });
    // Leaked: `_elementtree` may keep the table after the capsule is gone.
    let magic: &'static CStr = Box::leak(magic.into_boxed_c_str());
    let table = ExpatCapi {
        magic: magic.as_ptr(),
        size: std::mem::size_of::<ExpatCapi>() as c_int,
        major_version: major,
        minor_version: minor,
        micro_version: micro,
        error_string: XML_ErrorString as *const c_void,
        get_error_code: XML_GetErrorCode as *const c_void,
        // libexpat's XML_GetErrorColumnNumber and XML_GetErrorLineNumber
        // are macros for these.
        get_error_column_number: XML_GetCurrentColumnNumber as *const c_void,
        get_error_line_number: XML_GetCurrentLineNumber as *const c_void,
        parse: XML_Parse as *const c_void,
        parser_create_mm: XML_ParserCreate_MM as *const c_void,
        parser_free: XML_ParserFree as *const c_void,
        set_character_data_handler: XML_SetCharacterDataHandler as *const c_void,
        set_comment_handler: XML_SetCommentHandler as *const c_void,
        set_default_handler_expand: XML_SetDefaultHandlerExpand as *const c_void,
        set_element_handler: XML_SetElementHandler as *const c_void,
        set_namespace_decl_handler: XML_SetNamespaceDeclHandler as *const c_void,
        set_processing_instruction_handler: XML_SetProcessingInstructionHandler as *const c_void,
        set_unknown_encoding_handler: XML_SetUnknownEncodingHandler as *const c_void,
        set_user_data: XML_SetUserData as *const c_void,
        set_start_doctype_decl_handler: XML_SetStartDoctypeDeclHandler as *const c_void,
        set_encoding: XML_SetEncoding as *const c_void,
        default_unknown_encoding_handler: default_unknown_encoding as *const c_void,
        set_hash_salt: XML_SetHashSalt as *const c_void,
        set_reparse_deferral_enabled: XML_SetReparseDeferralEnabled as *const c_void,
        set_alloc_tracker_activation_threshold: XML_SetAllocTrackerActivationThreshold as *const c_void,
        set_alloc_tracker_maximum_amplification: XML_SetAllocTrackerMaximumAmplification as *const c_void,
        set_billion_laughs_attack_protection_activation_threshold: XML_SetBillionLaughsAttackProtectionActivationThreshold
            as *const c_void,
        set_billion_laughs_attack_protection_maximum_amplification: XML_SetBillionLaughsAttackProtectionMaximumAmplification
            as *const c_void,
        set_hash_salt_16_bytes: ptr::null(),
    };
    PyCapsule::new_with_value(py, Table(table), NAME)
}

/// `pyexpat`'s unknown encoding handler, for `_elementtree`'s parsers: a
/// single-byte encoding Python has a codec for. Like `pyexpat`'s, it leaves
/// a failure as the pending Python exception, which `_elementtree` raises
/// when the parse fails.
unsafe extern "C" fn default_unknown_encoding(_data: *mut c_void, name: *const c_char, info: *mut XML_Encoding) -> c_int {
    // `_elementtree` calls XML_Parse with the GIL held.
    Python::attach(|py| {
        if PyErr::occurred(py) {
            return 0;
        }
        // SAFETY: the parser passes the encoding's name and its XML_Encoding.
        match unsafe { crate::fill_encoding(py, name, info) } {
            Ok(()) => 1,
            Err(e) => {
                e.restore(py);
                0
            }
        }
    })
}
