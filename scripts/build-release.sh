#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
cd "$ROOT"

fail() {
  local message=$*
  printf 'FAIL: %s\n' "${message//$'\n'/\\n}" >&2
  exit 1
}

[[ $# -eq 0 || ( $# -eq 1 && $1 == --check ) ]] ||
  fail 'usage: scripts/build-release.sh [--check]'

[[ -f ags-extension.toml ]] || fail 'ags-extension.toml not found (step 3)'
version=$(sed -n '/^[[:space:]]*\[/q; s/^version = "\([^"]*\)"$/\1/p' ags-extension.toml 2>/dev/null) ||
  fail 'could not read ags-extension.toml'
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-testing\.[0-9]+)?$ ]] ||
  fail "version \"$version\" is neither X.Y.Z nor X.Y.Z-testing.N"
channel=stable
if [[ $version == *-testing.* ]]; then
  channel=testing
fi

[[ -f adapter/Cargo.toml ]] || fail 'adapter/Cargo.toml not found: the release needs the astroport-adapter crate (step 3)'
[[ -f adapter/Cargo.lock ]] || fail 'adapter/Cargo.lock not found'
[[ -f runner/Cargo.toml ]] || fail 'runner/Cargo.toml not found: the release needs the astroport-run crate (step 4)'
[[ -f runner/Cargo.lock ]] || fail 'runner/Cargo.lock not found'

changes=$(git status --porcelain --untracked-files=all 2>/dev/null) || fail 'could not check the working tree'
[[ -z $changes ]] || fail 'the working tree has uncommitted changes; a release is built from a commit'
tag="v$version"
tags=$(git tag --points-at HEAD 2>/dev/null) || fail 'could not list tags at HEAD'
grep -qxF -- "$tag" <<<"$tags" ||
  fail "HEAD is not tagged $tag; tag it first (git tag $tag), the release links that tag"
commit=$(git rev-parse HEAD 2>/dev/null) || fail 'could not read HEAD'
asset="astroport-$version-linux-x86_64.tar.zst"
printf 'release %s: channel %s, asset %s, commit %s\n' "$tag" "$channel" "$asset" "$commit"
if [[ ${1:-} == --check ]]; then
  exit 0
fi

engine=${ASTROPORT_CONTAINER:-}
if [[ -z $engine ]]; then
  if command -v podman >/dev/null 2>&1; then
    engine=podman
  else
    engine=docker
  fi
fi
case $engine in
  podman) user_args=(--userns=keep-id) ;;
  docker) user_args=(--user "$(id -u):$(id -g)") ;;
  *) fail 'ASTROPORT_CONTAINER must be podman or docker' ;;
esac
"$engine" info >/dev/null 2>&1 ||
  fail "container engine $engine is not reachable; start it yourself, this script never starts services"

release="$ROOT/build/release"
src="$release/src"
out="$release/out/$tag"
[[ ! -L "$ROOT/build" && ! -L $release ]] || fail 'build and build/release must not be symlinks'
for file in "$asset" "$asset.sha256"; do
  [[ ! -e "$out/$file" && ! -L "$out/$file" ]] || fail "$out/$file already exists"
done
scripts/release/fetch-dlss.sh build/release/dlss-sdk || exit 1
rm -rf -- "$src" 2>/dev/null || fail "could not remove $src"
mkdir -p -- "$src/build/release/dlss-sdk" "$out" 2>/dev/null || fail 'could not create release directories'
{ git archive HEAD | tar -x -C "$src"; } 2>/dev/null || fail "could not export HEAD to $src"

"$engine" build -t astroport-release -f scripts/release/Containerfile scripts/release ||
  fail "container image build failed with $engine"
epoch=$(git log -1 --format=%ct 2>/dev/null) || fail 'could not read the commit timestamp'
"$engine" run --rm "${user_args[@]}" \
  -v "$src:/work/src" \
  -v "$release/dlss-sdk:/work/src/build/release/dlss-sdk:ro" \
  -v "$out:/work/out" \
  -e "SOURCE_DATE_EPOCH=$epoch" -w /work/src \
  astroport-release scripts/release/inside.sh || fail "container release build failed with $engine"

(cd "$out" && sha256sum -c "$asset.sha256") || fail "checksum verification failed for $asset"
printf '%s\n' "$out/$asset" "$out/$asset.sha256"
