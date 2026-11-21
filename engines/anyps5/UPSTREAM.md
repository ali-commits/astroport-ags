# Upstream and provenance of engines/anyps5

## 1. Upstream

AnyPS5 by boykopovar: https://github.com/boykopovar/AnyPS5 (GPL-2.0-only).

## 2. Base

The base is `319621ba2e` (full hash `319621ba2e578bd053b0baba2877fe70520e0737`, committed 2026-10-29),
a commit on upstream `main`. The snapshot is the branch `linux-native`, 35 commits on top of it.

## 3. Carried changes

faycal's AstroHybrid Linux changes (https://github.com/imnotsharingmyname/pcportastrobot) and the
AstroPort changes:

```
$ git log --no-merges --reverse --format='- %h %an: %s' 319621ba2e..2429864775
- 4fcfa6a278 faycal: perf(agc): keep draw snapshots in resizable BAR memory (PR #9712 applied locally)
- 624f0a8f77 faycal: feat(agc): APS5_TRACE_MARKERS prints PUSH_MARKER paths with draw and dispatch counts per frame
- da1d520d23 faycal: perf(agc): resident staging switch, program-filtered dispatch trace, block-collect switch
- 1d381cb088 faycal: perf(agc): record DMA_DATA stores into GDS on the GPU instead of draining
- 67d70b0b59 faycal: perf(agc): plain GPU stores for GDS, sampled queue choice, CPU write-back sync counter
- bf05858826 faycal: perf(agc): bind GPU-produced draw buffers in place; CPU access trap for staged results
- 0b4fd12522 faycal: perf(agc): draws read GPU-produced buffers from the dispatch's device-local shadow
- 06d1f8941f faycal: perf(agc): GPU copy-ins read the producing dispatch's video-memory shadow
- 9f66f80d4f faycal: perf(libc): write alarms on resident guest pages instead of polling GetWriteWatch
- 3e571e1555 faycal: perf(agc): address-based builds skip publishing unit shadows over their heaps
- f189943d10 faycal: feat(agc): lane-separated recorder batches for async compute (APS5_COMPUTE_LANE)
- 0e5d40167f faycal: feat(agc): compute-lane batches on a second Vulkan queue (APS5_COMPUTE_QUEUE)
- 3792f9c5af faycal: feat(agc): profile storage traffic by resource behind an optional switch
- fb5785aaae faycal: feat(agc): support opt-in block shadows for thin linear storage images
- f4c78be33e faycal: test(agc): cover sized deallocation beyond cache limits
- 72c090368c faycal: perf(agc): wait only for completion-stored labels before resource reads (APS5_SYNC_COMPLETION_LABELS_ONLY)
- 8e64644062 faycal: feat(agc): configurable floor for the sampled texture cache budget (APS5_SAMPLED_BUDGET_FLOOR_MIB)
- 4ab160e3b3 faycal: perf(recompiler): skip call_once once a memo entry is ready
- 0a05659009 faycal: perf(agc): shed the device pool tier while video memory is over budget (APS5_POOL_SHED_OVER_BUDGET)
- ce5b4170ed faycal: perf(agc): reuse a data-only draw stage rebased to moved user-data pointers (APS5_REBASED_STAGE_REUSE)
- 803c689a71 faycal: feat(libkernel): hand guest memory to Kyty's GPU stack behind APS5_GPU_ENGINE=kyty
- fe8ecec02a faycal: feat(libAstroHybridHost): publish Kyty's window input to libScePad
- 2f05504cd1 faycal: feat(libAstroHybridHost): end the process through libc when Kyty's window closes
- a8413f36b2 faycal: feat(libScePad): trigger effect state from the game's effect and the trigger travel; pad output to Kyty (ABI 1.4)
- fcb81313ab faycal: feat(libAstroHybridHost): take pad motion sensors from Kyty (ABI 1.5)
- 87d9eb9668 faycal: feat(libc): keep a CUDA address window free in the guest arena when requested
- 9123102d58 faycal: feat(audioout,savedata): forward wireless DualSense audio to kyty_gpu and export sceSaveDataTransferringMount
- 8be0cd4d0e Claude: AstroHybrid host on Linux: libkyty_gpu.so engine, pad audio, APR staging, driver area
- e6b134da2a Ali: audioout2: on Linux, play USB DualSense haptic channels through the Kyty backend
- 7cade313aa Ali: audioout2: APS5_PAD_VIBRATION_GAIN for the host haptic channels
- 4e4b939318 Ali: fix(audioout2): hand host haptic grains to a worker thread
- 9503760288 Ali: feat(audioout2): APS5_PAD_VIBRATION_DUMP writes the mixed haptic channels to a file
- eb188dd62a Ali: feat(audiopropagation): render a source's input straight into the bed's first channel
- 5122ed27b7 Ali: fix(libkernel): wake every thread waiting on an equeue when an event fires
- 2429864775 Ali: fix(libSceAudioPropagation): accept path calculation without rays and audio path entries
```

`AGS_CONTROLLER_AUDIO_SINK` pad audio device; live haptic gain read from the renderer.

## 4. Files changed against the base

For GPL-2.0 section 2a, as of 2026-11-21. No changed file carries a licence header, so no
"Modified by" line was added to any file.

```
$ git diff --stat=200 319621ba2e 2429864775
 core/libs/prx/libAstroHybridHost/CMakeLists.txt                                   |   6 +++
 core/libs/prx/libAstroHybridHost/Export.cpp                                       | 261 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 core/libs/prx/libSceAgcDriver/CMakeLists.txt                                      |  10 ++++-
 core/libs/prx/libSceAgcDriver/Execution/include/Driver/Dispatch/DispatchCache.hpp |   2 +
 core/libs/prx/libSceAgcDriver/Execution/include/Driver/Draw/DrawCache.hpp         |   4 ++
 core/libs/prx/libSceAgcDriver/Execution/include/Driver/Driver.hpp                 |   4 +-
 core/libs/prx/libSceAgcDriver/Execution/include/VulkanDevice.hpp                  |   1 +
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Dispatch/Dispatch.cpp          |  29 ++++++++++++-
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Draw/Draw.cpp                  |  14 +++++-
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Draw/DrawCacheStatistics.cpp   |   5 +++
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Draw/DrawCapture.cpp           |   9 +++-
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Draw/DrawLookup.cpp            |  44 ++++++++++++++++++-
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Draw/DrawRelocate.cpp          |  86 ++++++++++++++++++++++++++++++++++--
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Lifecycle.cpp                  |  14 +++++-
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Packets/PacketExecution.cpp    |  94 ++++++++++++++++++++++++++++++++++++++++
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Packets/PacketMemory.cpp       |  72 +++++++++++++++++++++++++++++--
 core/libs/prx/libSceAgcDriver/Execution/src/Driver/Queues/QueueWorker.cpp         |   8 +++-
 core/libs/prx/libSceAgcDriver/Execution/src/GuestMemory.cpp                       |  23 ++++++++--
 core/libs/prx/libSceAgcDriver/Execution/src/VulkanDevice.cpp                      |  65 +++++++++++++++++++++++++---
 core/libs/prx/libSceAgcDriver/Graphics/include/BufferPool.hpp                     |   4 +-
 core/libs/prx/libSceAgcDriver/Graphics/include/Context.hpp                        |  64 ++++++++++++++++++++++++++-
 core/libs/prx/libSceAgcDriver/Graphics/include/GuestBufferMemory.hpp              |  13 ++++++
 core/libs/prx/libSceAgcDriver/Graphics/include/Recorder.hpp                       |   7 +++
 core/libs/prx/libSceAgcDriver/Graphics/include/Resources.hpp                      |   1 +
 core/libs/prx/libSceAgcDriver/Graphics/src/BufferPool.cpp                         |  25 ++++++++---
 core/libs/prx/libSceAgcDriver/Graphics/src/DeferredDestruction.cpp                |   4 ++
 core/libs/prx/libSceAgcDriver/Graphics/src/Draw.cpp                               |   3 +-
 core/libs/prx/libSceAgcDriver/Graphics/src/GuestBufferMemory.cpp                  | 210 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 core/libs/prx/libSceAgcDriver/Graphics/src/Recorder.cpp                           |  79 ++++++++++++++++++++++++++++-----
 core/libs/prx/libSceAgcDriver/Graphics/src/Resources.cpp                          |  12 ++++--
 core/libs/prx/libSceAgcDriver/Graphics/src/ShaderResources.cpp                    |  52 ++++++++++++++++++++--
 core/libs/prx/libSceAgcDriver/Graphics/src/Texture.cpp                            |  83 ++++++++++++++++++++++++++++-------
 core/libs/prx/libSceAgcDriver/tests/BufferPool.cpp                                |  80 +++++++++++++++++++++++++++++++++-
 core/libs/prx/libSceAgcDriver/tests/Graphics.cpp                                  |  22 ++++++++++
 core/libs/prx/libSceAgcDriver/tests/HostAllocator.cpp                             |   2 +-
 core/libs/prx/libSceAgcDriver/tests/Recorder.cpp                                  |  22 +++++++++-
 core/libs/prx/libSceAgcDriver/tests/execution/DrawDataHitUserWords.cpp            |  52 ++++++++++++++++++++++
 core/libs/prx/libSceAudioOut/src/AudioOut.cpp                                     |  41 +++++++++++++++++-
 core/libs/prx/libSceAudioOut/src/AudioOut2Context.cpp                             | 121 ++++++++++++++++++++++++++++++++++++++++++++++++++-
 core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.cpp                              |  39 +++++++++++++++++
 core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.hpp                              |   2 +
 core/libs/prx/libSceAudioOut/src/AudioOut2Port.cpp                                |   4 +-
 core/libs/prx/libSceAudioPropagation/AudioPropagation.hpp                         |  11 +++++
 core/libs/prx/libSceAudioPropagation/Export.cpp                                   |  33 +++++++++++---
 core/libs/prx/libScePad/Export.cpp                                                |   7 +++
 core/libs/prx/libScePad/include/PadState.hpp                                      |   3 ++
 core/libs/prx/libScePad/src/PadState.cpp                                          |  43 ++++++++++++++++++
 core/libs/prx/libSceSaveData.native/Export.cpp                                    |  17 +++++++-
 core/libs/prx/libc/include/GuestArena.hpp                                         |   9 ++++
 core/libs/prx/libc/include/WindowsMappings.hpp                                    |  88 +++++++++++++++++++++++++++++++++++++
 core/libs/prx/libc/src/GuestArena.cpp                                             | 201 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 core/libs/prx/libc/src/specifics/windows/CrashReport.cpp                          |  22 ++++++++++
 core/libs/prx/libkernel/Apr/src/Apr.cpp                                           |  27 ++++++++++--
 core/libs/prx/libkernel/DirectMemory/Export.cpp                                   | 142 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 core/libs/prx/libkernel/Equeue/Equeue.cpp                                         |   6 +--
 core/libs/tests/GuestAudioPropagation.cpp                                         |   7 ++-
 core/libs/tests/GuestEqueueEvents.cpp                                             |  31 +++++++++++++
 core/shader/recompiler/Recompiler.cpp                                             |  21 +++++++--
 docs/dev/ENVIRONMENT.md                                                           |  19 ++++++++
 59 files changed, 2279 insertions(+), 101 deletions(-)
```

## 5. Snapshot

`anyps5-hybrid-src`, branch `linux-native`, commit `2429864775` (full hash `2429864775efd7336ddd49a0332192757cbcee24`), plus 23
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
