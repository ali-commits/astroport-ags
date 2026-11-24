#!/bin/bash
# Build everything that runs in the game process (docs/rewrite/design.md 1.4): Kyty's renderer
# (libkyty_renderer.so), AnyPS5 (system libraries, libAstroportHost.prx, relinker), the bridge and
# the three forwarders. Needs network on a fresh configure (Kyty downloads its 3rdparty deps).
# Installs nothing into any run folder; "$BUILD_DIR/install" is the install tree astroport-run reads
# (--install), made of links into the build folder.
#   BUILD_DIR=<dir>   output folder, relative to the repository root or absolute (default: build)
#   JOBS=<n>          parallel compile jobs (default: ninja's and cargo's own)
set -e
N="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"   # repository root
cd "$N"
B=$(realpath -m "${BUILD_DIR:-build}")
mkdir -p "$B/libs" "$B/fwd"
J=${JOBS:+-j$JOBS}   # passed on the command line: xargs would not see a shell function

# run <log> <cmd...>: output to the log; on failure show the first ten error lines and stop
run() {
  local log=$1; shift
  "$@" >> "$log" 2>&1 || { grep -E 'error:|FAILED' "$log" | head -10; echo "FAIL: $* (see $log)"; exit 1; }
}
fail() { echo "FAIL: $*"; exit 1; }

: > "$B/kyty.log"
# -fPIC through the flags, not CMAKE_POSITION_INDEPENDENT_CODE: it also reaches Kyty's 3rdparty builds.
run "$B/kyty.log" cmake -S engines/kyty -B "$B/kyty" -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC -DKYTY_BUILD_LAUNCHER=OFF
run "$B/kyty.log" ninja $J -C "$B/kyty" kyty_renderer

: > "$B/anyps5.log"
run "$B/anyps5.log" cmake -S engines/anyps5 -B "$B/anyps5" -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++
run "$B/anyps5.log" xargs -a scripts/prx-targets.txt ninja $J -C "$B/anyps5"
run "$B/anyps5.log" ninja $J -C "$B/anyps5" relinker

: > "$B/bridge.log"
P="$B/anyps5/core/libs/libs"   # AnyPS5's libraries
ln -sf ../kyty/libkyty_renderer.so ../anyps5/core/libs/libs/libAstroportHost.prx "$B/libs/"
ASTROPORT_ENGINE_LIBS="$B/libs" run "$B/bridge.log" cargo build $J --release --locked --manifest-path bridge/Cargo.toml
cp bridge/target/release/libastroport_bridge.so "$B/libs/"   # the bridge has no soname: link it by file name
BRIDGE="$B/libs/libastroport_bridge.so"
run "$B/bridge.log" bridge/target/release/nidgen engines/kyty/src/libs "$B/fwd"
FORWARDERS=()
for l in libSceAgc libSceAgcDriver libSceVideoOut; do
  run "$B/bridge.log" cc -shared -nostdlib -Wl,-z,now -Wl,-soname,$l.prx -Wl,-rpath,'$ORIGIN' \
    -o "$B/fwd/$l.prx" "$B/fwd/$l.S" -L"$B/libs" -l:libastroport_bridge.so
  FORWARDERS+=("$B/fwd/$l.prx")
done

defined() { nm -D --defined-only "$@" | awk 'NF > 1 { print $NF }' | sort -u; }
undefined() { nm -D --undefined-only "$@" | awk 'NF > 1 { print $NF }' | sort -u; }

# 1. The renderer exports one symbol: nothing of its SDL3 and C++ can meet the host's (design 4.4).
exported=$(defined "$B/kyty/libkyty_renderer.so" | tr '\n' ' ')
[ "$exported" = "kyty_embed_api " ] || fail "libkyty_renderer.so must export only kyty_embed_api, exports: ${exported:0:300}"

# 2. What AnyPS5's other libraries take from its own graphics libraries, ours provide too.
missing=$(comm -12 <(undefined "$P/libSceJobManager.prx" "$P/libSceSystemService.prx") \
                   <(defined "$P/libSceAgc.prx" "$P/libSceAgcDriver.prx" "$P/libSceVideoOut.prx") |
          comm -23 - <(defined "${FORWARDERS[@]}" "$P/libAstroportHost.prx") | tr '\n' ' ')
[ -z "$missing" ] || fail "libSceJobManager / libSceSystemService import what no forwarder provides: $missing"

# 3. The forwarders and the bridge were generated from the same export table.
entry=$(defined "$BRIDGE" | grep '^astroport_entry_' | tr '\n' ' ')
for f in "${FORWARDERS[@]}"; do
  wanted=$(undefined "$f" | grep '^astroport_entry_' | tr '\n' ' ')
  [ -n "$entry" ] && [ "$wanted" = "$entry" ] || fail "$f imports '$wanted', the bridge defines '$entry'"
done

test -x "$B/anyps5/core/relinker/relinker" || fail "missing $B/anyps5/core/relinker/relinker"
n=$(find "$P" -maxdepth 1 -name '*.prx' | wc -l)
[ "$n" -gt 0 ] || fail "missing $P/*.prx"

I="$B/install"
mkdir -p "$I/lib" "$I/anyps5"
ln -sfn "$B/kyty/libkyty_renderer.so" "$BRIDGE" "${FORWARDERS[@]}" "$B/fwd/astroport-exports.txt" "$I/lib/"
ln -sfn "$P" "$I/anyps5/libs"
ln -sfn "$B/anyps5/core/relinker/relinker" "$I/anyps5/relinker"
ln -sfn "$N/patches" "$N/ags-extension.toml" "$I/"
echo "engines built: libkyty_renderer.so, $n system libraries, relinker, bridge ($entry), 3 forwarders; install tree $I"
