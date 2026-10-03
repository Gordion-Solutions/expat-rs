# Changelog

## 0.2.0 — unreleased

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
