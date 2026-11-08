#!/bin/bash
# Build AOSP's wayland scanner (external/wayland, a libexpat user in Android)
# twice, on libexpat and on expat-capi, and run both over every protocol in
# AOSP's external/wayland and external/wayland-protocols, in every output
# mode, from a file and from stdin, then over broken copies of some of them
# (truncated, unclosed or mismatched tags, unknown elements, bad attributes,
# invalid characters). The generated code, the messages (with the line and
# column the scanner gets from the parser) and the exit status must be the
# same.
#
# Usage: wayland_scanner.sh <work dir>
#
# Needs git, a C compiler, cargo, python3 and an installed libexpat:
# LIBEXPAT_PREFIX, or `brew --prefix expat`.
set -euo pipefail
work=${1:?usage: wayland_scanner.sh <work dir>}
root=$(cd "$(dirname "$0")/../.." && pwd)
prefix=${LIBEXPAT_PREFIX:-$(brew --prefix expat 2>/dev/null || true)}
mkdir -p "$work"
work=$(cd "$work" && pwd)

for repo in wayland wayland-protocols; do
  [ -d "$work/$repo" ] || git clone -q --depth 1 "https://android.googlesource.com/platform/external/$repo" "$work/$repo"
done

(cd "$root" && cargo build -q --release -p expat-capi)
target=${CARGO_TARGET_DIR:-$root/target}
src=$work/wayland/src
extra=""
[ "$(uname)" = Linux ] && extra="-lpthread -ldl -lm"
cc -O1 -w -I"$src" -I"$prefix/include" -o "$work/scanner-libexpat" "$src/scanner.c" "$src/wayland-util.c" \
  -L"$prefix/lib" -lexpat -Wl,-rpath,"$prefix/lib"
cc -O1 -w -I"$src" -I"$root/expat-capi/include" -o "$work/scanner-capi" "$src/scanner.c" "$src/wayland-util.c" \
  "$target/release/libexpat.a" $extra

# Broken copies, the same every run.
rm -rf "$work/broken"
mkdir "$work/broken"
python3 - "$work" <<'PY'
import pathlib, random, sys
work = pathlib.Path(sys.argv[1])
random.seed(7)
files = [work / 'wayland/protocol/wayland.xml'] + sorted((work / 'wayland-protocols').rglob('*.xml'))[:12]
n = 0
def put(data):
    global n
    n += 1
    (work / 'broken' / f'{n:04d}.xml').write_bytes(data)
for f in files:
    d = f.read_bytes()
    for _ in range(12):
        put(d[:random.randrange(1, len(d))])
    s = d.decode('utf-8')
    for old, new in [('</request>', ''), ('</event>', '</request>'), ('<arg ', '<argument '), ('name="', 'nam="'),
                     ('type="', 'type='), ('<description', '<description\x01'), ('summary="', 'summary="&bogus;'),
                     ('version="', 'version="x'), ('<interface ', '<interface name="dup" '),
                     ('<enum ', '<enum bitfield="maybe" '),
                     ('<protocol', '<!DOCTYPE protocol [<!ENTITY e "x">]>\n<protocol')]:
        t = s.replace(old, new, 1)
        if t != s:
            put(t.encode('utf-8'))
    put((s + '<trailing/>').encode('utf-8'))
PY

compare() {
  local out=$1 list=$2
  rm -rf "$out"; mkdir -p "$out/lib" "$out/capi"
  local n=0 same=0 f mode via impl b
  set +e
  while read -r f; do
    for mode in client-header server-header private-code public-code "-c client-header" "-c server-header"; do
      for via in file stdin; do
        n=$((n + 1))
        for impl in lib capi; do
          b=$work/scanner-libexpat; [ $impl = capi ] && b=$work/scanner-capi
          if [ $via = file ]; then
            $b $mode "$f" "$out/$impl/$n.out" > "$out/$impl/$n.log" 2>&1
          else
            $b $mode < "$f" > "$out/$impl/$n.out" 2> "$out/$impl/$n.log"
          fi
          echo "exit $?" >> "$out/$impl/$n.log"
        done
        if cmp -s "$out/lib/$n.out" "$out/capi/$n.out" && cmp -s "$out/lib/$n.log" "$out/capi/$n.log"; then
          same=$((same + 1))
        else
          echo "  differs: $f [$mode] from $via"
        fi
      done
    done
  done < "$list"
  set -e
  echo "  $n runs, $same identical"
  [ "$n" = "$same" ]
}

status=0
find "$work/wayland/protocol" "$work/wayland-protocols" -name '*.xml' | sort > "$work/protocols.txt"
echo "== $(wc -l < "$work/protocols.txt" | tr -d ' ') protocols"
compare "$work/out" "$work/protocols.txt" || status=1
find "$work/broken" -name '*.xml' | sort > "$work/broken.txt"
echo "== $(wc -l < "$work/broken.txt" | tr -d ' ') broken files"
compare "$work/out-broken" "$work/broken.txt" || status=1
exit $status
