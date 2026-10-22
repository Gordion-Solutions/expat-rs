#!/bin/bash
# Build CPython against expat-capi and run its XML tests, then the same
# tests with libexpat in expat-capi's place for comparison.
#
# Usage: cpython.sh <work dir> [branch] [--full]
#   <work dir>  where CPython is cloned and built (reused if present)
#   branch      CPython branch to build (default 3.14)
#   --full      also run CPython's whole test suite on expat-capi
#
# Needs git, a C compiler, cargo, and (for the comparison run) an
# installed libexpat: LIBEXPAT_PREFIX, or `brew --prefix expat`.
set -euo pipefail
work=${1:?usage: cpython.sh <work dir> [branch] [--full]}
branch=${2:-3.14}
full=${3:-}
root=$(cd "$(dirname "$0")/../.." && pwd)
src="$work/cpython-$branch"
lib="$work/expat-capi-lib"

# The library under libexpat's own file names (stage.sh builds it first).
"$root/expat-capi/tools/stage.sh" "$lib"

if [ ! -d "$src" ]; then
  git clone -q --depth 1 -b "$branch" https://github.com/python/cpython.git "$src"
fi
cd "$src"
if [ ! -f Makefile ]; then
  ./configure -q --with-system-expat --with-pydebug \
    LIBEXPAT_CFLAGS="-I$root/expat-capi/include" \
    LIBEXPAT_LDFLAGS="-L$lib -Wl,-rpath,$lib -lexpat"
fi
make -j"$(getconf _NPROCESSORS_ONLN)" -s
python=./python
[ -x python.exe ] && python=./python.exe

# Check which libexpat pyexpat loads: expat-capi says expat_2.7.1.
"$python" -c "import pyexpat; print('pyexpat on', pyexpat.EXPAT_VERSION)"

tests=(test_pyexpat test_xml_etree test_xml_etree_c test_sax test_minidom test_pulldom
       test_xml_dom_minicompat test_xml_dom_xmlbuilder test_plistlib test_xmlrpc test_docxmlrpc)
echo "== XML tests on expat-capi"
"$python" -m test -j4 "${tests[@]}"

prefix=${LIBEXPAT_PREFIX:-$(brew --prefix expat 2>/dev/null || true)}
if [ -n "$prefix" ] && [ -d "$prefix/lib" ]; then
  echo "== XML tests on libexpat ($prefix)"
  if [ "$(uname)" = Darwin ]; then
    DYLD_LIBRARY_PATH="$prefix/lib" "$python" -m test -j4 "${tests[@]}"
  else
    LD_LIBRARY_PATH="$prefix/lib" "$python" -m test -j4 "${tests[@]}"
  fi
fi

if [ "$full" = --full ]; then
  echo "== whole test suite on expat-capi"
  "$python" -m test -j"$(getconf _NPROCESSORS_ONLN)" --timeout 900
fi
