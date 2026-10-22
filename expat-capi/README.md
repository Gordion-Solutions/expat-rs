# expat-capi

A libexpat-compatible C library built on expat-rs: the same header
(`include/expat.h`, libexpat 2.7.1's, unchanged), the same function names
and behaviour, a memory-safe parser underneath. The target is CPython's
`pyexpat` and `_elementtree` modules (Target 2); other libexpat users may
work but aren't the goal yet.

Not published to crates.io: the ABI carries no stability promise yet.

## Building

```sh
cargo build --release -p expat-capi
expat-capi/tools/stage.sh /some/dir      # libexpat.1.dylib / libexpat.so.1
cc prog.c -Iexpat-capi/include -L/some/dir -lexpat -Wl,-rpath,/some/dir
```

`cargo build` writes `libexpat.dylib` / `libexpat.so` (and `libexpat.a`), but
programs linked against them look for libexpat's real file names, so use
`stage.sh` to copy the library under those names. Without that, macOS and
Linux quietly load the system libexpat instead: check with
`DYLD_PRINT_LIBRARIES=1` (macOS) or `ldd` (Linux).

Only the `XML_*` symbols are exported (`exported_symbols.txt`, `expat.map`).

## What's there

All 54 functions CPython links against, plus the rest of what libexpat's
own `xmlwf` uses (`XML_ParserCreate`, `XML_ParserCreateNS`,
`XML_DefaultCurrent`, `XML_GetCurrentByteCount`, `XML_GetIdAttributeIndex`,
`XML_UseParserAsHandlerArg`, `XML_SetCdataSectionHandler`,
`XML_SetDoctypeDeclHandler`, and the 2.7.2 alloc-tracker settings, which are
accepted and have no effect). Like libexpat, it applies the Fourth Edition's
name rules and accepts any version number made of `[A-Za-z0-9._-]`. Handlers may call back into the parser
(`XML_StopParser`, `XML_GetCurrentLineNumber`, setting handlers, ...) as
libexpat allows; calling `XML_Parse` from inside a handler fails cleanly.
See the safety notes at the top of `src/lib.rs`.

## CPython

CPython 3.14 builds against expat-capi (`./configure --with-system-expat
LIBEXPAT_CFLAGS=-I<expat-capi>/include LIBEXPAT_LDFLAGS="-L<staged dir>
-Wl,-rpath,<staged dir> -lexpat"`) and, on 21 Oct 2026 (macOS, debug build):

- its 11 XML test files (`test_pyexpat`, `test_xml_etree`, `test_xml_etree_c`,
  `test_sax`, `test_minidom`, `test_pulldom`, `test_xml_dom_minicompat`,
  `test_xml_dom_xmlbuilder`, `test_plistlib`, `test_xmlrpc`,
  `test_docxmlrpc`) pass: 1088 tests, the same 36 skipped as with libexpat
  2.8.2 (12 of them need a 2.7.2 or later header);
- its whole test suite passes: 47,976 tests in 529 files.

## Testing

`cargo test -p expat-capi` builds each `tests/c/*.c` probe against
`include/expat.h` and the static library and checks its output is exactly
what libexpat 2.8.2 prints (`tests/c/*.expected`, recorded by
`tools/regen_expected.sh`): callbacks and their arguments, line, column and
byte positions, error codes, messages and positions, stopping, buffers,
encodings, content models, settings.

`tools/xmlwf_suite.py` runs libexpat's own `xmlwf` (a dynamically linked
build, such as Homebrew's) over the W3C conformance suite on libexpat and on
expat-capi and compares exit status, messages and written output in five
modes. On 21 Oct 2026, 2638 files: every document libexpat accepts gives
identical output except `eduni/misc/007.xml` (below); no document libexpat
rejects is accepted; about 1050 malformed documents get a different error
message or position (see below).

`src/generated.rs` (constants, error strings, struct sizes) is generated
from the header and libexpat's `XML_ErrorString` by `tools/generate.py`;
both tools use libexpat only as a black box.

## Not yet matching libexpat

- With an unknown encoding, libexpat calls the XML-declaration handler
  before the unknown-encoding handler; expat-capi calls it after.
- Multi-byte unknown encodings (`XML_Encoding.convert`) are refused.
- External DTD subsets and external parameter entities aren't read: a
  parser from `XML_ExternalEntityParserCreate` with a NULL context accepts
  its input without parsing it (a non-validating parser may skip them,
  XML 1.0 §5.1). External parsed entities in content are fully supported.
- `XML_StopParser(parser, XML_TRUE)` (suspend) isn't supported;
  `XML_ResumeParser` isn't exported.
- `XML_GetInputContext` returns NULL.
- For about 1050 of the suite's malformed documents (mostly in DTDs) the
  error is classified or placed differently: libexpat's grammar reports
  "syntax error" where expat-rs finds an invalid token and the reverse,
  and positions follow. Same verdict in every case.
- Deliberately different: a UTF-8 byte order mark with an encoding
  declaration naming another encoding is a fatal error (XML 1.0 §4.3.3;
  the suite's `hst-lhs-007`), which libexpat accepts.
