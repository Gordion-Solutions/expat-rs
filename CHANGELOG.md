# Changelog

## 0.3.0 — 2026-11-15

What the libexpat-compatible C library (`expat-capi`, in this repository,
not published) needs from the parser: the information libexpat's
handlers receive, libexpat's error codes and positions, and its
behaviour with streamed input. CPython's XML test suites and its whole
test suite pass on `expat-capi`, as do Android's uses of libexpat.

### Added
- `Located { event, span, raw }` and `Span`: where each event is and its
  exact source text. `Parser::next_located`, `StreamParser::feed_located`
  / `feed_str_located` / `finish_located`. `Located::input_start` and
  `StreamParser::input_offset` give positions in input bytes, as
  libexpat counts them.
- `ErrorKind` and `XmlError::kind()`: one kind per libexpat error code
  expat-rs can produce, classified as libexpat classifies them.
- Declaration events: `ElementDecl` (with a flat `ContentModel` of
  `ContentNode`s, `ContentKind`, `Quantifier`), `AttlistDecl`,
  `EntityDecl`, `ParameterEntityRef`.
- `AmplificationLimits`: libexpat's billion-laughs protection (on by
  default with libexpat's defaults), `with_amplification_limits` and
  `StreamParser::set_amplification_limits`.
- `StreamParser`:
  - `with_reparse_deferral` / `set_reparse_deferral` /
    `reparse_deferral`;
  - `with_encoding`: decode as the caller says, whatever the document
    declares;
  - `with_unknown_encoding`: other encodings through an `EncodingMap`,
    single-byte or with 2–4 byte sequences decoded by a converter
    (libexpat's `XML_Encoding`); also `StreamDecoder::decode_with`;
  - `with_external_entity_refs`: external parsed entities reported as
    `Event::ExternalEntityRef`, with an `EntityContext` whose `parser()`
    parses the entity's content;
  - `with_outside_whitespace`: whitespace outside the root element as
    `Event::Whitespace`;
  - `with_cdata_pieces`: CDATA sections as `Event::StartCData`,
    `Event::CData` pieces as the input arrives, and `Event::EndCData`.
- `with_libexpat_leniency` (on `Parser` and `StreamParser`): accept what
  libexpat accepts beyond XML 1.0: any version number made of
  `[A-Za-z0-9._-]`, and an encoding declaration after a UTF-8 byte order
  mark naming another encoding.

### Changed (breaking)
- `Event::Doctype` has `system_id`, `public_id` and
  `has_internal_subset`.
- Minimum Rust version 1.83 (was 1.94), the version AOSP builds with.

### Changed
- Errors are reported at the position libexpat reports them, and
  duplicate attributes at the second attribute.
- `StreamParser`, as libexpat: reparse deferral retries when libexpat
  does; a `]]>` in content is an error as soon as it is fed; input
  forced to UTF-16 without a byte order mark takes its byte order from
  the first character; UTF-16 without a byte order mark is detected as
  libexpat detects it. `StreamDecoder` decides between UTF-16 and 8-bit
  input from the first two bytes (it waited for four).

### Fixed
- `StreamParser` was quadratic with UTF-16 fed in very small pieces
  (400,000 characters in 2-byte pieces took 60 s; now 0.03 s).

## 0.2.0 — 2026-10-03

Well-formedness checking is close to complete, events carry the
document's real content, and the parser is hardened, fuzzed and
benchmarked against libexpat.

### Conformance (W3C XML Conformance Test Suite)
- 1441 / 1501 well-formedness tests that apply to XML 1.0 Fifth Edition
  (was 927 / 1810 in 0.1.0); every well-formed test is accepted.
- 327 / 373 expected canonical outputs matched; the remaining 46 depend
  on external DTD subsets and parameter entities.
- 48 / 48 Namespaces in XML 1.0 tests.

### Added
- `Edition`: XML 1.0 Fifth Edition name rules (default) or Fourth
  Edition (Appendix B), on `Parser` and `StreamParser`.
- DOCTYPE and internal-subset parsing: every markup declaration checked.
- Entity handling: replacement text checked where it is used, No
  Recursion, the attribute-value entity rules, Entity Declared where it
  applies, first declaration binding. External entities are read only
  through an opt-in loader (`with_external_loader`).
- `decode` / `StreamDecoder`: UTF-8, UTF-16 (BOM or detected), US-ASCII
  and ISO-8859-1, with declaration consistency checks.
- Events with real content: line endings normalised, character and
  entity references expanded (entity markup delivered as events),
  attribute values normalised per §3.3.3, DTD default attributes
  (`Attribute::specified`), `NotationDecl`, `EndDoctype`, DTD processing
  instructions and comments, `SkippedEntity`.
- Namespace processing (`with_namespaces`): resolved element and
  attribute namespaces, `StartNamespace` / `EndNamespace`, all
  Namespaces 1.0 constraints.
- `StreamParser`: incremental parsing (`feed` / `feed_str` / `finish`),
  same results at any chunk size, with reparse deferral.
- `xmlwf`: `--edition`, `--external`, `--namespaces`, `--canonical`,
  `--chunk`.

### Security and robustness
- Linear time on all inputs: fixed a stack overflow on deeply nested
  content models and quadratic behaviour with many attributes, deep
  namespace nesting, and huge constructs streamed in small chunks (the
  class of libexpat CVE-2023-52425).
- cargo-fuzz targets (`fuzz/`): no-panic and stream-vs-whole
  differential.
- Bounded entity expansion (unchanged defaults: depth 20, 1 MiB).

### Changed (breaking)
- `Event` strings are `Cow<'a, str>`; `StartElement` has `namespace`
  and `Vec<Attribute>`.
- Modules are private; use the crate-root re-exports. `EntityTable` is
  no longer public.
- Errors report the real line and column (they were always 1:1 in
  parser-level errors); columns count characters.

### Performance
- 0.56–0.79x libexpat 2.8.2's throughput on 25–56 MB documents
  (`bench/`).

### Not yet supported
- Reading the external DTD subset and expanding parameter entities, DTD
  validation, XML 1.1, a libexpat-compatible C API.

## 0.1.0 — 2026-05-15

Initial release: tokeniser, well-formedness checker, bounded entity
expansion, conformance runner.
