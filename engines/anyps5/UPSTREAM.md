# Upstream and provenance of engines/anyps5

## 1. Upstream

AnyPS5 by boykopovar: https://github.com/boykopovar/AnyPS5 (GPL-2.0-only).

## 2. Base

The base is `319621ba2e` (full hash `319621ba2e578bd053b0baba2877fe70520e0737`, committed 2026-10-29),
a commit on upstream `main`.

## 3. Carried changes

The tree is the base, unmodified, plus the changes below. All of them were written by the AstroPort
project. With no external renderer installed, the upstream paths run.

- **External renderer switch.** `libkernel`: `ExternalRenderer.hpp`, `ExternalRenderer.cpp`, a table of
  function pointers that another library in the process may install. While one is installed, the direct
  memory exports (`DirectMemory/Export.cpp`) forward to it, and APR file reads (`Apr.cpp`) take the staged
  copy.
- **Host module.** `core/libs/prx/libAstroportHost/`: one C table through which the renderer's side hands
  over address space, GPU events and controller input, fetches controller output, asks for process exit
  and registers a shutdown callback; and the two HDR helpers `libSceSystemService` imports.
- **libScePad.** Keeps the raw trigger effect commands for the host module; behind
  `APS5_PAD_TRIGGER_EFFECT_STATE`, `scePadGetTriggerEffectState` derives each trigger's state from its
  effect and travel (`PadTriggerEffect.hpp`).
- **libSceAudioOut.** Controller audio and haptic channels play through the installed renderer
  (`APS5_PAD_VIBRATION_VIA_HOST`), at the gain the renderer's side reports, handed over by a worker queue;
  `AGS_CONTROLLER_AUDIO_SINK` names the controller sound card; `APS5_PAD_VIBRATION_DUMP` writes the mixed
  haptic channels to a file.
- **libSceSaveData.native.** Behind `APS5_SAVEDATA_TRANSFER_AS_MOUNT`, `sceSaveDataTransferringMount`
  mounts a regular save.
- **libSceAudioPropagation.** A source's input is rendered into the bed's first channel; path calculation
  without rays succeeds; non-empty audio paths are accepted and ignored.
- **libSceAvPlayer.** Tells the installed renderer when a player starts and stops showing video.
- **libc.** Lookup of files in an archived `app0`, and read-only stdio streams on archived files
  (`ArchiveStream.hpp`, `ArchiveStream.cpp`).
- **libkernel event queues.** Every thread waiting on an event queue is woken when an event fires.
- **Tests and notes.** Cases under `core/libs/tests/` and `libSceAudioOut/tests/` for the above, rows in
  `docs/dev/ENVIRONMENT.md` and notes in `docs/dev/debt/`.

## 4. Files changed against the base

For GPL-2.0 section 2a, as of 2026-11-25. No changed file carries a licence header, so no
"Modified by" line was added to any file.

The list is taken against repository commit `0c140c49`, at which this tree was the base plus three of the
fixes in section 3, which it therefore shows only where they were changed again: the first two audio
propagation items (`libSceAudioPropagation/AudioPropagation.hpp`, `Export.cpp`,
`core/libs/tests/GuestAudioPropagation.cpp`) and the event queue wake-up (`libkernel/Equeue/Equeue.cpp`,
`core/libs/tests/GuestEqueueEvents.cpp`).

```
$ cd engines/anyps5 && git diff --relative --stat=200 0c140c49 -- . ':!UPSTREAM.md'
 core/libs/prx/libAstroportHost/CMakeLists.txt            |  17 ++++++
 core/libs/prx/libAstroportHost/Export.cpp                | 254 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/prx/libAstroportHost/libAstroportHost.hpp      |  58 ++++++++++++++++++
 core/libs/prx/libSceAudioOut/CMakeLists.txt              |   6 ++
 core/libs/prx/libSceAudioOut/src/AudioOut.cpp            |  30 ++++++++--
 core/libs/prx/libSceAudioOut/src/AudioOut2Context.cpp    |  45 +++++++++++++-
 core/libs/prx/libSceAudioOut/src/AudioOut2Internal.hpp   |   1 +
 core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.cpp     |  51 ++++++++++++++++
 core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.hpp     |  37 ++++++++++++
 core/libs/prx/libSceAudioOut/src/AudioOut2Port.cpp       |   2 +-
 core/libs/prx/libSceAudioOut/tests/AudioOut2PadMix.cpp   |  89 ++++++++++++++++++++++++++++
 core/libs/prx/libSceAudioOut/tests/AudioOut2Renderer.cpp | 178 +++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/prx/libSceAudioPropagation/Export.cpp          |  41 ++++++++-----
 core/libs/prx/libSceAvPlayer/CMakeLists.txt              |   9 ++-
 core/libs/prx/libSceAvPlayer/include/AvPlayer.hpp        |   2 +
 core/libs/prx/libSceAvPlayer/src/Player.cpp              |  30 +++++++++-
 core/libs/prx/libScePad/Export.cpp                       |   9 +++
 core/libs/prx/libScePad/include/PadState.hpp             |   4 ++
 core/libs/prx/libScePad/include/PadTriggerEffect.hpp     |  27 +++++++++
 core/libs/prx/libScePad/src/PadState.cpp                 |  18 ++++++
 core/libs/prx/libSceSaveData.native/Export.cpp           |  17 +++++-
 core/libs/prx/libc/CMakeLists.txt                        |   3 +-
 core/libs/prx/libc/include/ArchiveStream.hpp             |  60 +++++++++++++++++++
 core/libs/prx/libc/include/FileStream.hpp                |  34 +++++++++++
 core/libs/prx/libc/src/AppArchive.cpp                    |  32 ++++++++--
 core/libs/prx/libc/src/ArchiveStream.cpp                 | 394 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/prx/libc/src/FileAndHeap.cpp                   |  66 +++++++++++++++++++--
 core/libs/prx/libc/src/Formatting.cpp                    |  59 +++++++++++++------
 core/libs/prx/libc/src/ScanningWide.cpp                  |   3 +
 core/libs/prx/libc/src/StandardStreams.cpp               |  16 ++++-
 core/libs/prx/libc/src/StreamPurge.cpp                   |   2 +
 core/libs/prx/libkernel/Apr/src/Apr.cpp                  |  18 +++---
 core/libs/prx/libkernel/CMakeLists.txt                   |  13 ++++
 core/libs/prx/libkernel/DirectMemory/Export.cpp          |  31 ++++++++++
 core/libs/prx/libkernel/System/src/ExternalRenderer.cpp  |  18 ++++++
 core/libs/prx/libkernel/include/ExternalRenderer.hpp     |  39 ++++++++++++
 core/libs/tests/GuestAppArchive.cpp                      |  65 +++++++++++++++++++-
 core/libs/tests/GuestAppArchiveStdio.cpp                 | 413 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/tests/GuestAstroportHost.cpp                   | 436 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/tests/GuestAudioPropagation.cpp                |  57 +++++++++++++++---
 core/libs/tests/GuestAvPlayer.cpp                        |  80 +++++++++++++++++++++++++
 core/libs/tests/GuestKernelAppArchive.cpp                |   2 +-
 core/libs/tests/GuestPad.cpp                             |  62 ++++++++++++++++++++
 core/libs/tests/GuestSaveDataMountStatus.cpp             |  38 ++++++++++++
 docs/dev/ENVIRONMENT.md                                  |   5 ++
 docs/dev/debt/libAstroportHost.md                        |   7 +++
 docs/dev/debt/libSceAudioOut.md                          |   1 +
 docs/dev/debt/libSceAudioPropagation.md                  |   4 +-
 docs/dev/debt/libSceAvPlayer.md                          |   1 +
 docs/dev/debt/libScePad.md                               |   1 +
 docs/dev/debt/libSceSaveData.native.md                   |   1 +
 docs/dev/debt/libc.md                                    |   3 +-
 docs/dev/debt/libkernel.md                               |   1 +
 53 files changed, 2810 insertions(+), 80 deletions(-)
```

## 5. Snapshot

The upstream tree at the base `319621ba2e`, plus 23
submodules at their pinned commits and 8 nested submodules (`mbedtls/framework`,
`freetype/subprojects/dlg`, and six under `nghttp2`):

| Path | URL | Commit |
|------|-----|--------|
| 3rdparty/ffmpeg-core | https://github.com/KytyPS5/ext-ffmpeg-core.git | `9ac4cfd195f192ed8b08566f49c28dbb48d08341` |
| 3rdparty/freetype | https://github.com/freetype/freetype.git | `42608f77f20749dd6ddc9e0536788eaad70ea4b5` |
| 3rdparty/freetype/subprojects/dlg | https://github.com/nyorain/dlg.git | `72dfcc858c040c54a6a0b88fcb7e70ee186d3167` |
| 3rdparty/glslang | https://github.com/KhronosGroup/glslang.git | `ba1640446f3826a518721d1f083f3a8cca1120c3` |
| 3rdparty/jxrlib | https://github.com/4creators/jxrlib.git | `f7521879862b9085318e814c6157490dd9dbbdb4` |
| 3rdparty/LibAtrac9 | https://github.com/shadps4-emu/ext-LibAtrac9.git | `946e05a9212976626a9f5e52f29c0a7202871f29` |
| 3rdparty/libjpeg-turbo | https://github.com/libjpeg-turbo/libjpeg-turbo.git | `c85e6b905bf237038faa936dab160ebfc5da0344` |
| 3rdparty/libusb-cmake | https://github.com/libusb/libusb-cmake.git | `bea6567b63796ab27c1c33257d230284fb2d8316` |
| 3rdparty/mbedtls | https://github.com/Mbed-TLS/mbedtls.git | `068ff080b369adfac81509f9b57b2afabaf82dc5` |
| 3rdparty/mbedtls/framework | https://github.com/Mbed-TLS/mbedtls-framework | `dde0c4a0e448a0552f18817dcea633bb851fd288` |
| 3rdparty/mimalloc | https://github.com/microsoft/mimalloc.git | `f8401befa675adb13b98decababd7fdc59572477` |
| 3rdparty/nghttp2 | https://github.com/nghttp2/nghttp2.git | `85e300c79fb6dbcfa9c1013215c8710c1c2cd3d2` |
| 3rdparty/nghttp2/tests/munit | https://github.com/ngtcp2/munit | `114805e1bab0222574b5eb3a92461ad5216647ed` |
| 3rdparty/nghttp2/third-party/mruby | https://github.com/mruby/mruby | `831da26b9021de0369d17b71b5667e2941a1a32d` |
| 3rdparty/nghttp2/third-party/neverbleed | https://github.com/tatsuhiro-t/neverbleed.git | `68f65c7b05f68e6fd93631668d4f469875946656` |
| 3rdparty/nghttp2/third-party/urlparse | https://github.com/ngtcp2/urlparse | `59b068a7618a256c6823b0b9801b61d1d04677a3` |
| 3rdparty/nghttp2/third-party/urlparse/http-parser | https://github.com/nodejs/http-parser | `ec8b5ee63f0e51191ea43bb0c6eac7bfbff3141d` |
| 3rdparty/nghttp2/third-party/urlparse/munit | https://github.com/ngtcp2/munit | `3cf9c79f3a76f313d560dedd5b47c0416a0fbb6e` |
| 3rdparty/ogg | https://github.com/xiph/ogg.git | `be05b13e98b048f0b5a0f5fa8ce514d56db5f822` |
| 3rdparty/opus | https://github.com/xiph/opus.git | `22244de5a79bd1d6d623c32e72bf1954b56235be` |
| 3rdparty/SDL2 | https://github.com/libsdl-org/SDL.git | `4b69833bc54abf3dd3288d4aa7afbba527775e5b` |
| 3rdparty/SPIRV-Headers | https://github.com/KhronosGroup/SPIRV-Headers.git | `b824a462d4256d720bebb40e78b9eb8f78bbb305` |
| 3rdparty/SPIRV-Tools | https://github.com/KhronosGroup/SPIRV-Tools.git | `262bdab48146c937467f826699a40da0fdfc0f1a` |
| 3rdparty/stb | https://github.com/nothings/stb.git | `2c980bb59875b0d32144a71867fbdebb2f77cd20` |
| 3rdparty/tracy | https://github.com/wolfpld/tracy.git | `30997d5ca6bb632cc10807a1da8a6d3de0aeeb3c` |
| 3rdparty/vgmstream | https://github.com/vgmstream/vgmstream.git | `7c520e8f9b078dfde2f84f1b5bcbad0c6c415806` |
| 3rdparty/vorbis | https://github.com/xiph/vorbis.git | `c2aa86b05e981c96bf381fc6aa11cdd03eccc2fb` |
| 3rdparty/Vulkan-Headers | https://github.com/KhronosGroup/Vulkan-Headers.git | `2fa203425eb4af9dfc6b03f97ef72b0b5bcb8350` |
| 3rdparty/VulkanMemoryAllocator | https://github.com/GPUOpen-LibrariesAndSDKs/VulkanMemoryAllocator.git | `a1d434708c217b2a6c7b365f1fe41fa03a562e59` |
| 3rdparty/ZArchive | https://github.com/Exzap/ZArchive.git | `965b66c8d67b6b7e30fd63b3b75aa91a99ff303b` |
| 3rdparty/zstd | https://github.com/facebook/zstd.git | `f8745da6ff1ad1e7bab384bd1f9d742439278e99` |

Changed in this snapshot: `3rdparty/Vulkan-Headers` and `3rdparty/ffmpeg-core` are symlinks to
Kyty's copies (`../../kyty/3rdparty/...`), which are the same commits (`2fa20343`, `9ac4cfd1`).

## 6. Left out

Pruned from the snapshot (licence files are kept at every root, including inside pruned paths):

- Repository meta: `.github`, `.gitmodules`, and every `.github/`, `.gitignore`, `.gitmodules` at any depth.
- Unused on Linux: `3rdparty/VulkanMemoryAllocator` (only its licence is kept), `3rdparty/mimalloc` (Windows only; only its licence is kept), `3rdparty/tracy` (needs `APS5_ENABLE_TRACY`, default OFF; only licences are kept).
- `3rdparty/SDL2/{test,Xcode,Xcode-iOS,VisualC,VisualC-GDK,VisualC-WinRT,android-project,docs}`
- `3rdparty/mbedtls/{tests,docs}`, `3rdparty/glslang/{Test,gtests,ndk_test}`
- `3rdparty/SPIRV-Tools/kokoro`, and everything in `3rdparty/SPIRV-Tools/test/` except `CMakeLists.txt`
- `3rdparty/opus/{dnn/torch,training,tests,doc}`, `3rdparty/vgmstream/ext_libs/dll-x64`
- `3rdparty/nghttp2/{tests,third-party/mruby}` (`examples` and `contrib` are kept: nghttp2's CMake adds them unconditionally; `doc` is kept too, because its CMake calls `configure_file` on `doc/*.rst.in`)
- `3rdparty/freetype/{docs,tests}`, `3rdparty/libjpeg-turbo/testimages` (`doc` is kept: the install step of libjpeg-turbo reads `doc/*.txt`; only `doc/change.log` is dropped as a `*.log` file), `3rdparty/zstd/{tests,contrib,doc}`
- Every `*.dll *.exe *.lib *.profdata *.prx *.sprx *.elf *.log` and `eboot.bin`.
- `3rdparty/Vulkan-Headers` and `3rdparty/ffmpeg-core` are not copied: they are symlinks to Kyty's copies of the same commits.

## 7. Fetched at configure time

None.
