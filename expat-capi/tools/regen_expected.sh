#!/bin/sh
# Regenerate tests/c/*.expected from the real libexpat: build each probe
# against it and record the output. libexpat is used only as a black box.
# Usage: regen_expected.sh [<libexpat prefix>]   (default: brew --prefix expat)
set -e
prefix=${1:-$(brew --prefix expat)}
here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for src in "$here"/tests/c/*.c; do
  name=$(basename "$src" .c)
  cc -Wall -DXML_DTD -I"$prefix/include" -o "$tmp/$name" "$src" -L"$prefix/lib" -lexpat -Wl,-rpath,"$prefix/lib"
  "$tmp/$name" > "$here/tests/c/$name.expected"
  echo "wrote tests/c/$name.expected ($(wc -l < "$here/tests/c/$name.expected") lines, $("$tmp/$name" >/dev/null; otool -L "$tmp/$name" 2>/dev/null | grep -o '/[^ ]*libexpat[^ ]*' || true))"
done
