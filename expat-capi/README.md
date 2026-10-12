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

All 54 functions CPython links against, plus `XML_ParserCreate` and
`XML_ParserCreateNS`. Handlers may call back into the parser
(`XML_StopParser`, `XML_GetCurrentLineNumber`, setting handlers, ...) as
libexpat allows; calling `XML_Parse` from inside a handler fails cleanly.
See the safety notes at the top of `src/lib.rs`.

## Testing

`cargo test -p expat-capi` builds each `tests/c/*.c` probe against
`include/expat.h` and the static library and checks its output is exactly
what libexpat 2.8.2 prints (`tests/c/*.expected`, recorded by
`tools/regen_expected.sh`): callbacks and their arguments, line, column and
byte positions, error codes, messages and positions, stopping, buffers,
encodings, content models, settings.

`src/generated.rs` (constants, error strings, struct sizes) is generated
from the header and libexpat's `XML_ErrorString` by `tools/generate.py`;
both tools use libexpat only as a black box.

## Not yet matching libexpat

- Default handler: whitespace outside the root element isn't passed to it,
  and a DOCTYPE (when there is no DOCTYPE handler) arrives in one piece
  rather than token by token.
- `XML_GetCurrentByteIndex` counts bytes of the document decoded to UTF-8,
  so it differs from libexpat for documents in other encodings.
- With an unknown encoding, libexpat calls the XML-declaration handler
  before the unknown-encoding handler; expat-capi calls it after.
- Multi-byte unknown encodings (`XML_Encoding.convert`) are refused.
- `XML_ExternalEntityParserCreate` returns NULL (external entities: phase 5).
- `XML_StopParser(parser, XML_TRUE)` (suspend) isn't supported;
  `XML_ResumeParser` isn't exported.
- `XML_GetInputContext` returns NULL.
- libexpat accepts any `version` in the XML declaration; expat-rs accepts
  only `1.x`, as XML 1.0 requires.
