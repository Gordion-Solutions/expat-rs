# W3C XML Conformance Suite Runner

The [W3C XML Conformance Test Suite](https://www.w3.org/XML/Test/) run
against `expat-rs`. libexpat scores 1801 / 1809 on the same suite.

## One-time setup

Download the suite from W3C and unpack it here:

```sh
curl -O https://www.w3.org/XML/Test/xmlts20020606.zip
unzip xmlts20020606.zip            # produces xmlconf/ here
```

(The suite is not vendored in this repo — it's external W3C content.)

## Running

Once an `xmlwf` binary is built (`cargo build --release --bin xmlwf`),
run the conformance pass:

```sh
./runner.sh <xmlconf-root>              # XML 1.0 Fifth Edition (default)
EDITION=4 ./runner.sh <xmlconf-root>    # Fourth Edition name rules
VERBOSE=1 ./runner.sh <xmlconf-root>    # also list every failing file
```

The catalogue (`xmlconf.xml`) tags some tests with the editions they apply
to. `editions.py` reads those tags and the runner skips tests that don't
apply to the selected edition — e.g. ~300 IBM tests of the Fourth Edition
Appendix B name rules are not errors under the Fifth Edition.

## Current status

Numbers live in `STATUS.md`.
