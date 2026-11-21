#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.." && pwd)"
passed=0
failed=0

ok() {
  printf 'ok %s\n' "$1"
  passed=$((passed + 1))
}

bad() {
  printf 'FAIL: %s: %s\n' "$1" "$2" >&2
  failed=$((failed + 1))
}

expect_fail() {
  local case_name=$1 message=$2
  shift 2
  if "$@" >"$T/stdout" 2>"$T/stderr"; then
    bad "$case_name" 'command succeeded'
    return 1
  fi
  if ! grep -qF -- "$message" "$T/stderr"; then
    bad "$case_name" "expected stderr to contain '$message'; got $(head -n 1 "$T/stderr")"
    return 1
  fi
}

fetch_tests() {
  local origin="$T/dlss-origin" sdk="$T/sdk" fetch="$ROOT/scripts/release/fetch-dlss.sh"
  local runtime=lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16
  local commit ready file
  local -a files=(include/nvsdk_ngx_vk.h lib/Linux_x86_64/libnvsdk_ngx.a "$runtime" LICENSE.txt)

  for file in "${files[@]}" lib/Windows_x86_64/nvsdk_ngx_d.lib doc/big.pdf; do
    mkdir -p -- "$(dirname "$origin/$file")"
    printf 'fixture\n' >"$origin/$file"
  done
  git init -q -- "$origin"
  git -C "$origin" add .
  git -C "$origin" -c core.hooksPath=/dev/null -c user.name=t -c user.email=t@example.invalid \
    commit --quiet --no-gpg-sign -m 'SDK fixture'
  git -C "$origin" tag --no-sign v1.0.0
  git -C "$origin" config uploadpack.allowFilter true
  commit=$(git -C "$origin" rev-parse HEAD)
  ready="DLSS SDK v1.0.0 ready at $sdk"

  local -a sdk_env=(env "DLSS_SDK_URL=file://$origin" DLSS_SDK_TAG=v1.0.0 "DLSS_SDK_COMMIT=$commit")
  if ! "${sdk_env[@]}" "$fetch" "$sdk" >"$T/stdout" 2>"$T/stderr"; then
    bad 'fetch: fresh' "$(head -n 1 "$T/stderr")"
  elif [[ $(tail -n 1 "$T/stdout") != "$ready" ]]; then
    bad 'fetch: fresh' 'unexpected ready line'
  elif [[ $(git -C "$sdk" rev-parse HEAD) != "$commit" ]]; then
    bad 'fetch: fresh' 'checkout is at the wrong commit'
  elif [[ ! -f "$sdk/${files[0]}" || ! -f "$sdk/${files[1]}" || ! -f "$sdk/$runtime" ||
          ! -f "$sdk/LICENSE.txt" || -e "$sdk/lib/Windows_x86_64" || -e "$sdk/doc" ]]; then
    bad 'fetch: fresh' 'unexpected sparse checkout contents'
  else
    ok 'fetch: fresh'
  fi

  if ! "${sdk_env[@]}" DLSS_SDK_URL=file:///nonexistent "$fetch" "$sdk" >"$T/stdout" 2>"$T/stderr"; then
    bad 'fetch: reuse' "$(head -n 1 "$T/stderr")"
  elif [[ $(tail -n 1 "$T/stdout") != "$ready" ]]; then
    bad 'fetch: reuse' 'unexpected ready line'
  else
    ok 'fetch: reuse'
  fi

  if expect_fail 'fetch: commit mismatch' 'does not match the pinned commit' \
      "${sdk_env[@]}" DLSS_SDK_COMMIT=0000000000000000000000000000000000000000 "$fetch" "$T/mismatch"; then
    if [[ -e "$T/mismatch" ]] || compgen -G "$T/*.partial.*" >/dev/null; then
      bad 'fetch: commit mismatch' 'destination or partial checkout remains'
    else
      ok 'fetch: commit mismatch'
    fi
  fi

  local other="$T/other"
  git init -q -- "$other"
  git -C "$other" -c core.hooksPath=/dev/null -c user.name=t -c user.email=t@example.invalid \
    commit --quiet --no-gpg-sign --allow-empty -m 'Unrelated commit'
  printf 'keep\n' >"$other/marker"
  if expect_fail 'fetch: other commit' 'holds another DLSS SDK commit' "${sdk_env[@]}" "$fetch" "$other"; then
    if [[ -f "$other/marker" ]]; then
      ok 'fetch: other commit'
    else
      bad 'fetch: other commit' 'marker was removed'
    fi
  fi
}

stage_tests() {
  local root="$T/root" plain="$T/plain" dlss="$T/dlss"
  local stage="$root/scripts/release/stage.sh"
  local renderer="$root/build/kyty-gpu/libkyty_gpu.so"
  local adapter="$root/adapter/target/release/astroport-adapter"
  local runner="$root/runner/target/release/astroport-run"
  local sdk="$root/build/release/dlss-sdk"
  local runtime=libnvidia-ngx-dlss.so.310.9.16
  local file layout_error='' licence_error='' plain_error sections
  local -a system_modules
  local -a layout=(
    ags-extension.toml bin/astroport-adapter bin/astroport-run
    lib/libastro_patcher.so lib/libkyty_gpu.so
    anyps5/relinker anyps5/libs/libSystemA.prx anyps5/libs/libSystemB.prx
    anyps5/libAstroHybridHost.prx
    shims/libSceAgc.prx shims/libSceAgcDriver.prx shims/libSceVideoOut.prx
    presets/u59-preset.json presets/u59-preset.PPSA21567.json
    patches/PPSA21567-1019.json patches/settings.json patches/versions.json assets/icon.png
  )
  local -a licences=(
    LICENSE NOTICE.md kyty/LICENSE kyty/LICENSES/Kyty-MIT.txt
    anyps5/LICENSE anyps5/3rdparty/zstd/COPYING
  )

  mkdir -p -- "$(dirname "$stage")"
  if [[ -f "$ROOT/scripts/release/stage.sh" ]]; then
    install -m 0755 -- "$ROOT/scripts/release/stage.sh" "$stage"
  fi
  for file in ags-extension.toml assets/icon.png patches/{PPSA21567-1019,settings,versions}.json \
      build/anyps5/core/libs/libs/{libSystemA,libSystemB,libAstroHybridHost}.prx \
      build/shims/{libSceAgcDriver,libSceVideoOut}.prx \
      engines/kyty/tools/{u59-preset,u59-preset.PPSA21567}.json \
      engines/kyty/{LICENSE,LICENSES/Kyty-MIT.txt} \
      engines/anyps5/{LICENSE,3rdparty/zstd/COPYING} LICENSE NOTICE.md; do
    mkdir -p -- "$(dirname "$root/$file")"
    printf 'fixture\n' >"$root/$file"
  done
  printf 'linked shim fixture\n' >"$root/linked-shim"
  ln -s ../../linked-shim "$root/build/shims/libSceAgc.prx"
  mkdir -p -- "$(dirname "$renderer")" "$(dirname "$adapter")" "$(dirname "$runner")" \
    "$root/patcher/target/release" "$root/build/anyps5/core/relinker"
  printf '#!/usr/bin/env bash\nexit 0\n' >"$root/build/anyps5/core/relinker/relinker"
  chmod 0755 "$root/build/anyps5/core/relinker/relinker"

  printf 'void fixture(void) {}\n' >"$T/plain.c"
  printf 'void NVSDK_NGX_VULKAN_Init_with_ProjectID(void) {}\n' >"$T/ngx.c"
  printf 'int main(void) { return 0; }\n' >"$T/main.c"
  TMPDIR="$T" timeout --foreground -k 2 30 cc -g -shared -o "$renderer" "$T/plain.c"
  TMPDIR="$T" timeout --foreground -k 2 30 cc -g -shared -o "$T/ngx.so" "$T/ngx.c"
  TMPDIR="$T" timeout --foreground -k 2 30 cc -g -shared -o "$root/patcher/target/release/libastro_patcher.so" "$T/plain.c"
  TMPDIR="$T" timeout --foreground -k 2 30 cc -g -o "$adapter" "$T/main.c"
  TMPDIR="$T" timeout --foreground -k 2 30 cc -g -o "$runner" "$T/main.c"
  chmod 0755 "$adapter" "$runner"

  if ! env ASTROPORT_REQUIRE_DLSS=0 "$stage" "$plain" >"$T/stdout" 2>"$T/stderr"; then
    plain_error=$(head -n 1 "$T/stderr")
    for file in layout 'no dlss' 'stripped output' 'links copied' modes; do
      bad "stage: $file" "$plain_error"
    done
  else
    for file in "${layout[@]}"; do
      if [[ ! -f "$plain/$file" ]]; then
        layout_error="missing $file"
        break
      fi
    done
    system_modules=("$plain/anyps5/libs/"*.prx)
    if [[ -n $layout_error ]]; then
      bad 'stage: layout' "$layout_error"
    elif (( ${#system_modules[@]} != 2 )) ||
        [[ -e "$plain/anyps5/libs/libAstroHybridHost.prx" ]] ||
        [[ $(tail -n 1 "$T/stdout") != "staged $plain: 2 system libraries, DLSS no" ]]; then
      bad 'stage: layout' 'unexpected system libraries or summary'
    else
      ok 'stage: layout'
    fi

    for file in "${licences[@]}"; do
      if [[ ! -f "$plain/licenses/$file" ]]; then
        licence_error="missing licenses/$file"
        break
      fi
    done
    if [[ -n $licence_error ]]; then
      bad 'stage: no dlss' "$licence_error"
    elif compgen -G "$plain/lib/libnvidia-ngx-dlss.so.*" >/dev/null ||
        [[ -e "$plain/licenses/NVIDIA-DLSS-SDK-LICENSE.txt" ]]; then
      bad 'stage: no dlss' 'unexpected DLSS files'
    else
      ok 'stage: no dlss'
    fi

    if sections=$(readelf -S -- "$plain/lib/libkyty_gpu.so" "$plain/bin/astroport-adapter" 2>"$T/stderr") &&
        [[ $sections != *'.debug_info'* ]]; then
      ok 'stage: stripped output'
    else
      bad 'stage: stripped output' 'staged ELF files are unreadable or contain .debug_info'
    fi
    if [[ -z $(find "$plain" -type l -print -quit) && -f "$plain/shims/libSceAgc.prx" ]] &&
        cmp -s -- "$root/linked-shim" "$plain/shims/libSceAgc.prx"; then
      ok 'stage: links copied'
    else
      bad 'stage: links copied' 'unexpected symlink or shim content'
    fi
    if [[ $(stat -c %a -- "$plain/bin/astroport-adapter" "$plain/bin/astroport-run" "$plain/anyps5/relinker") == $'755\n755\n755' &&
        $(stat -c %a -- "$plain/lib/libkyty_gpu.so" "$plain/patches/"*.json) == $'644\n644\n644\n644' ]]; then
      ok 'stage: modes'
    else
      bad 'stage: modes' 'unexpected executable or data file mode'
    fi
  fi

  cp -- "$renderer" "$T/plain.so"
  cp -- "$T/ngx.so" "$renderer"
  mkdir -p -- "$sdk/lib/Linux_x86_64/rel"
  printf 'DLSS runtime fixture\n' >"$sdk/lib/Linux_x86_64/rel/$runtime"
  printf 'DLSS licence fixture\n' >"$sdk/LICENSE.txt"
  mkdir -- "$dlss"
  if ! env ASTROPORT_REQUIRE_DLSS=1 "$stage" "$dlss" >"$T/stdout" 2>"$T/stderr"; then
    bad 'stage: dlss' "$(head -n 1 "$T/stderr")"
  elif [[ -f "$dlss/lib/$runtime" && -f "$dlss/licenses/NVIDIA-DLSS-SDK-LICENSE.txt" &&
        $(tail -n 1 "$T/stdout") == *'DLSS yes' ]]; then
    ok 'stage: dlss'
  else
    bad 'stage: dlss' 'missing DLSS files or summary'
  fi
  cp -- "$T/plain.so" "$renderer"
  if expect_fail 'stage: require dlss fails' 'renderer was built without DLSS' \
      env ASTROPORT_REQUIRE_DLSS=1 "$stage" "$T/require-dlss"; then
    ok 'stage: require dlss fails'
  fi

  strip --strip-all -- "$renderer"
  if expect_fail 'stage: stripped input' 'has no symbol table' \
      env ASTROPORT_REQUIRE_DLSS=0 "$stage" "$T/stripped"; then
    ok 'stage: stripped input'
  fi
  cp -- "$T/plain.so" "$renderer"

  mv -- "$runner" "$T/runner"
  if expect_fail 'stage: missing runner' 'missing runner/target/release/astroport-run (build the astroport-run crate in runner/, step 4)' \
      env ASTROPORT_REQUIRE_DLSS=0 "$stage" "$T/missing-runner"; then
    ok 'stage: missing runner'
  fi
  mv -- "$T/runner" "$runner"
  mv -- "$adapter" "$T/adapter"
  if expect_fail 'stage: missing adapter' 'missing adapter/target/release/astroport-adapter (build the astroport-adapter crate in adapter/, step 3)' \
      env ASTROPORT_REQUIRE_DLSS=0 "$stage" "$T/missing-adapter"; then
    ok 'stage: missing adapter'
  fi
  mv -- "$T/adapter" "$adapter"

  mkdir -- "$T/not-empty"
  touch -- "$T/not-empty/marker"
  if expect_fail 'stage: not empty' 'is not empty' \
      env ASTROPORT_REQUIRE_DLSS=0 "$stage" "$T/not-empty"; then
    ok 'stage: not empty'
  fi
}

case ${1:-} in
  ''|fetch|stage) ;;
  *) printf 'FAIL: unknown release test group %s\n' "$1" >&2; exit 1 ;;
esac

mkdir -p -- "$ROOT/build"
T=$(mktemp -d "$ROOT/build/release-test.XXXXXXXX")
trap 'rm -rf -- "$T"' EXIT
case ${1:-} in
  fetch) fetch_tests ;;
  stage) stage_tests ;;
  '') fetch_tests; stage_tests ;;
esac

if (( failed > 0 )); then
  exit 1
fi
printf 'release tests OK: %s\n' "$passed"
