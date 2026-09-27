# expat-rs

[![CI](https://github.com/Gordion-Solutions/expat-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/Gordion-Solutions/expat-rs/actions/workflows/ci.yml)

A Rust XML 1.0 parser. Follows the Fifth Edition by default, with an
option for Fourth Edition name rules (`Edition::Fourth`) to match parsers
built on the older spec, such as libexpat. Aims for the same conformance
as [libexpat](https://github.com/libexpat/libexpat) without C's
memory-safety bugs.

> **Status:** early development. Tokeniser, well-formedness checker,
> §2.2 character validation, and entity expansion with billion-laughs /
> quadratic-blowup defences. On the W3C XML conformance suite:
> **972 / 1501 (64.8%)** of the tests that apply to the Fifth Edition,
> **979 / 1810 (54.1%)** under Fourth Edition rules. The gap is
> unimplemented features (DTDs, namespaces, encoding detection). See
> `conformance/STATUS.md`.

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

## Build & test

```sh
cargo build --release
cargo test --release
```

## Project layout

```
.
├── METHODOLOGY.md             # clean-room declaration
├── src/
│   ├── lib.rs                 # public API
│   ├── token.rs               # Token enum (every W3C production cited)
│   ├── lexer.rs               # tokeniser
│   ├── chars.rs               # §2.2 Char, Name rules per Edition, char refs
│   ├── edition4.rs            # 4th ed. Appendix B tables (generated)
│   ├── event.rs               # high-level Event enum (parser output)
│   ├── parser.rs              # well-formedness checker
│   ├── entities.rs            # DTD entity decls + bounded expansion
│   ├── error.rs               # XmlError + Position
│   └── bin/
│       └── xmlwf.rs           # CLI well-formedness checker (--edition 4|5)
├── tests/
│   ├── tokeniser_tests.rs       # 21 tests, one per spec production
│   ├── well_formedness_tests.rs # 29 tests: §2.1 constraints, §2.2 Char, …
│   ├── entity_security_tests.rs # 16 tests: billion-laughs, quadratic-blowup, …
│   └── edition_tests.rs         # 5 tests: Fifth vs. Fourth Edition names
├── tools/
│   └── gen_edition4_tables.py # generates src/edition4.rs from the W3C spec
└── conformance/
    ├── README.md              # how to run the W3C XML test suite
    ├── runner.sh              # iterates the suite, reports pass/fail
    ├── editions.py            # which tests apply to which edition
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
- [x] Edition-aware W3C XML conformance runner — 972 / 1501 (Fifth),
      979 / 1810 (Fourth); gap is unimplemented features
- [ ] Encoding detection (UTF-16 BOM, declared encodings) — target +5-7%
- [ ] Namespaces (W3C XML Namespaces 1.0) — target +15%
- [ ] DTDs and validity constraints — target +25-30%
- [ ] Full W3C conformance — match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — drop-in replacement
