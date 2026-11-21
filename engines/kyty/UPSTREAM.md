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

AstroHybrid's bridge by faycal (https://github.com/imnotsharingmyname/pcportastrobot) and the
AstroPort changes, merged in `66b0b870` ("Merge Jetsku int22 into the Linux hybrid renderer"):

```
$ git log --no-merges --reverse --format='- %h %an: %s' dd9f9901..457b1fa8
- 00157dbd faycal: perf(texture memo): ASTROHYBRID_OPT_1 raises the binding memo default to 32768 entries
- c7716fb7 faycal: perf(page manager): ASTROHYBRID_OPT_1 skips host_lock for settled deferred spans
- 234c5298 faycal: feat(hybrid): kyty_gpu.dll bridge for AnyPS5's native AstroBot.exe (milestone 1)
- 14cbbbcd faycal: feat(hybrid): start Kyty's renderer inside the host process (milestone 2a)
- a6e784b8 faycal: feat(hybrid): lazy start, direct-memory exports and GPU event proxy (milestone 2b)
- 8221d49a faycal: feat(hybrid): Kyty's renderer runs Astro Bot inside AstroBot.exe (milestone 2)
- c5443342 faycal: feat(hybrid): relay Kyty's window input to the host pad library (milestone 3)
- 9275450f faycal: test(hybrid): refresh GPU loader expectations
- e4f3e5f5 faycal: chore(hybrid): snapshot Claude milestone 4 build work
- 4d41d0a4 faycal: hybrid: ABI 1.3, a closed window asks the host to exit instead of tearing the renderer down
- 6ed4997b faycal: perf: add opt-in draw preparation worker priority
- f10725be faycal: perf(watchdog): lock-free updates of known events (ASTROHYBRID_WATCHDOG_FAST_EVENTS)
- 6eba4178 faycal: feat(hybrid): ABI 1.4, the relay applies the host's pad output (rumble, light bar, trigger effects)
- 0aa28fed faycal: perf(diagnostics): log descriptor offset reuse by bind point
- df58c118 faycal: perf(diagnostics): attribute CP draw commit to render targets
- d42e4348 faycal: fix(hybrid): enable persistent graphics caches with a title ID
- 520675ae faycal: feat: add opt-in AstroHybrid frame-rate settings
- 6fdc82ff faycal: feat(hybrid): 40/45 FPS limits, fixed-cadence pacing for every limit, fullscreen start
- 1cbf2e8d faycal: feat(hybrid): opt-in real-time game step (ASTROHYBRID_GAME_SPEED)
- 34dda51d faycal: feat(hybrid): 35/50/55/90 FPS limits, game speed percent, smoother real-time step
- 614acdba faycal: feat(hybrid): ASTROHYBRID_WINDOW_TITLE and ASTROHYBRID_OVERLAY start modes (fps/compact/full)
- 4cb7fc66 faycal: feat(hybrid): opt-in guest-load fault context for read faults in the game image (ASTROHYBRID_GUEST_LOAD_FAULTS)
- cae890db faycal: feat(hybrid): opt-in relay of pad motion sensors to the host (ABI 1.5)
- 9dbd6770 faycal: feat(hybrid): opt-in generation fast path for writable buffer bindings
- dbced347 faycal: feat(hybrid): carry the read memo's cross-epoch proof by its signature
- e31d441a faycal: feat(hybrid): opt-in NVIDIA low-latency mode through VK_NV_low_latency2
- 9b4e9cda faycal: feat(hybrid): pass trace and targeted shader dump diagnostics
- 94cbc59a faycal: feat(hybrid): opt-in DLSS Super Resolution for the game's temporal upscale (blocked)
- a06d0635 faycal: docs(hybrid): DLSS needs the host's CUDA address window
- 377ff170 faycal: feat(hybrid): opt-in DLSS Frame Generation through Streamline
- 6312ad97 faycal: feat(hybrid): overlay shows frames on screen with frame generation
- 6a9bdfd6 faycal: feat(hybrid): ASTROHYBRID_FPS_LIMIT accepts any value from 10 to 240
- dc6509ce faycal: feat(hybrid): adaptive frame generation multiplier for an on-screen target
- a1ccda55 faycal: Controller: pick the playing pad, rumble strength for non-DualSense pads
- 37198216 faycal: Relay: angular velocity from orientation for pads without a gyroscope
- feb4bf5f Ali: feat(hybrid): Bluetooth DualSense speaker/haptics bridge, rumble scale, SaveDataTransferringMount
- ab9de9fc Jetsku: bufferCache: include the rdtsc/prefetch intrinsics header per compiler (Linux build)
- 2423aeee Claude: AstroHybrid on Linux: libkyty_gpu.so bridge, .prx forwarders, Linux fault and memory paths
- f14f408a Claude: LowLatency: ASTROHYBRID_LOW_LATENCY_INTERVAL_US paces frames in vkLatencySleepNV
- f7c280a7 Ali: DualSense: ASTROHYBRID_DUALSENSE_RUMBLE=0 keeps rumble reports off
- 07606667 Ali: AstroHybrid: ordinary mappings search the whole user range when the host donated memory
- 457b1fa8 Ali: Remove the Qt launcher
```

AGS session socket client (`src/streaming/`) ported from KytyPS5-AGS `src/streaming` at 064e7b9c6 (Ali, GPL-2.0), updated
to session protocol 2; live settings (`liveSettings*.cpp`), `KytyGpuPadVibrationGain` export.

## 4. Files changed against the base

For GPL-2.0 section 2a, as of 2026-11-21. None of these files carries a licence header, so no
"Modified by" line was added to any file.

```
$ git diff --stat=200 dd9f9901 457b1fa8
 CMakeLists.txt                                                 |   27 +--
 src/common/CMakeLists.txt                                      |    3 +
 src/common/astroHybrid.h                                       |   34 +++
 src/common/hangWatchdog.cpp                                    |   60 +++++-
 src/common/hleWaitStats.h                                      |    4 +-
 src/common/hostException.cpp                                   |   56 +++++
 src/common/threads.cpp                                         |    7 +
 src/common/threads.h                                           |    2 +
 src/graphics/host_gpu/pageManager.cpp                          |   31 +++
 src/graphics/host_gpu/renderer/cache/bufferCache.cpp           |  242 +++++++++++++++++++++
 src/graphics/host_gpu/renderer/cache/bufferCache.h             |   68 ++++++
 src/graphics/host_gpu/renderer/drawPrep/cpGaps.cpp             |   66 ++++++
 src/graphics/host_gpu/renderer/drawPrep/cpGaps.h               |   58 +++++
 src/graphics/host_gpu/renderer/drawPrep/drawPrep.cpp           |    6 +-
 src/graphics/host_gpu/renderer/frameGen.cpp                    |  604 +++++++++++++++++++++++++++++++++++++++++++++++++++
 src/graphics/host_gpu/renderer/frameGen.h                      |   76 +++++++
 src/graphics/host_gpu/renderer/hybridDlss.cpp                  |  631 ++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/graphics/host_gpu/renderer/hybridDlss.h                    |   44 ++++
 src/graphics/host_gpu/renderer/passTrace.h                     |   83 ++++++++
 src/graphics/host_gpu/renderer/pipeline/descriptors.cpp        |  201 ++++++++++++++++-
 src/graphics/host_gpu/renderer/pipeline/descriptors.h          |    5 +
 src/graphics/host_gpu/renderer/pipeline/pipelineCache.cpp      |   62 +++++-
 src/graphics/host_gpu/renderer/pipeline/textureBindingMemo.cpp |    8 +-
 src/graphics/host_gpu/renderer/render.h                        |   16 +-
 src/graphics/host_gpu/renderer/renderCompute.cpp               |    9 +
 src/graphics/host_gpu/renderer/renderDraw.cpp                  |   27 ++-
 src/graphics/presentation/performanceOverlay.cpp               |   58 ++++-
 src/graphics/presentation/videoOut.cpp                         |  234 +++++++++++++++++++-
 src/graphics/presentation/videoOut.h                           |    3 +
 src/graphics/presentation/window/lowLatency.cpp                |  429 +++++++++++++++++++++++++++++++++++++
 src/graphics/presentation/window/lowLatency.h                  |   63 ++++++
 src/graphics/presentation/window/swapchain.cpp                 |   13 ++
 src/graphics/presentation/window/vulkanWindow.cpp              |  101 ++++++++-
 src/graphics/presentation/window/window.cpp                    |   24 ++-
 src/graphics/shader/recompiler/ir/ProgramCodec.cpp             |    2 +-
 src/hybrid/kytyGpuApi.h                                        |  228 ++++++++++++++++++++
 src/hybrid/kytyGpuBridge.cpp                                   | 1007 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/hybrid/kytyGpuNids.inc                                     |  102 +++++++++
 src/hybrid/kyty_gpu.def                                        |  105 +++++++++
 src/kernel/memory.cpp                                          |   14 +-
 src/kernel/memory.h                                            |    5 +
 src/kernel/memoryAddressSpace.inc                              |   47 +++-
 src/launcher/CMakeLists.txt                                    |  327 ----------------------------
 src/launcher/Info.plist.in                                     |   17 --
 src/launcher/forms/configuration_edit_dialog.ui                |  814 ---------------------------------------------------------------------
 src/launcher/forms/configuration_list_widget.ui                |  417 ------------------------------------
 src/launcher/forms/main_dialog.ui                              |  185 ----------------
 src/launcher/icons/edit-configuration.svg                      |    7 -
 src/launcher/icons/global-settings.svg                         |    8 -
 src/launcher/icons/grid-view.svg                               |    8 -
 src/launcher/icons/input-mapping.svg                           |    9 -
 src/launcher/icons/kyty-small.svg                              |   18 --
 src/launcher/icons/kyty.svg                                    |   18 --
 src/launcher/icons/racing-wheel.svg                            |    7 -
 src/launcher/icons/refresh.svg                                 |    6 -
 src/launcher/icons/remove-configuration.svg                    |    6 -
 src/launcher/icons/trophy.svg                                  |    7 -
 src/launcher/include/additionalContentDialog.h                 |   42 ----
 src/launcher/include/cheatFile.h                               |   28 ---
 src/launcher/include/cheatRepository.h                         |   65 ------
 src/launcher/include/compatibilityDatabase.h                   |   38 ----
 src/launcher/include/configuration.h                           |  322 ----------------------------
 src/launcher/include/configurationEditDialog.h                 |   73 -------
 src/launcher/include/configurationItem.h                       |   65 ------
 src/launcher/include/configurationListWidget.h                 |  120 -----------
 src/launcher/include/controllerPreview.h                       |   55 -----
 src/launcher/include/dualsenseWidget.h                         |   40 ----
 src/launcher/include/gameContent.h                             |   27 ---
 src/launcher/include/gameViews.h                               |  193 -----------------
 src/launcher/include/inputMappingDialog.h                      |   54 -----
 src/launcher/include/launcherTheme.h                           |   12 --
 src/launcher/include/mainDialog.h                              |   40 ----
 src/launcher/include/mandatoryLineEdit.h                       |   28 ---
 src/launcher/include/patchesDialog.h                           |   51 -----
 src/launcher/include/precompile.h                              |   84 --------
 src/launcher/include/precompileDialog.h                        |   80 -------
 src/launcher/include/shaderListExport.h                        |   91 --------
 src/launcher/include/titleSound.h                              |   36 ----
 src/launcher/include/trophyViewerDialog.h                      |   28 ---
 src/launcher/include/updateChecker.h                           |   65 ------
 src/launcher/include/updateRelease.h                           |   38 ----
 src/launcher/include/wheelMappingDialog.h                      |  106 ---------
 src/launcher/resources.qrc                                     |   18 --
 src/launcher/src/additionalContentDialog.cpp                   |  159 --------------
 src/launcher/src/cheatFile.cpp                                 |  254 ----------------------
 src/launcher/src/cheatRepository.cpp                           |  195 -----------------
 src/launcher/src/compatibilityDatabase.cpp                     |  199 -----------------
 src/launcher/src/configuration.cpp                             |  304 --------------------------
 src/launcher/src/configurationEditDialog.cpp                   |  818 ----------------------------------------------------------------------
 src/launcher/src/configurationItem.cpp                         |  387 ---------------------------------
 src/launcher/src/configurationListWidget.cpp                   | 1505 --------------------------------------------------------------------------------------------------------------------------------
 src/launcher/src/controllerPreview.cpp                         |  245 ---------------------
 src/launcher/src/dualsenseWidget.cpp                           |  300 --------------------------
 src/launcher/src/gameContent.cpp                               |   80 -------
 src/launcher/src/inputMappingDialog.cpp                        |  761 -----------------------------------------------------------------
 src/launcher/src/launcherTheme.cpp                             |   45 ----
 src/launcher/src/main.cpp                                      |   72 -------
 src/launcher/src/mainDialog.cpp                                | 1077 -------------------------------------------------------------------------------------------
 src/launcher/src/mandatoryLineEdit.cpp                         |   44 ----
 src/launcher/src/patchesDialog.cpp                             |  314 ---------------------------
 src/launcher/src/precompile.cpp                                |  288 -------------------------
 src/launcher/src/precompileDialog.cpp                          |  357 -------------------------------
 src/launcher/src/shaderListExport.cpp                          |  206 ------------------
 src/launcher/src/titleSound.cpp                                |  106 ---------
 src/launcher/src/trophyViewerDialog.cpp                        |  573 -------------------------------------------------
 src/launcher/src/updateChecker.cpp                             |  440 --------------------------------------
 src/launcher/src/updateRelease.cpp                             |   89 --------
 src/launcher/src/wheelMappingDialog.cpp                        |  820 ----------------------------------------------------------------------
 src/launcher/styles/launcher.qss                               |  120 -----------
 src/libs/agc.cpp                                               |    4 +
 src/libs/controller.cpp                                        |  160 +++++++++++++-
 src/libs/controller.h                                          |    2 +
 src/libs/dualSenseBluetooth.cpp                                |   43 ++--
 src/libs/dualSenseHaptics.cpp                                  |   49 ++++-
 src/libs/libSaveData.cpp                                       |   30 ++-
 tests/LauncherConfigurationTests.cpp                           |  280 ------------------------
 tests/LauncherEditDialogTests.cpp                              |  124 -----------
 tests/LauncherGameViewsTests.cpp                               |  122 -----------
 tests/LauncherPrecompileItemTests.cpp                          |  166 ---------------
 tests/LauncherPrecompileTests.cpp                              |  268 -----------------------
 tests/LauncherShaderListExportTests.cpp                        |  194 -----------------
 tests/hybrid/kytyGpuLoadTest.c                                 |  248 +++++++++++++++++++++
 tests/hybrid/kytyGpuTestHost.c                                 |   55 +++++
 tools/hybrid/build_kyty_gpu.py                                 |  122 +++++++++++
 tools/hybrid/build_kyty_gpu_linux.py                           |  122 +++++++++++
 tools/hybrid/build_linux_shims.py                              |   63 ++++++
 tools/hybrid/gen_kyty_gpu_exports.py                           |   95 +++++++++
 127 files changed, 5779 insertions(+), 14644 deletions(-)
```

## 5. Snapshot

`kyty-hybrid-src`, branch `main`, commit `457b1fa8`, plus these submodules at their pinned commits:

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
