#!/bin/sh
# Copy the built library to <dir> under libexpat's real file names
# (libexpat.1.dylib / libexpat.so.1), so that programs linked with
# -L<dir> -lexpat -Wl,-rpath,<dir> load it, not the system libexpat.
set -e
dir=${1:?usage: stage.sh <dir> [profile]}
profile=${2:-release}
root=$(cd "$(dirname "$0")/../.." && pwd)
# cargo runs in $root, so a relative CARGO_TARGET_DIR is relative to it.
target=${CARGO_TARGET_DIR:-target}
case $target in /*) ;; *) target=$root/$target ;; esac
# Build first: cargo test doesn't update target/<profile>/, so copying
# without building can stage a stale library.
if [ "$profile" = release ]; then flag=--release; else flag="--profile $profile"; fi
(cd "$root" && cargo build -q $flag -p expat-capi)
target=${CARGO_TARGET_DIR:-$root/target}
mkdir -p "$dir"
case $(uname) in
  Darwin) cp "$target/$profile/libexpat.dylib" "$dir/libexpat.1.dylib"; ln -sf libexpat.1.dylib "$dir/libexpat.dylib" ;;
  *)      cp "$target/$profile/libexpat.so" "$dir/libexpat.so.1"; ln -sf libexpat.so.1 "$dir/libexpat.so" ;;
esac
