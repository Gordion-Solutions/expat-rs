# expat-rs

[![CI](https://github.com/Gordion-Solutions/expat-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Gordion-Solutions/expat-rs/actions/workflows/ci.yml)

A Rust XML 1.0 parser. Follows the Fifth Edition by default, with an
option for Fourth Edition name rules (`Edition::Fourth`) to match parsers
built on the older spec, such as libexpat. Aims for the same conformance
as [libexpat](https://github.com/libexpat/libexpat) without C's
memory-safety bugs.

> **Status:** early development. Well-formedness checking, DOCTYPE /
> internal-subset handling, entity expansion with billion-laughs /
> quadratic-blowup defences, opt-in external entity loading, UTF-8 /
> UTF-16 / ASCII / Latin-1 decoding, and events carrying the document's
> real content (normalised text and attribute values, entity content,
> DTD defaults). On the W3C XML suite: **1441 / 1501 (96.0%)**
> well-formedness tests that apply to the Fifth Edition (**1750 / 1810**
> under Fourth Edition rules), and **327 / 373 (87.7%)** expected
> canonical outputs matched. Opt-in namespace processing passes all 48
> W3C Namespaces 1.0 tests. Input can be whole (`Parser`) or pushed in
> chunks (`StreamParser`, like libexpat's `XML_Parse`), with identical
> results. Hardened against hostile input (linear time on all inputs,
> bounded entity expansion, no external reads by default) and fuzzed;
> currently 0.56-0.79x libexpat's throughput. Not yet built: the
> external DTD subset and parameter-entity expansion, DTD validation, and
> the libexpat C ABI. See `conformance/STATUS.md`.

## What this is

A from-scratch Rust implementation of an XML 1.0 parser, designed to:

1. **Pass the W3C XML Conformance Test Suite** (the same suite libexpat passes — 1,801 of 1,809 tests).
2. **Eliminate an entire class of memory-safety vulnerabilities.** libexpat has a long history of CVEs spanning integer overflows (e.g. CVE-2024-45491, -45492, -45493), denial-of-service via resource exhaustion (e.g. CVE-2024-8176, CVE-2023-52425), external entity issues (e.g. CVE-2024-28757, CVE-2013-0340), and other logic-level vulnerabilities (e.g. CVE-2018-20843). Rust eliminates the memory-safety subset by construction; the remaining classes require careful implementation regardless of language.
3. **Provide a drop-in replacement** for libexpat's C ABI, so existing consumers (CPython's `pyexpat`, Apache HTTPD's `mod_dav`, D-Bus, fontconfig) can adopt it without recompiling.

## What this is not

- **Not a translation of libexpat.** See `METHODOLOGY.md` for the clean-room declaration. The implementation is built from the W3C XML 1.0 Fifth Edition specification (plus the Fourth Edition's Appendix B for `Edition::Fourth`), with no code derived from the libexpat source.
- **Not a fork of an existing Rust XML parser.** `quick-xml`, `xml-rs`,`roxmltree` exist and are good, but none target full libexpat-compatible conformance (DTDs, entity expansion, namespace processing, encoding detection).

## Why

libexpat parses XML in CPython's stdlib (xml.parsers.expat), Apache HTTPD's mod_dav, D-Bus, fontconfig, CMake, and many embedded systems. Bugs in libexpat translate directly to RCE in all of them.
Replacing it with a memory-safe parser closes that path.

## Usage

```toml
[dependencies]
expat-rs = "0.2"
```

```rust
use expat_rs::{Event, Parser};

fn main() -> Result<(), expat_rs::XmlError> {
    let mut parser = Parser::new("<greeting lang='en'>Hello, &amp; welcome</greeting>");
    while let Some(event) = parser.next_event()? {
        match event {
            Event::StartElement { name, attributes, .. } => println!("<{name}> {} attribute(s)", attributes.len()),
            Event::Text(text) => println!("text: {text}"),
            _ => {}
        }
    }
    Ok(())
}
```

- **Bytes:** `expat_rs::decode(&bytes)?` detects UTF-8, UTF-16, US-ASCII
  and ISO-8859-1.
- **Chunks:** `StreamParser::new()` then `feed(chunk, |event| ...)` and
  `finish(...)`, like libexpat's `XML_Parse`.
- **Namespaces:** `.with_namespaces()`.
- **External entities:** never read unless you call
  `.with_external_loader(...)`.

Full documentation with examples: [docs.rs/expat-rs](https://docs.rs/expat-rs).
The `xmlwf` binary checks files from the command line
(`xmlwf --help` lists its options).

## Build & test

```sh
cargo build --release
cargo test --release
```

Conformance: download the W3C suite and run `conformance/runner.sh`,
`conformance/output.py` and `conformance/namespaces.py` (see
`conformance/README.md`). Fuzzing: `cd fuzz && cargo +nightly fuzz run
parse` (or `stream_vs_whole`). Benchmarks: `bench/generate.py` then
`bench/compare.py`.

## Project layout

```
.
├── METHODOLOGY.md             # clean-room declaration
├── src/
│   ├── lib.rs                 # public API
│   ├── token.rs               # Token enum (every W3C production cited)
│   ├── lexer.rs               # tokeniser
│   ├── lexer/dtd.rs           # DOCTYPE + internal-subset syntax
│   ├── chars.rs               # §2.2 Char, Name rules per Edition, char refs
│   ├── edition4.rs            # 4th ed. Appendix B tables (generated)
│   ├── event.rs               # high-level Event enum (parser output)
│   ├── parser.rs              # well-formedness checker
│   ├── entities.rs            # entity declarations and table
│   ├── expand.rs              # entity expansion checks, bounded
│   ├── encoding.rs            # byte → text decoding (UTF-8/16, ASCII, Latin-1)
│   ├── namespaces.rs          # Namespaces in XML 1.0 layer (opt-in)
│   ├── stream.rs              # StreamParser: incremental (push) parsing
│   ├── error.rs               # XmlError + Position
│   └── bin/
│       └── xmlwf.rs           # CLI checker (--edition, --external, --namespaces, --canonical, --chunk)
├── tests/
│   ├── tokeniser_tests.rs       # 21 tests, one per spec production
│   ├── well_formedness_tests.rs # 32 tests: §2.1 constraints, §2.2 Char, XMLDecl, …
│   ├── entity_security_tests.rs # 16 tests: billion-laughs, quadratic-blowup, …
│   ├── edition_tests.rs         # 5 tests: Fifth vs. Fourth Edition names
│   ├── dtd_tests.rs             # 9 tests: DOCTYPE and markup declarations
│   ├── entity_semantics_tests.rs # 16 tests: replacement text, recursion, loader
│   ├── encoding_tests.rs        # 4 tests: UTF-8/16, ASCII, Latin-1
│   ├── event_tests.rs           # 17 tests: what callers receive
│   ├── namespace_tests.rs       # 8 tests: resolution, scoping, constraints
│   ├── stream_tests.rs          # 10 tests: every split point vs whole-document
│   └── resource_tests.rs        # 7 tests: inputs that were quadratic or crashed
├── fuzz/                      # cargo-fuzz targets (parse, stream_vs_whole)
├── bench/                     # benchmark documents + comparison with libexpat
├── tools/
│   └── gen_edition4_tables.py # generates src/edition4.rs from the W3C spec
└── conformance/
    ├── README.md              # how to run the W3C XML test suite
    ├── runner.sh              # iterates the suite, reports pass/fail
    ├── editions.py            # which tests apply to which edition
    ├── output.py              # compares canonical output with expected files
    ├── namespaces.py          # runs the W3C namespace tests
    └── STATUS.md              # current conformance numbers
```

## License

MIT — see `LICENSE`. Same as libexpat (so contributions can flow either way
if a consumer needs a feature in both).

## Roadmap

- [x] Tokeniser (21 tests, every W3C XML 1.0 production)
- [x] Well-formedness checker (20 tests — tag balance, root uniqueness,
      attribute uniqueness, prolog/epilog rules, built-in entities)
- [x] Full Unicode `NameStartChar` / `NameChar` per §2.3
- [x] Edition selection: Fifth (default) or Fourth Edition name rules
- [x] §2.2 Char enforcement and validated character references
- [x] Entity expansion with defensive limits — 9 security tests including
      billion-laughs and quadratic-blowup
- [x] Edition-aware W3C XML conformance runner — 1441 / 1501 (Fifth),
      1750 / 1810 (Fourth)
- [x] DOCTYPE and internal-subset syntax (all markup declarations)
- [x] Strict XML declaration; references checked in attribute values
- [x] Entity expansion checks (replacement text, No Recursion, attribute rules)
- [x] External parsed entities via an opt-in loader (off by default: no XXE)
- [x] Encoding detection: UTF-8, UTF-16, US-ASCII, ISO-8859-1
- [x] Accurate events: normalised text and attribute values, entity content,
      DTD default attributes, notations — 327 / 373 canonical outputs match
- [x] Namespaces (W3C XML Namespaces 1.0), opt-in — 48 / 48 W3C tests
- [x] Incremental (chunked) input — `StreamParser`, same results at any chunk size
- [x] Hardening — linear time on all inputs, cargo-fuzz targets, benchmarks vs libexpat
- [ ] External DTD subset, parameter-entity expansion, conditional sections
- [ ] DTD validity constraints
- [ ] Full W3C conformance — match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — drop-in replacement
