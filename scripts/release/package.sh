#!/usr/bin/env bash
set -euo pipefail

script=$(readlink -f "${BASH_SOURCE[0]}")
ROOT="$(cd "$(dirname "$script")/../.." && pwd)"

fail() {
  local message=$*
  printf 'FAIL: %s\n' "${message//$'\n'/\\n}" >&2
  exit 1
}

[[ $# -eq 2 && -n $1 && -n $2 ]] || fail "usage: ${script#"$ROOT/"} <stage-dir> <out-dir>"
root=$(realpath -e -- "$1" 2>/dev/null) && [[ -d $root ]] || fail "missing stage directory $1"
arch=$(uname -m)
[[ $arch == x86_64 ]] || fail "unsupported architecture $arch"

required=(
  ags-extension.toml bin/astroport-adapter bin/astroport-run
  lib/libastro_patcher.so lib/libkyty_gpu.so anyps5/relinker anyps5/libAstroHybridHost.prx
  shims/libSceAgc.prx shims/libSceAgcDriver.prx shims/libSceVideoOut.prx
  presets/u59-preset.json presets/u59-preset.PPSA21567.json
  patches/PPSA21567-1018.json patches/PPSA21567-1019.json patches/settings.json patches/versions.json
  licenses/LICENSE licenses/NOTICE.md
)
for path in "${required[@]}"; do
  [[ -f "$root/$path" ]] || fail "missing $path"
done

shopt -s nullglob
has_system_library=false
for path in "$root"/anyps5/libs/*.prx; do
  if [[ -f $path ]]; then
    has_system_library=true
    break
  fi
done
"$has_system_library" || fail 'anyps5/libs holds no .prx'

special=$(find "$root" ! -type f ! -type d ! -type l -printf '%P/' -quit 2>/dev/null) \
  || fail 'could not scan the stage for special files'
[[ -z $special ]] || fail "special files in the package: ${special%/}"

out=$(realpath -m -- "$2" 2>/dev/null) || fail "could not resolve output directory $2"
[[ "$out/" != "$root/"* ]] || fail 'output directory is inside the stage'
mkdir -p -- "$out" 2>/dev/null || fail "could not create output directory $out"
work=$(mktemp -d -- "$out/.astroport-package.XXXXXXXX" 2>/dev/null) || fail 'mktemp failed'
trap 'rm -rf -- "$work"' EXIT

find "$root" -type l -print0 >"$work/links" 2>/dev/null || fail 'could not scan the stage for links'
while IFS= read -r -d '' link; do
  target=$(readlink -- "$link" 2>/dev/null) || fail "could not read link ${link#"$root/"}"
  [[ $target != /* ]] || fail "absolute link ${link#"$root/"} -> $target"
  real=$(realpath -e -- "$link" 2>/dev/null) || fail "dangling or looping link ${link#"$root/"}"
  [[ "$real/" == "$root/"* ]] || fail "link escapes the package: ${link#"$root/"} -> $target"
done <"$work/links"

manifest="$root/ags-extension.toml"
field() {
  sed -n "/^[[:space:]]*\[/q; s/^$1 = $2\$/\1/p" "$manifest"
}
quoted='"\([^"]*\)"'
id=$(field id "$quoted")
version=$(field version "$quoted")
protocol=$(field protocol '\([0-9]*\)[[:blank:]]*\(#.*\)\?')
adapter=$(field adapter "$quoted")
[[ $id == astroport ]] || fail "manifest id is \"$id\", not \"astroport\""
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+(-testing\.[0-9]+)?$ ]] \
  || fail "version \"$version\" is neither X.Y.Z nor X.Y.Z-testing.N"
[[ $protocol == 2 ]] || fail "manifest protocol is \"$protocol\", not 2 (AGS installs protocol 2 releases only)"

relative_path() {
  [[ -n $1 && "/$1/" != *'//'* && "/$1/" != *'/./'* && "/$1/" != *'/../'* ]]
}

for path in "$adapter" bin/astroport-adapter bin/astroport-run anyps5/relinker; do
  if ! relative_path "$path" || [[ ! -f "$root/$path" || ! -x "$root/$path" || -L "$root/$path" ]]; then
    fail "$path is not an executable file"
  fi
done
for key in icon logo badge background; do
  path=$(sed -n "/^\[assets\]\([[:blank:]]*#.*\)\?[[:blank:]]*\$/,/^[[:space:]]*\[/s/^$key = $quoted\$/\1/p" "$manifest")
  [[ $key == background && -z $path ]] && continue
  if ! relative_path "$path" || [[ ! -f "$root/$path" || -L "$root/$path" ]]; then
    fail "asset $key ($path) is not a regular file"
  fi
  size=$(stat -c %s -- "$root/$path" 2>/dev/null) || fail "could not determine size of asset $path"
  (( size <= 2097152 )) || fail "asset $path is larger than 2 MiB"
done

asset="astroport-$version-linux-$arch.tar.zst"
for file in "$asset" "$asset.sha256"; do
  [[ ! -e "$out/$file" && ! -L "$out/$file" ]] || fail "$out/$file already exists"
done

epoch=${SOURCE_DATE_EPOCH:-0}
[[ $epoch =~ ^-?[0-9]+$ ]] || fail "invalid SOURCE_DATE_EPOCH $epoch"
if ! {
  LC_ALL=C tar --sort=name --format=gnu --owner=0 --group=0 --numeric-owner \
    --mode=u+rwX,go+rX,go-w --mtime="@$epoch" -C "$root" -cf - . |
    zstd -19 -T0 -q -o "$work/$asset"
} 2>/dev/null; then
  fail 'could not create package archive'
fi
(cd "$work" && sha256sum -- "$asset" >"$asset.sha256") 2>/dev/null || fail 'could not write checksum'

for file in "$asset" "$asset.sha256"; do
  mv -nT -- "$work/$file" "$out/$file" 2>/dev/null || fail "could not move $file to $out"
  # GNU mv -n can report success when it skips a destination created by another run.
  [[ ! -e "$work/$file" ]] || fail "$out/$file already exists"
done
printf '%s\n' "$out/$asset" "$out/$asset.sha256"
