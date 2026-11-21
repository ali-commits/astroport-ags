#!/bin/bash
# Build Kyty's renderer (libkyty_gpu.so) and AnyPS5 (system libraries, relinker) from engines/ into build/,
# plus the three Linux forwarders. Needs network on a fresh configure (Kyty downloads its 3rdparty deps).
# Installs nothing into any run folder.
set -e
N="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"   # repository root
cd "$N"
mkdir -p build

# run <log> <cmd...>: output to the log; on failure show the first ten error lines and stop
run() {
  local log=$1; shift
  "$@" >> "$log" 2>&1 || { grep -E 'error:|FAILED' "$log" | head -10; echo "FAIL: $* (see $log)"; exit 1; }
}

: > build/kyty.log
run build/kyty.log cmake -S engines/kyty -B build/kyty -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC -DCMAKE_EXPORT_COMPILE_COMMANDS=ON
run build/kyty.log ninja -C build/kyty kyty_emulator
python3 -I engines/kyty/tools/hybrid/build_kyty_gpu_linux.py build/kyty build/kyty-gpu

: > build/anyps5.log
run build/anyps5.log cmake -S engines/anyps5 -B build/anyps5 -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++
run build/anyps5.log xargs -a scripts/prx-targets.txt ninja -C build/anyps5
run build/anyps5.log ninja -C build/anyps5 relinker

mkdir -p build/stage
ln -sf ../kyty-gpu/libkyty_gpu.so ../anyps5/core/libs/libs/libAstroHybridHost.prx build/stage/
python3 -I engines/kyty/tools/hybrid/build_linux_shims.py build/stage build/shims

for f in build/kyty-gpu/libkyty_gpu.so build/shims/libSceAgc.prx build/shims/libSceAgcDriver.prx build/shims/libSceVideoOut.prx; do
  test -f "$f" || { echo "FAIL: missing $f"; exit 1; }
done
test -x build/anyps5/core/relinker/relinker || { echo "FAIL: missing build/anyps5/core/relinker/relinker"; exit 1; }
n=$(find build/anyps5/core/libs/libs -maxdepth 1 -name '*.prx' | wc -l)
[ "$n" -gt 0 ] || { echo "FAIL: missing build/anyps5/core/libs/libs/*.prx"; exit 1; }
echo "engines built: libkyty_gpu.so, $n system libraries, relinker, 3 forwarders"
