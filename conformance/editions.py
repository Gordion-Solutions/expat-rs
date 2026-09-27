#!/usr/bin/env python3
"""List W3C suite test files that do NOT apply to a given XML 1.0 edition.

Usage: editions.py <xmlconf-root> <edition>

Reads the catalogue (xmlconf.xml and the per-contributor files it pulls in
as external entities) and prints the absolute path of every TEST whose
EDITION attribute is present and does not include <edition>. Tests with no
EDITION attribute apply to every edition. Used by runner.sh.
"""
import os
import re
import sys


def main():
    root, edition = sys.argv[1], sys.argv[2]
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
            uri = re.search(r'\bURI="([^"]+)"', test)
            ed = re.search(r'\bEDITION="([^"]+)"', test)
            if uri and ed and edition not in ed.group(1).split():
                print(os.path.normpath(os.path.join(base, uri.group(1))))


if __name__ == "__main__":
    main()
