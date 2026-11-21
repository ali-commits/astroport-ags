#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.." && pwd)"
source "$ROOT/scripts/release/dlss.env"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

[[ $# -eq 1 && -n $1 ]] || fail 'usage: fetch-dlss.sh <dest>'
dest=$1

ready() {
  printf 'DLSS SDK %s ready at %s\n' "$DLSS_SDK_TAG" "$dest"
}

if [[ -e "$dest" || -L "$dest" ]]; then
  # The empty prefix confirms Git found the checkout at dest itself.
  if [[ $(git -C "$dest" rev-parse --show-prefix HEAD 2>/dev/null) == $'\n'"$DLSS_SDK_COMMIT" ]]; then
    ready
    exit 0
  fi
  fail "$dest holds another DLSS SDK commit; remove it and run again"
fi

partial="$dest.partial.$$"
mkdir -p -- "$(dirname -- "$dest")" 2>/dev/null || fail "could not create the parent directory for $dest"
mkdir -- "$partial" 2>/dev/null || fail "could not create $partial"
trap 'rm -rf -- "$partial"' EXIT
trap 'exit 1' HUP INT TERM

git clone --quiet --depth 1 --branch "$DLSS_SDK_TAG" --filter=blob:none --no-checkout \
  -- "$DLSS_SDK_URL" "$partial" 2>/dev/null || fail "could not clone DLSS SDK tag $DLSS_SDK_TAG"
git -C "$partial" sparse-checkout set --no-cone /include/ /lib/Linux_x86_64/ /LICENSE.txt 2>/dev/null \
  || fail 'could not set the DLSS SDK sparse checkout'
git -C "$partial" checkout --quiet 2>/dev/null || fail "could not check out DLSS SDK tag $DLSS_SDK_TAG"

commit=$(git -C "$partial" rev-parse HEAD 2>/dev/null) || fail 'could not read the DLSS SDK commit'
[[ $commit == "$DLSS_SDK_COMMIT" ]] \
  || fail "tag $DLSS_SDK_TAG is at $commit, which does not match the pinned commit $DLSS_SDK_COMMIT"

for file in include/nvsdk_ngx_vk.h lib/Linux_x86_64/libnvsdk_ngx.a LICENSE.txt; do
  [[ -f "$partial/$file" ]] || fail "DLSS SDK is missing $file"
done

shopt -s nullglob
runtimes=("$partial"/lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.*)
runtime_found=false
for runtime in "${runtimes[@]}"; do
  if [[ -f "$runtime" ]]; then
    runtime_found=true
    break
  fi
done
"$runtime_found" || fail 'DLSS SDK is missing lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.*'

# Keep a destination created by another fetch intact.
if ! mv -T --no-clobber -- "$partial" "$dest" 2>/dev/null || [[ -e "$partial" ]]; then
  fail "could not move the DLSS SDK checkout to $dest"
fi
ready
