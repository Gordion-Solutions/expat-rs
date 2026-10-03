# Conformance Status

Numbers updated as the parser progresses.

## Latest run

Tests apply per XML 1.0 edition (see `README.md`). Fifth Edition is the
default; Fourth Edition runs the whole suite.

| Category | 5th ed. | 4th ed. |
|---|---:|---:|
| Tests skipped (not applicable to edition) | 309 | 0 |
| Well-formed inputs accepted | **567** / 567 (100%) | **567** / 567 (100%) |
| Not-well-formed inputs rejected | **874** / 934 (93.6%) | **1183** / 1243 (95.2%) |
| **Total** | **1441 / 1501 (96.0%)** | **1750 / 1810 (96.7%)** |
| _libexpat reference_ | | _1801 / 1809 (99.6%)_ |

History (Fifth / Fourth):

| Change | 5th ed. | 4th ed. |
|---|---:|---:|
| v0.1 (no edition filtering) | 927 / 1810 | 927 / 1810 |
| §2.2 Char, char refs, edition option | 972 / 1501 | 979 / 1810 |
| DOCTYPE + internal subset tokeniser | 1329 / 1501 | 1636 / 1810 |
| Strict XML declaration, AttValue references | 1381 / 1501 | 1688 / 1810 |
| Entity semantics, external entity loading | 1436 / 1501 | 1745 / 1810 |
| Encoding detection (UTF-16, ASCII, Latin-1) | 1441 / 1501 | 1750 / 1810 |

## Event output (canonical XML)

`output.py` runs `xmlwf --canonical` on every well-formed test that has
an expected-output file in the catalogue and compares byte for byte, so
it checks *what the parser reports*, not just accept/reject:

| | 5th ed. | 4th ed. |
|---|---:|---:|
| Output matches | **327 / 373 (87.7%)** | **327 / 373 (87.7%)** |

History: 251 (text, line endings, char refs) → 281 (entity content) →
327 (attribute normalisation, DTD defaults, notations, DTD PIs). All 46
remaining mismatches depend on declarations in external DTD subsets or
parameter entities, which are not read yet.

## Namespaces

`namespaces.py` runs the W3C Namespaces in XML 1.0 tests
(`eduni/namespaces/1.0` and errata) with `xmlwf --namespaces`:

| | Result |
|---|---:|
| Namespaces 1.0 | **48 / 48** (3 `TYPE="error"` tests skipped) |

## Incremental parsing

All three runners take `XMLWF_ARGS`; with `XMLWF_ARGS="--chunk 1"` (one
byte at a time) and `"--chunk 7"`, `StreamParser` gives exactly the
results above. Across all 2638 suite files at chunk sizes 1, 2, 5 and
13, accept/reject and canonical output are identical to whole-document
parsing, and so are error messages and positions except for one
odd-length UTF-16 file (a streaming parser can't know the length until
the end, so it reports the malformed text first).

## Hardening

**Resource limits.** Every input costs time linear in its size. Measured
and fixed (each covered by `tests/resource_tests.rs`):

| Input | Before | After |
|---|---|---|
| 100k nested groups in a DTD content model | stack overflow | 0.36 s |
| 40k attributes on one element | 3.0 s (quadratic) | 0.02 s |
| 20k namespaced attributes | 2.5 s | 0.02 s |
| 20k declared + used attributes | 2.2 s | 0.03 s |
| 50k nested elements with namespace declarations | 7.1 s | 0.13 s |
| 4 MB comment streamed in 1 KB chunks (CVE-2023-52425 class) | 19.5 s | 0.03 s |

Entity expansion is bounded by `ExpansionLimits`; external entities are
never read without a loader.

**Fuzzing** (`fuzz/`, cargo-fuzz): `parse` (any bytes, any options: must
not panic) and `stream_vs_whole` (differential: streaming in fuzzer-chosen
chunks must equal whole-document parsing). The differential target found
one bug (two errors in one text run reported differently by the two
modes; fixed so the earliest error wins). Since then both targets have
run clean: no panics, timeouts, OOMs or disagreements.

**Performance** (`bench/`): best-of-5 against libexpat 2.8.2's `xmlwf`
on generated 25–56 MB documents, Intel i5-7600K:

| Document | libexpat | expat-rs | Ratio |
|---|---:|---:|---:|
| records (attribute-heavy) | 95 MB/s | 75 MB/s | 0.79x |
| articles (text-heavy) | 396 MB/s | 244 MB/s | 0.62x |
| feed (namespaces) | 140 MB/s | 92 MB/s | 0.66x |
| unicode (non-ASCII) | 192 MB/s | 107 MB/s | 0.56x |

`StreamParser` in 64 KB chunks runs at the same speed as `Parser`.

## Notes

The total is the progression metric: it climbs as features land. It is
not a correctness claim, and passing the well-formedness suite is not the
same as being a drop-in libexpat replacement (no C ABI yet).

Run with: `./runner.sh <path>/xmlconf` (after downloading and unzipping
`xmlts20130923.zip` from W3C); `EDITION=4` for the Fourth Edition. The
runner calls `xmlwf --external`, so external parsed entities beside each
test are read; the library itself never reads external entities unless
given a loader (`Parser::with_external_loader`).

## What's still failing (Fifth Edition)

All 60 remaining failures are not-well-formed inputs we accept. From a
manual review of the failure list (`VERBOSE=1`):

| Category | Approx. tests | Feature that would cover it |
|---|---:|---|
| External DTD subset: conditional sections (`<![INCLUDE[`/`<![IGNORE[`), text declarations and declarations in `.dtd` files, parameter entities inside declarations | ~55 | Reading the external subset and external parameter entities; PE expansion |
| Other edge cases | ~5 | Iterative |

Namespaces and DTD validity are not measured by this runner (it runs the
well-formedness categories); both are still unimplemented.

## Roadmap

- [x] Tokeniser
- [x] Well-formedness checker
- [x] Full Unicode `NameStartChar` / `NameChar` per §2.3
- [x] Defensive entity expansion (billion-laughs / quadratic-blowup mitigation)
- [x] §2.2 Char enforcement and validated character references
- [x] Edition selection: Fifth (default) or Fourth Edition name rules
- [x] Edition-aware conformance runner
- [x] DOCTYPE and internal-subset syntax (all markup declarations)
- [x] Strict XML declaration; references checked in attribute values
- [ ] Namespaces (W3C XML Namespaces 1.0)
- [x] Entity expansion checks: replacement-text well-formedness, No Recursion, attribute rules
- [x] External parsed entities via an opt-in loader
- [x] Encoding detection: UTF-8, UTF-16, US-ASCII, ISO-8859-1
- [x] Accurate events: text with line endings normalised and references replaced, entity content, normalised attribute values, DTD defaults, notations, DTD PIs and comments
- [x] Canonical-output checker (`output.py`)
- [x] Namespaces in XML 1.0 (opt-in) — 48 / 48 W3C namespace tests
- [x] Incremental (chunked) input: `StreamParser`, identical results at any chunk size
- [x] Hardening: linear-time on all inputs, fuzzing, benchmarks vs libexpat
- [ ] External DTD subset, parameter-entity expansion, conditional sections
- [ ] Validity constraints — target match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — Python `pyexpat` works unmodified
- [ ] Per-test categorisation in the runner (parse `xmlconf.xml` to make the in-scope number exact rather than approximate)
