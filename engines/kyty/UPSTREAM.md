# Upstream and provenance of engines/kyty

## 1. Upstream

KytyPS5 by Jetsku: https://github.com/Jetsku/KytyPS5 (GPL-2.0-only). It is based on
Kyty by InoriRus (MIT; the licence text is `LICENSES/Kyty-MIT.txt`).

## 2. Base

The base is `dd9f9901` (full hash `dd9f9901dbafc5419a8f8e98b8bda0eb80492b2c`), which is the tag
`u59-windows-20261118-int22` and also Jetsku `main` on 2026-11-21:

```
$ git ls-remote origin refs/heads/main refs/tags/u59-windows-20261118-int22
dd9f9901dbafc5419a8f8e98b8bda0eb80492b2c	refs/heads/main
dd9f9901dbafc5419a8f8e98b8bda0eb80492b2c	refs/tags/u59-windows-20261118-int22
$ git rev-list --count 744705ee..dd9f9901
1210
```

It contains 1210 commits on top of the int21 release `744705ee`
(tag `u59-windows-20261024-int21`). Authors of those commits:

```
$ git shortlog -sn --no-merges 744705ee..dd9f9901
   705	Jetsku
   290	nmzik
    18	Claxten
    13	Senaxx
     8	TheCruZ
     6	Nawid3333
     6	sniperspy60
     5	Artyom Zabroda
     5	Mr. Big Britches
     5	Çağatay Güçlü
     4	Ettore Sartori
     4	superkeka
     3	Chai T
     2	Fadil
     1	4wgycvn84p-afk
     1	Andre Michi
     1	Axcxiii
     1	Claude
     1	Devran Cosmo Uenal
     1	Erlou
     1	Felipe
     1	Henrique Reis Neves
     1	KairosMakki
     1	LloydNicholson
     1	Luiz Henrique
     1	M4
     1	Mac
     1	Matus Riedl
     1	MrFr3di
     1	NeySlim
     1	Phr3d13
     1	Pixeluted
     1	Tikhon Ulyanov
     1	bigjjmoneyman
     1	gallexme
     1	herrMirto
     1	lukintore
     1	mcm
     1	shadowbeat070
     1	Алексей Изилянов
```

## 3. Carried changes

The tree is the base, unmodified, plus the changes below. All of them were written by the AstroPort
project.

- **Renderer as a library.** The CMake target `kyty_renderer` builds the emulator's sources without
  `main.cpp` into `libkyty_renderer.so`, which exports one symbol. `src/embed/` holds its C function table
  (`kytyEmbed.h`, `kytyEmbed.cpp`), the version script, and `embedOptions.h`, the header-only options that
  the hooks in upstream files read. In the stand-alone emulator every option has its default and the hooks
  do nothing.
- **Present gate.** `videoOut.cpp`: the present thread asks the embedding host before each flip and reports
  each present, so that the host can limit the frame rate.
- **Window.** `window.cpp`: a fixed window title, the host's frame limit in the session socket's live
  output, a live change of the window size, and a Wayland surface at the display's real resolution.
- **NVIDIA low-latency mode.** `lowLatency.h`, `lowLatency.cpp` (VK_NV_low_latency2 with
  VK_KHR_present_id), with hooks in `vulkanWindow.cpp`, `swapchain.cpp` and `videoOut.cpp`; the seam wraps
  the AGC flip and submit exports instead of editing them.
- **Controllers.** `controller.cpp`: the pad in use becomes player 1, by preferred pad family when one is
  set; rumble strength scale, and a minimum strength for Switch pads.
- **DualSense.** `dualSenseBluetooth.cpp`, `dualSenseHaptics.cpp`: a pad of unknown connection counts as
  wireless, the only Bluetooth DualSense is used when its address is unknown, the speaker is unmuted and
  plays both channels as mono, and a switch keeps motor rumble reports off.
- **FSR frame generation (build option `KYTY_FSR_FRAMEGEN`, off by default).** In
  `src/graphics/presentation/window/`: `frameGen.h`, `frameGen.cpp` (the capture ring and the generator),
  `frameGenLogic.h` (pacing, display gate, continuity), `ffxBlobs.h`, `ffxBlobs.cpp`, `ffxReflect.h` (the
  SDK's shaders and their binding lists) and `ffxCompat.h` (force-included into the SDK's sources); hooks
  in `swapchain.cpp`, `vulkanWindow.cpp`, `upscalePass.h` and `upscalePass.cpp`; `tests/FrameGenTests.cpp`;
  the fetch and the library `kyty_ffx` in `3rdparty/CMakeLists.txt`, with
  `3rdparty/patches/fidelityfx-context-size.patch`, `fidelityfx-scratch-align.patch` and
  `fidelityfx-descriptor-pool.patch`; the shaders, the option and the test in
  `CMakeLists.txt`. With the option off none of it is compiled and the hooks do nothing.
- **Build on Linux with clang.** One line or a few each in `src/common/CMakeLists.txt`, `hleWaitStats.h`,
  `descriptors.cpp`, `ProgramCodec.cpp` and `ResourceTracking.cpp`.
- **AGS session socket client.** `src/streaming/`, `tests/SessionSocketTests.cpp`, with its CMake wiring and
  a frame counter in `window.cpp`.
- **Qt launcher removed.** `src/launcher`, its six `tests/Launcher*Tests.cpp` and its CMake wiring are not
  in the tree.

## 4. Files changed against the base

For GPL-2.0 section 2a, as of 2026-11-25. None of these files carries a licence header, so no
"Modified by" line was added to any file.

The list is taken against repository commit `0c140c49`, at which this tree was the base plus the last
three items of section 3 (session socket client, launcher removal, and the `ResourceTracking.cpp` line),
which it therefore does not show.

```
$ cd engines/kyty && git diff --relative --stat=200 0c140c49 -- . ':!UPSTREAM.md'
 CMakeLists.txt                                          |  12 +++++
 src/common/CMakeLists.txt                               |   2 +-
 src/common/hleWaitStats.h                               |   4 +-
 src/embed/embedOptions.h                                |  69 ++++++++++++++++++++++++++++
 src/embed/kytyEmbed.cpp                                 | 335 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/embed/kytyEmbed.h                                   |  80 +++++++++++++++++++++++++++++++++
 src/embed/kytyEmbed.map                                 |   1 +
 src/graphics/host_gpu/renderer/pipeline/descriptors.cpp |   2 +-
 src/graphics/presentation/videoOut.cpp                  |  52 +++++++++++++++++++--
 src/graphics/presentation/window/lowLatency.cpp         | 275 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/graphics/presentation/window/lowLatency.h           |  43 ++++++++++++++++++
 src/graphics/presentation/window/swapchain.cpp          |   7 ++-
 src/graphics/presentation/window/vulkanWindow.cpp       |  10 +++++
 src/graphics/presentation/window/window.cpp             |  26 +++++++----
 src/graphics/shader/recompiler/ir/ProgramCodec.cpp      |   2 +-
 src/libs/controller.cpp                                 | 108 ++++++++++++++++++++++++++++++++++++++++++--
 src/libs/dualSenseBluetooth.cpp                         |  25 ++++++++---
 src/libs/dualSenseHaptics.cpp                           |  26 +++++++++--
 18 files changed, 1049 insertions(+), 30 deletions(-)
```

## 5. Snapshot

The upstream tree at the base `dd9f9901`, plus these submodules at their pinned commits:

| Path | URL | Commit |
|------|-----|--------|
| 3rdparty/LibAtrac9 | https://github.com/shadps4-emu/ext-LibAtrac9.git | `ec8899dadf393f655f2871a94e0fe4b3d6220c9a` |
| 3rdparty/SDL3 | https://github.com/libsdl-org/SDL.git | `fa2c02bb6e21974a89ea9824bc53c9932abe5f9c` |
| 3rdparty/SPIRV-Headers | https://github.com/KhronosGroup/SPIRV-Headers.git | `01e0577914a75a2569c846778c2f93aa8e6feddd` |
| 3rdparty/SPIRV-Tools | https://github.com/KhronosGroup/SPIRV-Tools.git | `7f2d9ee926f98fc77a3ed1e1e0f113b8c9c49458` |
| 3rdparty/Vulkan-Headers | https://github.com/KhronosGroup/Vulkan-Headers.git | `2fa203425eb4af9dfc6b03f97ef72b0b5bcb8350` |
| 3rdparty/VulkanMemoryAllocator | https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator.git | `a1d434708c217b2a6c7b365f1fe41fa03a562e59` |
| 3rdparty/ffmpeg-core | https://github.com/KytyPS5/ext-ffmpeg-core.git | `9ac4cfd195f192ed8b08566f49c28dbb48d08341` |
| 3rdparty/fmt | https://github.com/fmtlib/fmt.git | `11ddbcb7898d2d3445d431a54814367b21dee6ad` |
| 3rdparty/imgui | https://github.com/ocornut/imgui.git | `f1cc2ae15e53a861a874c3034aae6798fde194ab` |
| 3rdparty/magic_enum | https://github.com/Neargye/magic_enum.git | `1384769c66bd16ec9bb1353f45fe8ec8ccc12dbd` |
| 3rdparty/nlohmann_json | https://github.com/nlohmann/json.git | `272411c5e6ea45919af7673524e74e60c62116df` |
| 3rdparty/spdlog | https://github.com/gabime/spdlog.git | `8671ca4d492c8ee1cdfd3dd88afb9f88dd268178` |
| 3rdparty/tracy | https://github.com/wolfpld/tracy.git | `30997d5ca6bb632cc10807a1da8a6d3de0aeeb3c` |
| 3rdparty/xxHash | https://github.com/Cyan4973/xxHash.git | `e573d4d2aaeaba0f3e5a0a9a54144a1f2b4b56e7` |

Changed in this snapshot: `3rdparty/ffmpeg-core/CMakeLists.txt` no longer runs `git rev-parse` to find
the ext-ffmpeg-core release to download. The snapshot has no `.git` there, so it sets
`FFMPEG_GIT_SHA` to the pinned `9ac4cfd195f1`.

## 6. Left out

Pruned from the snapshot (licence files are kept at every root, including inside pruned paths):

- Repository meta: `.github`, `.coderabbit.yaml`, `.pre-commit-config.yaml`, `.gitmodules`, and every `.github/`, `.gitignore`, `.gitmodules` at any depth.
- `tools/pgo` (Windows PGO profiles, used only with `KYTY_PGO_USE`) and `docs/screenshots`.
- `3rdparty/winpthread` (clang-cl only; only its `COPYING` is kept).
- `3rdparty/SDL3/{test,examples,docs,Xcode,VisualC,VisualC-GDK,android-project}`
- `3rdparty/Vulkan-Headers/{registry,tests}`
- `3rdparty/SPIRV-Tools/kokoro`, and everything in `3rdparty/SPIRV-Tools/test/` except `CMakeLists.txt`
- `3rdparty/tracy/{profiler,examples,manual,capture,csvexport,import,update,server}`
- `3rdparty/nlohmann_json/{docs,tests}`, `3rdparty/imgui/{examples,docs}`, `3rdparty/xxHash/{tests,fuzz,doc}`, `3rdparty/fmt/doc, 3rdparty/fmt/test except test/gtest (the CMake build compiles its gtest)`, `3rdparty/spdlog/{tests,example,bench}`
- Restored because they are licence files: `3rdparty/winpthread/COPYING` and the `LICENSE`, `COPYING`, `license.md` files under the pruned `test`, `doc`, `docs`, `examples` folders.
- Every `*.dll *.exe *.lib *.profdata *.prx *.sprx *.elf *.log` and `eboot.bin`.

## 7. Fetched at configure time

CMake downloads these (`3rdparty/CMakeLists.txt`), so configuring needs network access:
opus `ddbe4838`, OpenXR-SDK `f2448a87`, xbyak `44a72f36`, zydis `120e0e70`, zstd `f8745da6`,
ZArchive `d2c71773` (with `3rdparty/patches/zarchive-reader.patch`), Noto Sans `42af67cc`.

Only with `-DKYTY_FSR_FRAMEGEN=ON`: AMD FidelityFX SDK, tag `v1.1.4`, checked to be commit
`c6efa6bf7f2027b3ec94f28578bb5965eabb9e55` (https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK,
MIT, `LICENSE.txt` at its root), with `3rdparty/patches/fidelityfx-context-size.patch` and
`fidelityfx-scratch-align.patch` and `fidelityfx-descriptor-pool.patch`. Seven of its
source files and eighteen of its GLSL shaders are built; none of its build files, tools or binaries is
used.
