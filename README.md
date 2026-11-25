# AstroPort

An unofficial native Linux port of Astro Bot (PS5, PPSA21567). The game's own code runs natively on
Linux through AnyPS5. Kyty's renderer, built as a shared library and loaded into the same process, draws
it through Vulkan. A small Rust library, the bridge, joins the two.

This is a fan project. It is not affiliated with, endorsed by, or supported by Sony Interactive Entertainment
or Team Asobi. "Astro Bot" and "PlayStation" are trademarks of their owners.

**No game files are included.** You need your own legally obtained copy of the game. This repository
contains no game code or assets, no PlayStation firmware, and no keys.

## Status

In development. The bridge and every engine change were rewritten from scratch on top of the unmodified
upstream engines, and much of the new code has not yet met real hardware.

- **Runs:** on the development machine (Linux, NVIDIA RTX 3090) the game starts on the bridge and reaches
  the title level. Game version 1.019; 1.018 is less tested and matched by version only.
- **Written, not yet tested on hardware:** controller input and output (rumble, light bar, adaptive
  triggers, motion), controller audio and haptics (USB and Bluetooth DualSense), controller selection, the
  frame-rate limiter, NVIDIA low-latency mode, the native Wayland surface, live settings from AGS, and the
  save-data fallback. They compile and their unit tests pass; nobody has played with them yet.
- **DLSS:** the setting, the bridge options and the release packaging exist. The renderer change that
  replaces the game's upscale pass is not in the tree yet, so the setting has no effect.
- **Frame generation:** AMD FSR 3 frame generation, built into the renderer (any Vulkan GPU with the needed
  features). AGS's Frame generation setting (`perf.frame_generation`, local play, method `fsr`, values Off and 2x, default Off) sets `ASTROPORT_FSR_FG`. The game keeps its frame cap and Kyty presents one generated frame per real one: 60 fps shows 120, 30 fps shows 60. With vertical sync the display must refresh at least twice the cap, or generation stays off (a 60 Hz display at 60 fps). Low latency is off while it is on. Because the game declares the key, AGS does not inject its own lsfg-vk for it; lsfg-vk profiles the player set up outside AGS still apply when the setting is Off.
- **AGS install:** the extension (`adapter/`, `runner/`) and the release scripts exist; a release built
  from this tree has not been installed through AGS yet.

## Layout

| Path | What it is |
|---|---|
| `engines/kyty/` | KytyPS5, unmodified upstream plus our changes: the renderer as a library (`libkyty_renderer.so`, seam in `src/embed/`). See its `UPSTREAM.md`. |
| `engines/anyps5/` | AnyPS5, unmodified upstream plus our changes: a switch for an external renderer and the host module `libAstroportHost`. See its `UPSTREAM.md`. |
| `bridge/` | Rust library `libastroport_bridge.so`: starts the renderer inside the game process, routes the game's PS5 graphics and memory calls to it, relays events, controller and settings. Also `nidgen`, which generates the three forwarder libraries. |
| `patcher/` | Rust library that applies KytyPS5-style memory patches (`patches/*.json`) to the running game. |
| `patches/` | Patch definitions for game versions 1.018 and 1.019, and the AGS settings (`settings.json`). |
| `adapter/` | Rust AGS extension (`ags-extension.toml`): the AstroPort pages, settings and launch. |
| `runner/` | `astroport-run`, the AGS build job that turns a game dump into a run folder. |
| `tools/` | Helpers for porting patch files between game versions and for recording controller audio; `tools/hidtap/` records the haptic samples sent to a DualSense over Bluetooth. |
| `scripts/` | `build-engines.sh`, `play.sh`, `smoke.sh`, `check-repo.sh`, `build-release.sh` and `release/`. |
| `assets/`, `ags-extension.toml` | Images and manifest of the AGS extension. |
| `docs/` | `release.md`; `rewrite/` holds the design of the bridge and the specifications it was built from. |

## Building

```bash
scripts/build-engines.sh
```

This builds the renderer, AnyPS5's libraries and relinker, the bridge and the forwarders into `build/`, and
writes the install tree `build/install`. It needs network access on the first configure (Kyty downloads
opus, OpenXR-SDK, xbyak, zydis, zstd, ZArchive and Noto Sans), and CMake, Ninja, clang, gcc, cargo, Python 3,
`nm` and `glslangValidator`. `BUILD_DIR=<dir>` changes the output folder and `JOBS=<n>` limits the parallel
jobs.

The other Rust parts are built with cargo:

```bash
cargo build --release --locked --manifest-path patcher/Cargo.toml
cargo build --release --locked --manifest-path adapter/Cargo.toml
cargo build --release --locked --manifest-path runner/Cargo.toml
```

Tests that need neither engine nor game:

```bash
cargo test --manifest-path bridge/Cargo.toml
cargo test --manifest-path adapter/Cargo.toml
cargo test --manifest-path runner/Cargo.toml
```

Releases are built with `scripts/build-release.sh`; see [docs/release.md](docs/release.md).

## Running

Through AGS: add this repository's URL in AGS, set "Astro Bot dump folder", and press "Build game files".

By hand, for development. First turn your dump into a run folder, which lands in
`<data>/generated/run-<version>`:

```bash
runner/target/release/astroport-run build --dump <dump folder> --data <data folder> --install build/install
```

Then start it:

```bash
RUN_DIR=<data folder>/generated/run-1019 scripts/play.sh
```

`scripts/play.sh` is a developer script; its header lists the options (frame cap, start level, patches off,
log file). It loads the game patches through `patcher/`, so build that first or pass `CHEATS=0`.

## Built on

- **KytyPS5** by Jetsku and contributors: the renderer, based on **Kyty** by InoriRus.
- **AnyPS5** by boykopovar and contributors: the native runtime for the game's code.

See [NOTICE.md](NOTICE.md) for upstreams, bases and third-party licences.

## Licence

The port's own code and both engines are licensed under the GNU General Public License, version 2 only
(`GPL-2.0-only`); see [LICENSE](LICENSE). Third-party libraries under `engines/*/3rdparty/` keep their own
licences.

The renderer contains no NVIDIA code and is not built against NVIDIA's SDK. Release packages may also carry
NVIDIA's DLSS runtime, a separate file under NVIDIA's DLSS SDK licence, which is not a GPL licence. It is not
in this repository: the release build fetches it from https://github.com/NVIDIA/DLSS. DLSS is offered on
NVIDIA GPUs only.

## Inspired by

**AstroHybrid** by faycal (https://github.com/imnotsharingmyname/pcportastrobot) is the project that showed
the approach of running Kyty's renderer inside an AnyPS5 process. AstroPort is an independent
implementation of that idea and shares no code with it.
