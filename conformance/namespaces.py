#!/usr/bin/env python3
"""Run the W3C Namespaces in XML 1.0 tests (eduni/namespaces).

Usage: [VERBOSE=1] [XMLWF_ARGS=...] namespaces.py <xmlconf-root> [<xmlwf-binary>]

Runs `xmlwf --namespaces --external` on each NS1.0 test: not-wf tests must
be rejected; valid and invalid tests (which are namespace-well-formed)
must be accepted. TYPE="error" tests are skipped, since a processor may
or may not report those.
"""
import os
import re
import subprocess
import sys

CATALOGUES = ["eduni/namespaces/1.0/rmt-ns10.xml", "eduni/namespaces/errata-1e/errata1e.xml"]


def main():
    root = sys.argv[1]
    xmlwf = sys.argv[2] if len(sys.argv) > 2 else os.path.join(
        os.path.dirname(__file__), "..", "target", "release", "xmlwf")
    verbose = os.environ.get("VERBOSE") == "1"
    passed, failed, skipped = 0, [], 0
    for cat in CATALOGUES:
        path = os.path.join(root, cat)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        for m in re.finditer(r"<TEST\b([^>]*)>(.*?)</TEST>", text, re.S):
            attrs = dict(re.findall(r'(\w+)="([^"]*)"', m.group(1)))
            kind = attrs.get("TYPE")
            if kind == "error":
                skipped += 1
                continue
            doc = os.path.join(os.path.dirname(path), attrs["URI"])
            run = subprocess.run([xmlwf, "--namespaces", "--external", *os.environ.get("XMLWF_ARGS", "").split(), doc], capture_output=True)
            ok = (run.returncode != 0) if kind == "not-wf" else (run.returncode == 0)
            if ok:
                passed += 1
            else:
                failed.append((os.path.relpath(doc, root), kind, " ".join(m.group(2).split()),
                               run.stderr.decode(errors="replace").strip()))
    total = passed + len(failed)
    print("=== W3C Namespaces in XML 1.0 — expat-rs ===")
    print(f"Skipped (TYPE=error): {skipped}")
    print(f"Passed: {passed} / {total}  ({100 * passed / max(total, 1):.1f}%)")
    if verbose:
        for doc, kind, desc, err in failed:
            print(f"\n{doc} [{kind}] {desc}")
            if err:
                print(f"  got: {err[:200]}")


if __name__ == "__main__":
    main()
