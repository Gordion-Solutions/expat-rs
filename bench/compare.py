"""Best-of-5 wall time: libexpat's xmlwf vs expat-rs's xmlwf (cargo build --release).

Usage: [LIBEXPAT_XMLWF=/path/to/xmlwf] compare.py [<dir from generate.py>]
"""
import os, subprocess, sys, time
B = sys.argv[1] if len(sys.argv) > 1 else "bench-data"
LIBEXPAT = os.environ.get("LIBEXPAT_XMLWF", "xmlwf")
OURS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "target", "release", "xmlwf")
RUNS = 5
import shutil
if not shutil.which(LIBEXPAT):
    sys.exit(f"libexpat's xmlwf not found ({LIBEXPAT!r}); set LIBEXPAT_XMLWF, "
             "e.g. LIBEXPAT_XMLWF=$(brew --prefix expat)/bin/xmlwf")
if not os.path.exists(OURS):
    sys.exit(f"{OURS} not found; run cargo build --release first")
def best(cmd):
    times = []
    for _ in range(RUNS):
        t = time.perf_counter()
        r = subprocess.run(cmd, capture_output=True)
        times.append(time.perf_counter() - t)
        if r.returncode != 0 or r.stdout:
            sys.exit(f"{cmd}: rc={r.returncode} {r.stderr[:200]} {r.stdout[:200]}")
    return min(times)
print(f"{'document':14} {'MB':>6}  {'parser':34} {'best s':>7} {'MB/s':>7} {'vs libexpat':>11}")
for doc in ["records.xml", "articles.xml", "feed.xml", "unicode.xml"]:
    path = os.path.join(B, doc)
    mb = os.path.getsize(path) / 1048576
    ns = doc == "feed.xml"
    rows = [("libexpat xmlwf" + (" -n" if ns else ""), [LIBEXPAT, *(["-n"] if ns else []), path]),
            ("expat-rs Parser (whole)", [OURS, *(["--namespaces"] if ns else []), path]),
            ("expat-rs StreamParser (64 KB chunks)", [OURS, "--chunk", "65536", *(["--namespaces"] if ns else []), path])]
    base = None
    for label, cmd in rows:
        t = best(cmd)
        base = base or t
        print(f"{doc:14} {mb:6.1f}  {label:34} {t:7.3f} {mb/t:7.1f} {base/t:10.2f}x")
