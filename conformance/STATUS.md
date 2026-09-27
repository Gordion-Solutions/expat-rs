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
- [ ] External DTD subset, parameter-entity expansion, conditional sections
- [ ] Validity constraints — target match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — Python `pyexpat` works unmodified
- [ ] Per-test categorisation in the runner (parse `xmlconf.xml` to make the in-scope number exact rather than approximate)
