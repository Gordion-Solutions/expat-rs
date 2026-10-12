# Target 2, phase 1: confirming the C ABI surface

Results are recorded in the Target 2 plan ("Confirmed in phase 1", Positions,
Entity limits). These tools reproduce them:

- `surface.py <expat.h>` — the libexpat functions CPython 3.13 / 3.14 / main
  link against (54 for a 2.7.x header, the same on all three).
- `probe.c` — logs libexpat callbacks with line, column and byte index, error
  codes and positions, the specified-attribute count, reparse-deferral timing
  and default-handler output, for comparison with expat-rs. Build:
  `cc -o probe probe.c -I$(brew --prefix expat)/include -L$(brew --prefix expat)/lib -lexpat`

- `errcodes.c`, `error_corpus.py`, `gen_error_tests.py` — libexpat's error
  code, line and column for each malformed input in the corpus;
  `gen_error_tests.py <errcodes built against libexpat>` regenerates
  `tests/error_kind_tests.rs`, which checks expat-rs reports the matching
  ErrorKind at the same place (whole-document and streamed).

Findings (libexpat 2.8.2, 4 Oct 2026): columns count characters (0-based), not
bytes; text is split at references exactly as expat-rs splits it; reparse
deferral on/off timing matches expat-rs plus an off switch. Error positions
(12 Oct): libexpat reports the character that makes a token invalid, the start
of a construct cut off by the end of input, the name of a mismatched end tag,
and the encoding name for encoding errors; expat-rs now does the same on all
corpus cases.
