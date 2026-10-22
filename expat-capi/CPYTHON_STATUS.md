# CPython on expat-capi

CPython can use expat-capi in place of libexpat for `pyexpat` and
everything built on it (`xml.etree.ElementTree`, `xml.sax`, `xml.dom.minidom`,
`plistlib`, `xmlrpc`). This file records how that was checked and what is
known to differ.

## Result

CPython 3.14 (branch head `8e382c3`, 21 Oct 2026; debug build, macOS 13.7
x86_64, Apple clang 14) built with `--with-system-expat` against expat-capi:

| Run | Result |
| --- | --- |
| The 11 XML test files on expat-capi | 1088 tests pass, 36 skipped |
| The same tests on libexpat 2.8.2 (same build) | 1088 pass, the same 36 skipped |
| CPython's whole test suite on expat-capi | 47,976 tests pass (529 files run; the rest skipped or needing resources not enabled) |

The XML test files are `test_pyexpat`, `test_xml_etree`, `test_xml_etree_c`,
`test_sax`, `test_minidom`, `test_pulldom`, `test_xml_dom_minicompat`,
`test_xml_dom_xmlbuilder`, `test_plistlib`, `test_xmlrpc` and
`test_docxmlrpc`. Twelve of the 36 skips are tests of libexpat 2.7.2's
allocation tracker, which CPython only compiles in against a 2.7.2 or later
header; expat-capi ships 2.7.1's header (it accepts the 2.7.2 settings, with
no effect).

## Reproducing

```sh
expat-capi/tools/cpython.sh ~/build            # build 3.14, run the XML tests on both libraries
expat-capi/tools/cpython.sh ~/build 3.14 --full # and the whole suite on expat-capi
```

The script stages expat-capi under libexpat's file names (`tools/stage.sh`),
configures CPython with

```sh
./configure --with-system-expat --with-pydebug \
  LIBEXPAT_CFLAGS=-I<expat-rs>/expat-capi/include \
  LIBEXPAT_LDFLAGS="-L<staged dir> -Wl,-rpath,<staged dir> -lexpat"
```

checks `pyexpat.EXPAT_VERSION` (expat-capi reports `expat_2.7.1`), and runs the
tests. For the libexpat comparison the same build is run with libexpat's
directory in `DYLD_LIBRARY_PATH` / `LD_LIBRARY_PATH`, which takes its place.

## How it was reached

The first run had 38 failures. Each was matched to libexpat by black-box
tests (probe programs run against libexpat 2.8.2; libexpat's source was not
used) and fixed in expat-rs or expat-capi:

- When unfinished markup is retried as input arrives (reparse deferral),
  so ElementTree's pull parser and pyexpat's 1 MiB slices see events when
  libexpat would deliver them; still linear time.
- UTF-16 without a byte order mark, UTF-16 encoding names, which
  unknown-encoding maps are accepted, text pieces of at most 1024 bytes for
  converted input.
- Error codes and positions for malformed input (102 cases checked against
  libexpat in `tests/error_kind_tests.rs`).
- The not-standalone handler, the unparsed-entity handler, the default
  handler for references it can't resolve.
- External entity sub-parsers (`XML_ExternalEntityParserCreate`) with the
  external entity handler, the external DTD and the foreign DTD calls.

## Known differences from libexpat

- External DTD subsets and external parameter entities aren't read: a
  sub-parser created for a DTD (NULL context) accepts its input without
  parsing it. XML 1.0 §5.1 allows a non-validating parser to skip them, and
  both pyexpat and `xml.sax` leave them off by default. No CPython test
  depends on them.
- Multi-byte unknown encodings (`XML_Encoding.convert`) are refused, as
  pyexpat itself refuses them.
- Suspending a parse (`XML_StopParser(parser, XML_TRUE)`, `XML_ResumeParser`)
  isn't supported; pyexpat doesn't use it.
- `XML_GetInputContext` returns NULL (pyexpat then reports no context).
- For about 1050 malformed documents in the W3C suite the error message or
  position differs (mostly DTD edge cases); the verdict is the same in every
  case.
- A UTF-8 byte order mark with an encoding declaration naming another
  encoding is a fatal error, as XML 1.0 §4.3.3 requires; libexpat accepts it.

## Not yet checked

Other CPython versions (3.13, 3.15), release (non-debug) builds, Linux, and
runs under AddressSanitizer: phase 6 of Target 2.
