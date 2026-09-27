# Conformance Status

Numbers updated as the parser progresses.

## Latest run

Tests apply per XML 1.0 edition (see `README.md`). Fifth Edition is the
default; Fourth Edition runs the whole suite.

| Category | 5th ed. | 4th ed. |
|---|---:|---:|
| Tests skipped (not applicable to edition) | 309 | 0 |
| Well-formed inputs accepted | **536** / 567 (94.5%) | **536** / 567 (94.5%) |
| Not-well-formed inputs rejected | **436** / 934 (46.7%) | **443** / 1243 (35.6%) |
| **Total** | **972 / 1501 (64.8%)** | **979 / 1810 (54.1%)** |
| _libexpat reference_ | | _1801 / 1809 (99.6%)_ |

Previous run (v0.1, no edition filtering): 927 / 1810 (51.2%).

The Fourth Edition name-rule tests (IBM P85–P89, ~310) all place the bad
name in a processing instruction inside the DTD internal subset, which
the parser does not yet tokenise — so `Edition::Fourth` is implemented and
unit-tested but those tests only start passing once DTD declarations land.

Most remaining not-well-formed failures exercise features not built yet
(DTD declarations and validity, namespace constraints, encoding
detection, external-DTD loading) — see the breakdown below. The total is
the progression metric: it climbs as features land. It is not a
correctness claim.

> The 557 → 533 dip happened because we now reject undeclared entities.
> ~24 of the W3C tests use entities defined in external DTDs we don't
> load — they fail accordingly. We could add a permissive mode to
> accept those if needed.

Run with: `./runner.sh <path>/xmlconf` (after downloading and unzipping
`xmlts20130923.zip` from W3C); `EDITION=4` for the Fourth Edition.

## What's in the "out of scope" bucket

The 680 tests we currently miss break down roughly as:

| Category | Count (estimated) | Feature that would cover it |
|---|---:|---|
| DTD validation | ~400 | DTD declarations + validity constraints |
| Namespace constraints | ~150 | Namespace processing |
| Encoding detection | ~80 | UTF-16 BOM, declared encodings |
| Entity-related well-formedness in external subsets | ~50 | External-DTD loading |
| Edge cases | ~the rest | Iterative |

These counts come from a manual scan of failing tests and are
approximate. We'll replace them with exact numbers once the runner
categorises each test (planned).

## Roadmap

- [x] Tokeniser
- [x] Well-formedness checker
- [x] Full Unicode `NameStartChar` / `NameChar` per §2.3
- [x] Defensive entity expansion (billion-laughs / quadratic-blowup mitigation)
- [x] §2.2 Char enforcement and validated character references
- [x] Edition selection: Fifth (default) or Fourth Edition name rules
- [x] Edition-aware conformance runner
- [ ] Encoding detection (UTF-16 BOM, declared encodings)
- [ ] Namespaces (W3C XML Namespaces 1.0)
- [ ] DTD declarations
- [ ] Validity constraints — target match libexpat's 1801/1809
- [ ] `libexpat.so` ABI shim — Python `pyexpat` works unmodified
- [ ] Per-test categorisation in the runner (parse `xmlconf.xml` to make the in-scope number exact rather than approximate)
