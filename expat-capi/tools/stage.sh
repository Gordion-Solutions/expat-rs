#!/bin/sh
# Copy the built library to <dir> under libexpat's real file names
# (libexpat.1.dylib / libexpat.so.1), so that programs linked with
# -L<dir> -lexpat -Wl,-rpath,<dir> load it, not the system libexpat.
set -e
dir=${1:?usage: stage.sh <dir> [profile]}
profile=${2:-release}
root=$(cd "$(dirname "$0")/../.." && pwd)
mkdir -p "$dir"
case $(uname) in
  Darwin) cp "$root/target/$profile/libexpat.dylib" "$dir/libexpat.1.dylib"; ln -sf libexpat.1.dylib "$dir/libexpat.dylib" ;;
  *)      cp "$root/target/$profile/libexpat.so" "$dir/libexpat.so.1"; ln -sf libexpat.so.1 "$dir/libexpat.so" ;;
esac
