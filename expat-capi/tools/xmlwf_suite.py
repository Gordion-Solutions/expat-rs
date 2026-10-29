#!/usr/bin/env python3
"""Run libexpat's own xmlwf over the W3C XML conformance suite twice, once
on libexpat and once on expat-capi, and compare everything it reports.

Usage: xmlwf_suite.py <xmlwf> <xmlconf dir> <staged expat-capi dir> [modes]

<xmlwf> is a dynamically linked xmlwf from libexpat (e.g. Homebrew's);
<staged dir> is where tools/stage.sh put libexpat.1.dylib / libexpat.so.1,
loaded in libexpat's place through DYLD_LIBRARY_PATH / LD_LIBRARY_PATH.
Modes (comma-separated, default all): plain, ns (-n), out (-d: written
output), out-ns, read (-r). Compares exit status, messages (with error
positions) and written output, and summarises the differences.
"""
import collections
import glob
import os
import re
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

MODES = {"plain": [], "ns": ["-n"], "out": ["-d"], "out-ns": ["-n", "-d"], "read": ["-r"]}


def main():
    xmlwf, suite, staged = sys.argv[1:4]
    modes = {k: MODES[k] for k in sys.argv[4].split(",")} if len(sys.argv) > 4 else MODES
    files = sorted(f for f in glob.glob(f"{suite}/**/*.xml", recursive=True) if "/out/" not in f)
    var = "DYLD_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
    envs = {"libexpat": dict(os.environ), "expat-capi": dict(os.environ, **{var: staged})}

    def run(env, args, path):
        with tempfile.TemporaryDirectory() as d:
            cmd = [xmlwf, *(a for a in args if a != "-d"), *(["-d", d] if "-d" in args else []), path]
            r = subprocess.run(cmd, capture_output=True, env=env, timeout=60)
            written = b"".join(open(os.path.join(d, n), "rb").read() for n in sorted(os.listdir(d)))
            return r.returncode, r.stdout.replace(path.encode(), b"F"), r.stderr.replace(path.encode(), b"F"), written

    def check(path):
        out = []
        for mode, args in modes.items():
            a, b = run(envs["libexpat"], args, path), run(envs["expat-capi"], args, path)
            if a != b:
                out.append((mode, path[len(suite):], a, b))
        return out

    with ThreadPoolExecutor(os.cpu_count()) as ex:
        diffs = [d for ds in ex.map(check, files) for d in ds]

    msg = lambda o: re.sub(rb"F:\d+:\d+: ", b"", o).strip().decode(errors="replace")
    def category(a, b):
        if (a[0] == 0) != (b[0] == 0):
            return f"verdict: libexpat {'accepts' if a[0] == 0 else 'rejects'}, expat-capi {'accepts' if b[0] == 0 else 'rejects'}"
        if a[0] == 0:
            return "well-formed: output differs"
        return "error message differs" if msg(a[1]) != msg(b[1]) else "error position differs"

    print(f"{len(files)} files x {len(modes)} modes")
    for (mode, cat), n in sorted(collections.Counter((m, category(a, b)) for m, _, a, b in diffs).items()):
        print(f"  {mode:7} {cat:45} {n}")
    for mode, path, a, b in diffs:
        if category(a, b).startswith(("verdict", "well-formed")):
            print(f"  {mode}: {path}\n    libexpat:   {a[0]} {a[1][:100]!r}\n    expat-capi: {b[0]} {b[1][:100]!r}")


if __name__ == "__main__":
    main()
