#!/usr/bin/env python3
"""List the libexpat functions CPython's pyexpat and _elementtree link against.

Usage: surface.py <expat.h> [branch ...]   (default branches: 3.13 3.14 main)

Fetches Modules/pyexpat.c and Modules/_elementtree.c for each CPython branch,
collects every XML_ identifier (expanding pyexpat's XML_Set##name handler table
and XML_Get##name getters), and intersects them with the functions declared in
the given expat.h. Functions behind version guards newer than 2.7.1 (alloc
tracker, XML_SetHashSalt16Bytes) are excluded, matching a 2.7.x header.
"""
import re
import subprocess
import sys

EXCLUDED = {  # compiled out for a header claiming 2.7.0 or 2.7.1
    "XML_SetAllocTrackerActivationThreshold",
    "XML_SetAllocTrackerMaximumAmplification",
    "XML_SetHashSalt16Bytes",
}
GETTERS = ["CurrentLineNumber", "CurrentColumnNumber", "CurrentByteIndex", "ErrorCode"]


def fetch(branch, path):
    # curl uses the system's certificate store (python.org builds of Python
    # often lack one, which breaks urllib's HTTPS verification).
    url = f"https://raw.githubusercontent.com/python/cpython/{branch}/{path}"
    return subprocess.run(["curl", "-sSfL", url], check=True, capture_output=True, text=True).stdout


def strip_strings_and_comments(c_source):
    """Drop C comments and string literals, so names that only appear in
    messages (e.g. "XML_ParserCreate failed") don't count as uses."""
    c_source = re.sub(r"/\*.*?\*/", " ", c_source, flags=re.S)
    c_source = re.sub(r"//[^\n]*", " ", c_source)
    return re.sub(r'"(?:\\.|[^"\\\n])*"', '""', c_source)


def main():
    header = open(sys.argv[1]).read()
    api = set(re.findall(r"XMLPARSEAPI\([^)]*\)\s*\n?\s*(XML_\w+)\s*\(", header))
    branches = sys.argv[2:] or ["3.13", "3.14", "main"]
    results = {}
    for br in branches:
        pyexpat = strip_strings_and_comments(fetch(br, "Modules/pyexpat.c"))
        etree = strip_strings_and_comments(fetch(br, "Modules/_elementtree.c"))
        ids = set(re.findall(r"\bXML_\w+", pyexpat + etree))
        ids |= {"XML_Set" + h for h in re.findall(r"HANDLER_INFO\((\w+)\)", pyexpat)}
        ids |= {"XML_Get" + g for g in GETTERS}
        results[br] = sorted((ids & api) - EXCLUDED)
    first = results[branches[0]]
    for br in branches:
        same = "same" if results[br] == first else "DIFFERS"
        print(f"{br}: {len(results[br])} functions ({same} as {branches[0]})")
    print("\n".join(first))


if __name__ == "__main__":
    main()
