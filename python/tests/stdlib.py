"""CPython's own XML test suites, run with expat_rs.install() in effect.

    python tests/stdlib.py            # with expat-rs
    python tests/stdlib.py --libexpat # the same run on the interpreter's libexpat

Runs the interpreter's test runner in this process after install(), on the
test modules of the XML packages that this Python version has (its `test`
package must be installed). Exits non-zero if any fails.

Expected differences from a run on libexpat:
- the allocation-limit tests skip: expat-rs doesn't offer those methods,
  as a libexpat before 2.7.2 doesn't;
- 3.15's test_error_path_no_crash is left out: it injects failures into
  Python's allocator, which the parser's Rust allocations don't go through.
"""

import importlib.util
import runpy
import sys

MODULES = [
    "test_pyexpat",
    "test_xml_etree",
    "test_xml_etree_c",
    "test_sax",
    "test_minidom",
    "test_pulldom",
    "test_xml_dom_minicompat",
    "test_xml_dom_xmlbuilder",
    "test_plistlib",
    "test_xmlrpc",
    "test_docxmlrpc",
]

IGNORE = ["test_error_path_no_crash"]


def main():
    ours = "--libexpat" not in sys.argv[1:]
    if ours:
        import expat_rs

        expat_rs.install()
        import pyexpat

        assert pyexpat is expat_rs._pyexpat
    if importlib.util.find_spec("test") is None or importlib.util.find_spec("test.support") is None:
        sys.exit("this interpreter has no test package")
    present = [m for m in MODULES if importlib.util.find_spec("test." + m)]
    args = ["test", "-v"]
    if ours:
        for name in IGNORE:
            args += ["-i", name]
    sys.argv = args + present
    runpy.run_module("test", run_name="__main__", alter_sys=True)


if __name__ == "__main__":
    main()
