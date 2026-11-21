#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/../.." && pwd)"
cd "$ROOT"

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

export HOME=/tmp/astroport-home CARGO_HOME=/tmp/astroport-cargo
mkdir -p -- "$HOME" "$CARGO_HOME" 2>/dev/null || fail 'could not create container home directories'

# Match the Kyty configuration in build-engines.sh so CMake keeps the SDK path.
cmake -S engines/kyty -B build/kyty -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC \
  -DKYTY_NGX_SDK_DIR=/work/src/build/release/dlss-sdk || fail 'Kyty CMake configuration failed'
scripts/build-engines.sh || fail 'engine build failed'
cargo build --release --locked --manifest-path patcher/Cargo.toml || fail 'patcher build failed'
cargo build --release --locked --manifest-path adapter/Cargo.toml || fail 'astroport-adapter build failed'
cargo build --release --locked --manifest-path runner/Cargo.toml || fail 'astroport-run build failed'
ASTROPORT_REQUIRE_DLSS=1 scripts/release/stage.sh build/release/stage
scripts/release/package.sh build/release/stage /work/out
