#!/bin/bash
# Run expat-capi on an Android device or emulator (the one adb talks to):
# build it with the NDK for the device's ABI, link the C probes and AOSP's
# wayland scanner dynamically against it (soname libexpat.so, like AOSP's),
# and run them twice on the device, loading our libexpat.so and then the
# device's own (/system/lib64/libexpat.so). Our output must be exactly
# libexpat 2.8.2's (tests/c/*.expected); the device's own libexpat may be
# older and differ, so its results are shown, not checked.
#
# Usage: android_device.sh <work dir> <wayland work dir>
#   <wayland work dir>  as made by wayland_scanner.sh (AOSP's external/wayland
#                       and external/wayland-protocols)
#
# Needs the Android NDK (ANDROID_NDK_HOME), the Rust target for the device
# (e.g. `rustup target add x86_64-linux-android`), and adb on PATH (or ADB).
set -euo pipefail
work=${1:?usage: android_device.sh <work dir> <wayland work dir>}
wayland=$(cd "${2:?usage: android_device.sh <work dir> <wayland work dir>}" && pwd)
root=$(cd "$(dirname "$0")/../.." && pwd)
ndk=${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to the NDK}
adb=${ADB:-adb}
api=24
mkdir -p "$work"
work=$(cd "$work" && pwd)
bin=$(echo "$ndk"/toolchains/llvm/prebuilt/*/bin)

abi=$("$adb" shell getprop ro.product.cpu.abi | tr -d '\r')
case $abi in
  arm64-v8a) triple=aarch64-linux-android ;;
  x86_64) triple=x86_64-linux-android ;;
  *) echo "unsupported ABI $abi"; exit 1 ;;
esac
echo "== device: Android $("$adb" shell getprop ro.build.version.release | tr -d '\r'), $abi"
clang=$bin/$triple$api-clang
upper=$(echo "$triple" | tr 'a-z-' 'A-Z_')
export "CARGO_TARGET_${upper}_LINKER=$clang"
(cd "$root" && cargo build -q --release -p expat-capi --target "$triple" --target-dir "$work/target")

out=$work/device
rm -rf "$out"
mkdir -p "$out/bin" "$out/p"
cp "$work/target/$triple/release/libexpat.so" "$out/bin/"
for src in "$root"/expat-capi/tests/c/*.c; do
  "$clang" -Wall -Werror -DXML_DTD -O1 -I"$root/expat-capi/include" -o "$out/bin/$(basename "$src" .c)" "$src" \
    -L"$out/bin" -lexpat
  cp "${src%.c}.expected" "$out/"
done
cat > "$out/which.c" <<'EOF'
#define _GNU_SOURCE
#include <dlfcn.h>
#include <expat.h>
#include <stdio.h>
int main(void) {
  Dl_info i;
  dladdr((void *)XML_ParserCreate, &i);
  printf("%s from %s\n", XML_ExpatVersion(), i.dli_fname);
  return 0;
}
EOF
"$clang" -I"$root/expat-capi/include" -o "$out/bin/which" "$out/which.c" -L"$out/bin" -lexpat
src=$wayland/wayland/src
"$clang" -O1 -w -I"$src" -I"$root/expat-capi/include" -o "$out/bin/wayland-scanner" "$src/scanner.c" \
  "$src/wayland-util.c" -L"$out/bin" -lexpat
(cd "$wayland" && find wayland/protocol wayland-protocols -name '*.xml' | sort) > "$out/list.txt"
(cd "$wayland" && tar -cf - -T "$out/list.txt") | tar -xf - -C "$out/p"

dir=/data/local/tmp/expat-capi
"$adb" shell "rm -rf $dir && mkdir -p $dir"
"$adb" push -q "$out/." "$dir/"
"$adb" shell "cd $dir && chmod 755 bin/* && sh -s" <<'EOF'
ours() { LD_LIBRARY_PATH=$PWD/bin "$@"; }
echo "== loaded: ours $(ours bin/which); device's $(bin/which)"
status=0
for e in *.expected; do
  n=${e%.expected}
  ours bin/$n > $n.ours 2>&1 || true
  if cmp -s $n.ours $e; then echo "== $n: identical to libexpat"; else echo "== $n: DIFFERS"; status=1; fi
  bin/$n > $n.device 2>&1
  s=$?
  if cmp -s $n.device $n.ours; then echo "   device's libexpat: the same"
  else echo "   device's libexpat: differs from line $(cmp $n.device $n.ours 2>/dev/null | sed 's/.* line //'), exit $s"; fi
done
n=0
mkdir -p w/ours w/device
cd p
while read -r f; do
  for mode in client-header server-header private-code public-code "-c client-header" "-c server-header"; do
    n=$((n + 1))
    for lib in ours device; do
      if [ $lib = ours ]; then ours ../bin/wayland-scanner $mode $f ../w/$lib/$n.out > ../w/$lib/$n.log 2>&1
      else ../bin/wayland-scanner $mode $f ../w/$lib/$n.out > ../w/$lib/$n.log 2>&1; fi
      echo "exit $?" >> ../w/$lib/$n.log
    done
  done
done < ../list.txt
cd ..
if diff -r w/ours w/device > /dev/null; then echo "== wayland scanner: $n runs, ours and the device's identical"
else echo "== wayland scanner: $n runs, differences:"; diff -rq w/ours w/device | head; status=1; fi
exit $status
EOF
