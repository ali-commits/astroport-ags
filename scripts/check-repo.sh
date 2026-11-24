#!/usr/bin/env bash
# Repository rules. No arguments; works from any cwd. Exit 0 = "repo OK", else one FAIL line per broken rule.
set -u
ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"
G=(git -C "$ROOT")
fail=0
bad() { echo "FAIL: $*"; fail=1; }

total=0 max=0 maxf="" n=0
while IFS= read -r -d '' f; do
  n=$((n+1))
  s=$(stat -c %s "$ROOT/$f" 2>/dev/null || echo 0)
  total=$((total+s))
  (( s > max )) && { max=$s; maxf=$f; }
  (( s > 52428800 )) && bad "file over 50 MiB: $f ($s bytes)"
  case "$f" in
    *.prx|*.sprx|*.elf|eboot.bin|*/eboot.bin|*.dll|*.exe|*.lib|*.profdata|*.log|src-*|run-*|ngx-sdk/*|build/*|*nvsdk_ngx*|*libnvidia-ngx-*)
      bad "forbidden tracked path: $f" ;;
  esac
done < <("${G[@]}" ls-files -z)
(( total >= 314572800 )) && bad "tracked total $((total/1048576)) MiB is not under 300 MiB"

"${G[@]}" ls-files -s | awk '$1=="160000"{print $4}' | while read -r p; do echo "FAIL: gitlink: $p"; done | grep . && fail=1
"${G[@]}" ls-files | grep -E '(^|/)\.gitmodules$' | while read -r p; do echo "FAIL: tracked .gitmodules: $p"; done | grep . && fail=1

hits=$("${G[@]}" grep -I -l -F "$HOME" 2>/dev/null)
[ -n "$hits" ] && bad "files contain \$HOME: $(echo $hits)"

for d in "$ROOT"/engines/*/3rdparty/*; do
  [ -d "$d" ] && [ ! -L "$d" ] || continue
  case "$(basename "$d")" in patches|renderdoc) continue ;; esac
  [ -n "$(find "$d" -maxdepth 1 \( -iname 'licen[cs]e*' -o -iname 'copying*' -o -iname 'copyright*' \) -print -quit)" ] \
    || bad "no licence file in ${d#$ROOT/}"
done

tracked=$("${G[@]}" ls-files)
for f in .gitignore LICENSE README.md NOTICE.md engines/kyty/LICENSE engines/kyty/LICENSES/Kyty-MIT.txt engines/kyty/UPSTREAM.md \
  engines/anyps5/LICENSE engines/anyps5/UPSTREAM.md patcher/Cargo.toml patches/PPSA21567-1018.json \
  patches/PPSA21567-1019.json scripts/play.sh scripts/prx-targets.txt tools/hidtap/Cargo.toml \
  scripts/build-release.sh scripts/release/dlss.env scripts/release/fetch-dlss.sh scripts/release/stage.sh \
  scripts/release/package.sh scripts/release/inside.sh scripts/release/Containerfile scripts/release/test.sh docs/release.md \
  ags-extension.toml adapter/Cargo.toml adapter/Cargo.lock adapter/src/main.rs patches/settings.json patches/versions.json \
  assets/icon.svg assets/logo.svg assets/icon.png assets/logo.png assets/badge.png assets/badge.svg runner/Cargo.toml runner/Cargo.lock \
  engines/kyty/src/streaming/transport.cpp patcher/src/lib.rs; do
  grep -qxF "$f" <<<"$tracked" || bad "missing tracked file: $f"
done

for p in build/x run-hybrid-1019/x src-1019/x ngx-sdk/x last-hybrid.log patcher/target/x tools/hidtap/target/x tools/__pycache__/x.pyc adapter/target/x runner/target/x build/release/dlss-sdk/x; do
  "${G[@]}" check-ignore -q --no-index "$p" || bad "should be ignored: $p"
done
for p in engines/anyps5/3rdparty/zstd/build/cmake/CMakeLists.txt engines/kyty/src/graphics/host_gpu/renderer/cache/textureCache.cpp patcher/src/lib.rs scripts/check-repo.sh adapter/src/main.rs runner/src/main.rs ags-extension.toml assets/icon.svg; do
  "${G[@]}" check-ignore -q --no-index "$p" && bad "should not be ignored: $p"
done

(( fail )) && exit 1
echo "repo OK: $((total/1048576)) MiB in $n files, largest $maxf ($((max/1048576)) MiB)"
