# AstroPort releases

## Channels

Stable uses `vX.Y.Z` tags and GitHub releases. Testing uses `vX.Y.Z-testing.N` tags and GitHub pre-releases. The channel blocks in `ags-extension.toml` are:

```toml
[[channels]]
id = "stable"
name = "Stable"
tag = '^v\d+\.\d+\.\d+$'
prerelease = false
default = true

[[channels]]
id = "testing"
name = "Testing"
tag = '^v\d+\.\d+\.\d+-testing\.\d+$'
prerelease = true
```

## Cutting a release

The owner sets `version` in `ags-extension.toml`, commits it, and runs these commands from the repository root. Substitute the manifest version for `<version>`.

```sh
git tag v<version>
scripts/build-release.sh --check
scripts/build-release.sh
```

The build needs a reachable Podman or Docker engine. Set `ASTROPORT_CONTAINER` to `podman` or `docker` to choose one. `ASTROPORT_CONTAINER_ARGS` adds options to the build container's `run`, for example `--memory 9g --cpuset-cpus 0-5` to cap it on a shared machine. By default the script uses Podman if it is on `PATH` and Docker otherwise. The archive and checksum are in `build/release/out/v<version>/`.

Only the owner publishes the tag and release. Use `--prerelease` for a testing version.

```sh
git push origin v<version>
gh release create v<version> [--prerelease] --notes-file <notes> \
  build/release/out/v<version>/astroport-<version>-linux-x86_64.tar.zst \
  build/release/out/v<version>/astroport-<version>-linux-x86_64.tar.zst.sha256
```

## Before the first DLSS release

- [ ] The owner reads `build/release/dlss-sdk/LICENSE.txt` and confirms it permits redistribution of `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.310.9.16`. If the licence does not permit redistribution, the release is blocked until the owner decides how to proceed.

## DLSS SDK

The release build fetches the SDK from `https://github.com/NVIDIA/DLSS` at tag `v310.9.16`, commit `9981c6350dc58c0ce21f024fd65e31e851c31689`. Keep the SDK in the ignored `build/release/dlss-sdk` directory. To update the pin, change the URL, tag, and commit together in `scripts/release/dlss.env`.

## Source build

In a fresh checkout, the `[install.source]` commands build and stage AstroPort without DLSS. Run them from the repository root and substitute your staging directory for `{stage}`.

```sh
scripts/build-engines.sh
cargo build --release --locked --manifest-path patcher/Cargo.toml
cargo build --release --locked --manifest-path adapter/Cargo.toml
cargo build --release --locked --manifest-path runner/Cargo.toml
scripts/release/stage.sh "{stage}"
```

For a manual source build with DLSS, fetch the SDK and seed Kyty's CMake cache from the repository root, then run the build and stage commands above.

```sh
scripts/release/fetch-dlss.sh build/release/dlss-sdk
cmake -S engines/kyty -B build/kyty -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo \
  -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC \
  -DKYTY_NGX_SDK_DIR="$PWD/build/release/dlss-sdk"
```

## Release notes template

For a testing version, use the title `# AstroPort <version> (testing)`.

```markdown
# AstroPort <version>

Needs AGS with extension protocol 2 and extension jobs (AGS milestone 3 or later: the game files are built by an extension job and launched from `generated/`). Supported game versions: 1.019 (PPSA21567), and 1.018 (less tested).

- **Changes:** ...
- **Known issues:** ...
- **After installing, updating or rolling back:** build the game files again from the game page; AGS refuses to launch game files built before the install.
- **DLSS:** this package contains NVIDIA's DLSS runtime (SDK 310.9.16) under NVIDIA's DLSS SDK licence, which is not compatible with the GPL. DLSS works on NVIDIA GPUs only. A source build has no DLSS unless you fetch the SDK yourself.
- **Source:** <repository URL>/tree/v<version> (GPL-2.0-only). Licences are in `licenses/` inside the package.
- **Checksum:** astroport-<version>-linux-x86_64.tar.zst.sha256
```
