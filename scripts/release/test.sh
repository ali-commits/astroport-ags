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

case ${1:-} in
  ''|fetch) ;;
  *) printf 'FAIL: unknown release test group %s\n' "$1" >&2; exit 1 ;;
esac

mkdir -p -- "$ROOT/build"
T=$(mktemp -d "$ROOT/build/release-test.XXXXXXXX")
trap 'rm -rf -- "$T"' EXIT
fetch_tests

if (( failed > 0 )); then
  exit 1
fi
printf 'release tests OK: %s\n' "$passed"
