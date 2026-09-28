#!/usr/bin/env python3
"""Check parser output against the W3C suite's expected canonical output.

Usage: [EDITION=4|5] [VERBOSE=1] [XMLWF_ARGS=...] output.py <xmlconf-root> [<xmlwf-binary>]

For every catalogue TEST of TYPE valid or invalid (both are well-formed)
that names an OUTPUT file, runs `xmlwf --canonical --external` and compares
stdout with that file byte for byte. Tests the catalogue marks as not
applying to EDITION (default 5), and XML 1.1 / namespace-only tests, are
skipped.
"""
import os
import re
import subprocess
import sys


def tests(root):
    with open(os.path.join(root, "xmlconf.xml"), encoding="utf-8") as f:
        catalogue = f.read()
    for part in re.findall(r'<!ENTITY\s+\S+\s+SYSTEM\s+"([^"]+\.xml)"', catalogue):
        path = os.path.join(root, part)
        if not os.path.exists(path):
            continue
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read()
        base = os.path.dirname(path)
        for test in re.findall(r"<TEST\b[^>]*>", text):
            attrs = dict(re.findall(r'(\w+)="([^"]*)"', test))
            if "OUTPUT" in attrs and attrs.get("TYPE") in ("valid", "invalid"):
                yield (os.path.normpath(os.path.join(base, attrs["URI"])),
                       os.path.normpath(os.path.join(base, attrs["OUTPUT"])), attrs)


def main():
    root = sys.argv[1]
    xmlwf = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        os.path.dirname(__file__), "..", "target", "release", "xmlwf")
    edition = os.environ.get("EDITION", "5")
    verbose = os.environ.get("VERBOSE") == "1"
    passed, failed, skipped = 0, [], 0
    for doc, expected, attrs in tests(root):
        editions = attrs.get("EDITION")
        if (editions and edition not in editions.split()) \
                or attrs.get("RECOMMENDATION", "XML1.0") not in ("XML1.0", "XML1.0-errata2e") \
                or attrs.get("NAMESPACE") == "yes" and "xml-1.1" in doc:
            skipped += 1
            continue
        run = subprocess.run([xmlwf, "--edition", edition, "--external", "--canonical", *os.environ.get("XMLWF_ARGS", "").split(), doc],
                             capture_output=True)
        with open(expected, "rb") as f:
            want = f.read()
        if run.returncode == 0 and run.stdout == want:
            passed += 1
        else:
            failed.append((doc, run.stdout, want, run.stderr))
    total = passed + len(failed)
    print(f"=== W3C canonical output — expat-rs (XML 1.0 edition {edition}) ===")
    print(f"Skipped: {skipped}")
    print(f"Output matches: {passed} / {total}  ({100 * passed / max(total, 1):.1f}%)")
    if verbose:
        for doc, got, want, err in failed:
            print(f"\n{os.path.relpath(doc, root)}")
            if err:
                print(f"  error: {err.decode(errors='replace').strip()[:200]}")
            print(f"  want:  {want[:200]!r}")
            print(f"  got:   {got[:200]!r}")


if __name__ == "__main__":
    main()
