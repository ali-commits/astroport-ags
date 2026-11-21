# AstroPort

An unofficial native Linux port of Astro Bot (PS5, PPSA21567). The game's own code runs natively on
Linux through AnyPS5, and Kyty's renderer, built as a shared library, draws it through Vulkan.

This is a fan project. It is not affiliated with, endorsed by, or supported by Sony Interactive Entertainment
or Team Asobi. "Astro Bot" and "PlayStation" are trademarks of their owners.

**No game files are included.** You need your own legally obtained copy of the game. This repository
contains no game code or assets, no PlayStation firmware, and no keys.

## Status

Works on the author's PC (Linux, NVIDIA, 4K with frame generation): game version 1.019, about 30 FPS in the
heaviest level before frame generation, DualSense haptics and adaptive triggers.

The AGS install is coming. Until then `scripts/play-hybrid.sh` is a developer script: it starts an existing run
folder (`RUN_DIR`), and the step that turns a game dump into that folder is not automated here yet.

## Layout

| Path | What it is |
|---|---|
| `engines/kyty/` | Kyty renderer with the hybrid bridge (KytyPS5 fork). See `UPSTREAM.md`. |
| `engines/anyps5/` | AnyPS5 with the hybrid changes. See `UPSTREAM.md`. |
| `patcher/` | Rust library that applies KytyPS5-style memory patches (`patches/*.json`) to the running game. |
| `tools/hidtap/` | Rust diagnostic library that records the haptic samples sent to a DualSense over Bluetooth. |
| `tools/` | Helpers for porting patch files between game versions and for recording controller audio. |
| `patches/` | Patch definitions for game versions 1.018 and 1.019 (performance, physics at 30 FPS, fixes). |
| `adapter/` | Rust AGS extension (`ags-extension.toml`): the AstroPort pages, settings and launch. |
| `runner/` | `astroport-run`, the AGS build job that turns a game dump into a run folder. |
| `assets/` | Icon, logo and badge for the AGS extension. |
| `ags-extension.toml` | AGS extension manifest. |
| `scripts/` | `build-engines.sh`, `play-hybrid.sh`, `check-repo.sh` and `prx-targets.txt`. |

## Install through AGS

Add this repository's URL in AGS, set "Astro Bot dump folder", and press "Build game files". Supported game
versions are 1.019 and 1.018 (less tested, matched by version only).

## Building

```bash
scripts/build-engines.sh
```

This needs network access (Kyty's configure step downloads opus, OpenXR-SDK, xbyak, zydis, zstd, ZArchive and
Noto Sans), CMake, clang, gcc, Ninja, Python 3 and `glslangValidator`. Output goes to `build/`. The two Rust
parts need cargo:

```bash
cargo build --release --manifest-path patcher/Cargo.toml
```

```bash
cargo build --release --manifest-path tools/hidtap/Cargo.toml
```

Releases are built with `scripts/build-release.sh`; see [docs/release.md](docs/release.md).

Tests for the AGS parts:

```bash
timeout 900 cargo test --manifest-path adapter/Cargo.toml
```

```bash
timeout 900 cargo test --manifest-path runner/Cargo.toml
```

## Credits

This port stands on other people's work:

- **AstroHybrid** by faycal ([imnotsharingmyname/pcportastrobot](https://github.com/imnotsharingmyname/pcportastrobot)):
  the Windows PC port this project follows. The bridge that runs Kyty's renderer inside AnyPS5, and most of
  the engine changes in both forks, were written there. The launch settings follow its launcher.
- **KytyPS5** by Jetsku and contributors: the renderer, based on **Kyty** by InoriRus.
- **AnyPS5** by boykopovar and contributors: the native runtime for the game's code.
- **lsfg-vk**: frame generation on Linux.

See [NOTICE.md](NOTICE.md) for upstreams, bases and third-party licences.

## Licence

AstroPort is multi-licence. The port's own code and both engines are licensed under the GNU General Public
License, version 2 only (`GPL-2.0-only`); see [LICENSE](LICENSE). Third-party libraries under
`engines/*/3rdparty/` keep their own licences.

Release packages also contain a DLSS-enabled renderer built with NVIDIA's DLSS SDK, and NVIDIA's DLSS runtime
where the SDK licence allows it to be redistributed. Those parts are under NVIDIA's DLSS SDK licence, which is
not compatible with the GPL. Distributing them together with GPL-2.0 code is a known licence risk that this
project accepts. The SDK is not in this repository: the release build fetches it from
https://github.com/NVIDIA/DLSS, and a source build without it has no DLSS. DLSS is offered on NVIDIA GPUs only.
