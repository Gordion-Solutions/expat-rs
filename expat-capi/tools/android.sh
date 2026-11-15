#!/bin/bash
# Build expat-capi for Android with the NDK and run the C probes on Android's
# C library (bionic): each probe is linked statically against bionic, so it
# runs on any Linux kernel of the same architecture, here in an arm64 Docker
# container (emulated on other machines). The output must be exactly
# libexpat's, as in `cargo test` (tests/c/*.expected). With a wayland work
# dir from wayland_scanner.sh, it also runs AOSP's wayland scanner built for
# Android against the one built on the host's libexpat.
#
# Usage: android.sh <work dir> [<wayland work dir>]
#
# Needs the Android NDK (ANDROID_NDK_HOME), its Rust target
# (`rustup target add aarch64-linux-android`), Docker, and for the wayland
# part an installed libexpat (LIBEXPAT_PREFIX, or `brew --prefix expat`).
set -euo pipefail
work=${1:?usage: android.sh <work dir> [<wayland work dir>]}
wayland=${2:-}
root=$(cd "$(dirname "$0")/../.." && pwd)
ndk=${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}
api=24
mkdir -p "$work"
work=$(cd "$work" && pwd)
bin=$(echo "$ndk"/toolchains/llvm/prebuilt/*/bin)
clang=$bin/aarch64-linux-android$api-clang
image=${DOCKER_IMAGE:-alpine}

export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$clang
(cd "$root" && cargo build -q --release -p expat-capi --target aarch64-linux-android --target-dir "$work/target")
lib=$work/target/aarch64-linux-android/release
echo "== libexpat.so: $("$bin/llvm-readelf" -d "$lib/libexpat.so" | grep -o 'soname: \[.*\]')," \
  "$("$bin/llvm-nm" -D --defined-only "$lib/libexpat.so" | grep -c ' T XML_') XML_ functions," \
  "$("$bin/llvm-nm" -D --defined-only "$lib/libexpat.so" | grep -vc ' XML_' || true) other symbols"

rm -rf "$work/probes"
mkdir "$work/probes"
for src in "$root"/expat-capi/tests/c/*.c; do
  n=$(basename "$src" .c)
  "$clang" -static -Wall -Werror -DXML_DTD -O1 -I"$root/expat-capi/include" -o "$work/probes/$n" "$src" "$lib/libexpat.a"
  cp "${src%.c}.expected" "$work/probes/"
done
status=0
docker run --rm --platform linux/arm64 -v "$work/probes:/p:ro" "$image" sh -c '
  s=0
  for e in /p/*.expected; do
    n=$(basename "$e" .expected)
    if /p/$n 2>&1 | cmp -s - "$e"; then echo "== $n: identical to libexpat"; else echo "== $n: DIFFERS"; s=1; fi
  done
  exit $s' || status=1

if [ -n "$wayland" ]; then
  wayland=$(cd "$wayland" && pwd)
  prefix=${LIBEXPAT_PREFIX:-$(brew --prefix expat 2>/dev/null || true)}
  src=$wayland/wayland/src
  out=$work/wayland
  rm -rf "$out"
  mkdir -p "$out/lib" "$out/android"
  cc -O1 -w -I"$src" -I"$prefix/include" -o "$out/scanner-libexpat" "$src/scanner.c" "$src/wayland-util.c" \
    -L"$prefix/lib" -lexpat -Wl,-rpath,"$prefix/lib"
  "$clang" -static -O1 -w -I"$src" -I"$root/expat-capi/include" -o "$out/scanner-android" \
    "$src/scanner.c" "$src/wayland-util.c" "$lib/libexpat.a"
  find "$wayland/wayland/protocol" "$wayland/wayland-protocols" -name '*.xml' | sort > "$out/protocols.txt"
  cat > "$out/run.sh" <<'EOF'
#!/bin/sh
# run.sh <scanner> <out dir> <list>: every output mode, one numbered result each.
n=0
while read -r f; do
  for mode in client-header server-header private-code public-code "-c client-header" "-c server-header"; do
    n=$((n + 1))
    $1 $mode "$f" "$2/$n.out" > "$2/$n.log" 2>&1
    echo "exit $?" >> "$2/$n.log"
  done
done < "$3"
echo "$n"
EOF
  chmod +x "$out/run.sh"
  n=$("$out/run.sh" "$out/scanner-libexpat" "$out/lib" "$out/protocols.txt")
  # The same paths inside the container, so messages naming files match.
  docker run --rm --platform linux/arm64 -v "$wayland:$wayland:ro" -v "$out:$out" "$image" \
    "$out/run.sh" "$out/scanner-android" "$out/android" "$out/protocols.txt" > /dev/null
  if diff -r "$out/lib" "$out/android" > "$out/diff.txt"; then
    echo "== wayland scanner: $n runs, all identical"
  else
    echo "== wayland scanner: differences in $out/diff.txt"
    status=1
  fi
fi
exit $status
