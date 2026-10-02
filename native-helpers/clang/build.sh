#!/bin/sh
# 使用显式固定 LLVM SDK 构建 helper，不查找或执行被分析项目的工具。
set -eu
sdk=${1:?usage: build.sh /absolute/LLVM-21.1.8}
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
version=$("$sdk/bin/llvm-config" --version)
[ "$version" = 21.1.8 ] || { echo 'LLVM 21.1.8 required' >&2; exit 1; }
mkdir -p "$base/build/runtime/lib"
cp "$sdk/lib/libclang-cpp.dylib" "$base/build/runtime/lib/libclang-cpp.dylib"
/usr/bin/clang++ -std=c++17 -O1 -g0 -fexceptions -fno-rtti \
  -I"$sdk/include" "$base/src/main.cpp" \
  -Wl,-rpath,@executable_path/runtime/lib -Wl,-rpath,@executable_path/../runtime/lib "$sdk/lib/libclang-cpp.dylib" \
  -o "$base/build/narrant-clang"
