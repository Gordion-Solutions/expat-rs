# Conformance Status

Numbers updated as the parser progresses.

## Latest run

Tests apply per XML 1.0 edition (see `README.md`). Fifth Edition is the
default; Fourth Edition runs the whole suite.

| Category | 5th ed. | 4th ed. |
|---|---:|---:|
| Tests skipped (not applicable to edition) | 309 | 0 |
| Well-formed inputs accepted | **538** / 567 (94.9%) | **538** / 567 (94.9%) |
| Not-well-formed inputs rejected | **843** / 934 (90.3%) | **1150** / 1243 (92.5%) |
| **Total** | **1381 / 1501 (92.0%)** | **1688 / 1810 (93.3%)** |
| _libexpat reference_ | | _1801 / 1809 (99.6%)_ |

History (Fifth / Fourth):

| Change | 5th ed. | 4th ed. |
|---|---:|---:|
| v0.1 (no edition filtering) | 927 / 1810 | 927 / 1810 |
| §2.2 Char, char refs, edition option | 972 / 1501 | 979 / 1810 |
| DOCTYPE + internal subset tokeniser | 1329 / 1501 | 1636 / 1810 |
| Strict XML declaration, AttValue references | 1381 / 1501 | 1688 / 1810 |

The total is the progression metric: it climbs as features land. It is
not a correctness claim, and passing the well-formedness suite is not the
same as being a drop-in libexpat replacement (no C ABI yet).

Run with: `./runner.sh <path>/xmlconf` (after downloading and unzipping
`xmlts20130923.zip` from W3C); `EDITION=4` for the Fourth Edition.

## What's still failing (Fifth Edition)

From a manual review of the failure list (`VERBOSE=1`):

| Category | Approx. tests | Feature that would cover it |
|---|---:|---|
| External DTD subset and external entities: conditional sections, text declarations, declarations in `.dtd`/`.ent` files | ~55 | Loading and parsing external entities |
| Entity semantics: undeclared or recursive entities, `<` or bad references arriving via replacement text, references to external/unparsed entities in attributes | ~30 | Real entity expansion with replacement-text re-parsing |
| Valid documents rejected: external entities we don't load, UTF-16 input | ~29 | External entities; encoding detection |
| Other edge cases | ~6 | Iterative |

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
- [ ] Encoding detection (UTF-16 BOM, declared encodings)
- [ ] Namespaces (W3C XML Namespaces 1.0)
- [ ] External DTD subset and external entities (incl. conditional sections)
- [ ] Entity expansion with replacement-text well-formedness
- [ ] Validity constraints — target match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — Python `pyexpat` works unmodified
- [ ] Per-test categorisation in the runner (parse `xmlconf.xml` to make the in-scope number exact rather than approximate)
