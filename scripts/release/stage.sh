#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.." && pwd)"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

[[ $# -eq 1 && -n $1 ]] || fail 'usage: stage.sh <stage-dir>'
stage=$1
shopt -s dotglob globstar nullglob

if [[ -e "$stage" || -L "$stage" ]]; then
  [[ -d "$stage" && ! -L "$stage" ]] || fail "$stage is not a directory"
  entries=("$stage"/*)
  (( ${#entries[@]} == 0 )) || fail "$stage is not empty"
fi

# BUILD_DIR as for scripts/build-engines.sh (default build), inside the repository.
B=$(realpath -m --relative-to="$ROOT" -- "$ROOT/${BUILD_DIR:-build}")
[[ $B != ../* && $B != .. ]] || fail "BUILD_DIR must be inside the repository"
engine_hint="run scripts/build-engines.sh (BUILD_DIR=$B)"
licence_hint='restore repository licence files'
anyps5=$B/anyps5/core
system_libs="$anyps5/libs/libs"
sdk=$B/release/dlss-sdk

sources=()
destinations=()
modes=()
add_file() {
  local source=$1 destination=$2 mode=$3 hint=$4
  [[ -f "$ROOT/$source" ]] || fail "missing $source ($hint)"
  sources+=("$source")
  destinations+=("$destination")
  modes+=("$mode")
}

add_file adapter/target/release/astroport-adapter bin/astroport-adapter 0755 \
  'build the astroport-adapter crate in adapter/, step 3'
add_file runner/target/release/astroport-run bin/astroport-run 0755 \
  'build the astroport-run crate in runner/, step 4'
add_file ags-extension.toml ags-extension.toml 0644 'step 3 files'
[[ -d "$ROOT/assets" ]] || fail 'missing assets/ (step 3 files)'
for file in "$ROOT"/assets/**/*; do
  [[ -d "$file" ]] && continue
  relpath=${file#"$ROOT/"}
  add_file "$relpath" "$relpath" 0644 'step 3 files'
done
for file in patches/settings.json patches/versions.json; do
  [[ -f "$ROOT/$file" ]] || fail "missing $file (step 3 files)"
done
for file in "$ROOT"/patches/*.json; do
  relpath=${file#"$ROOT/"}
  add_file "$relpath" "$relpath" 0644 'step 3 files'
done

add_file patcher/target/release/libastro_patcher.so lib/libastro_patcher.so 0644 \
  'cargo build --release --manifest-path patcher/Cargo.toml'
# The install tree astroport-run reads (docs/rewrite/design.md 1.4, 7.4): lib/ holds the renderer, the
# bridge and the three forwarders; anyps5/libs holds AnyPS5's libraries, its own three graphics
# libraries included (the runner leaves those out of a run folder).
add_file $B/kyty/libkyty_renderer.so lib/libkyty_renderer.so 0644 "$engine_hint"
add_file $B/libs/libastroport_bridge.so lib/libastroport_bridge.so 0644 "$engine_hint"
for name in libSceAgc.prx libSceAgcDriver.prx libSceVideoOut.prx astroport-exports.txt; do
  add_file "$B/fwd/$name" "lib/$name" 0644 "$engine_hint"
done
add_file "$anyps5/relinker/relinker" anyps5/relinker 0755 "$engine_hint"
add_file "$system_libs/libAstroportHost.prx" anyps5/libs/libAstroportHost.prx 0644 "$engine_hint"
system_count=0
for file in "$ROOT/$system_libs"/*.prx; do
  name=${file##*/}
  [[ $name == libAstroportHost.prx ]] && continue
  add_file "$system_libs/$name" "anyps5/libs/$name" 0644 "$engine_hint"
  system_count=$((system_count + 1))
done
(( system_count > 0 )) || fail "missing $system_libs/*.prx ($engine_hint)"

for name in u59-preset.json u59-preset.PPSA21567.json; do
  add_file "engines/kyty/tools/$name" "presets/$name" 0644 'restore repository presets'
done

# The renderer loads the driver's NGX itself (design 6.4), so DLSS ships when its runtime is here.
dlss=no
sdk_hint="run scripts/release/fetch-dlss.sh $B/release/dlss-sdk"
runtimes=("$ROOT/$sdk"/lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.*)
if (( ${#runtimes[@]} == 1 )) && [[ -f ${runtimes[0]} ]]; then
  dlss=yes
  runtime=${runtimes[0]}
  add_file "${runtime#"$ROOT/"}" "lib/${runtime##*/}" 0644 "$sdk_hint"
  add_file "$sdk/LICENSE.txt" licenses/NVIDIA-DLSS-SDK-LICENSE.txt 0644 "$sdk_hint"
elif (( ${#runtimes[@]} > 0 )) || [[ ${ASTROPORT_REQUIRE_DLSS:-} == 1 ]]; then
  fail "no single DLSS runtime in $sdk ($sdk_hint)"
fi

for file in LICENSE NOTICE.md; do
  add_file "$file" "licenses/$file" 0644 "$licence_hint"
done
for engine in kyty anyps5; do
  add_file "engines/$engine/LICENSE" "licenses/$engine/LICENSE" 0644 "$licence_hint"
  for file in "$ROOT/engines/$engine"/LICENSES/**/*; do
    [[ -f "$file" ]] || continue
    relpath=${file#"$ROOT/engines/"}
    add_file "engines/$relpath" "licenses/$relpath" 0644 "$licence_hint"
  done
  for file in "$ROOT/engines/$engine"/3rdparty/*/*; do
    [[ -f "$file" ]] || continue
    name=${file##*/}
    case "${name,,}" in
      licen[cs]e*|copying*|copyright*)
        relpath=${file#"$ROOT/engines/"}
        add_file "engines/$relpath" "licenses/$relpath" 0644 "$licence_hint"
        ;;
    esac
  done
done

for i in "${!sources[@]}"; do
  install -D -m "${modes[i]}" -- "$ROOT/${sources[i]}" "$stage/${destinations[i]}" 2>/dev/null \
    || fail "could not stage ${sources[i]} in $stage"
done
for file in "$stage"/bin/* "$stage"/lib/{libastro_patcher,libkyty_renderer,libastroport_bridge}.so; do
  strip --strip-unneeded -- "$file" 2>/dev/null || fail "could not strip $file"
done

printf 'staged %s: %s system libraries, DLSS %s\n' "$stage" "$system_count" "$dlss"
