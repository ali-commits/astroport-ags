# K2: FSR 3.1 frame generation in Kyty's own presenter

Idea K2 of `spec-upstream-ideas.md` (patch 0055), built without AMD's swapchain proxy. This file holds
the plan as built, the facts it rests on, and the status. Kyty paths are under `engines/kyty/`;
`W` is `src/graphics/presentation/window/`.

## 1. Status (2026-11-25)

**Update:** verified in game (294 of 294 frames generated, no validation errors). The product build now turns
the option on and AGS offers it as a setting (section 7, items 3 and 4). The paragraph below is the first run.


**Built and run once; no frame generated yet.** The CMake option `KYTY_FSR_FRAMEGEN` is OFF by default, and
`scripts/build-engines.sh` does not turn it on. With it OFF nothing new is fetched or compiled and no new
symbol is referenced; the hooks are inline no-ops. The ON build (`build-rewrite/kyty-fg`) links and its 11
`frame_gen_tests` pass. The one game run (Astro Bot, RTX 3090, NVIDIA 620.73, Wayland, Immediate, 60 Hz)
logged "off: frames of format A2R10G10B10UnormPack32 ..." and "300 frames, 0 generated": AMD has no BGR
10-bit format. `Generate` now hands AMD an A2B10G10R10 copy of such frames (section 2, the `format` gate);
that change is built with the option ON and OFF but not run.

| Part | State |
|---|---|
| Fetch, patch, `kyty_ffx` (3rdparty/CMakeLists.txt) | Finished. Checked: the seven SDK files compile with Kyty's global flags; the block itself was configured and built in a scratch project (section 6). |
| Shaders and `ffx_blob_table.h` (CMakeLists.txt) | Finished. Checked: the block, cut out of the real file, built all 36 modules in a scratch project. |
| `ffxCompat.h`, `ffxReflect.h`, `ffxBlobs.*` | Finished. Unit-tested (36 modules reflect; names match AMD's tables). |
| `frameGenLogic.h` | Finished. Unit-tested. |
| `frameGen.h`, `frameGen.cpp` | Finished code, compiled per file with the renderer's flags. Not run: every AMD call is unverified at run time. |
| Hooks: `swapchain.cpp`, `upscalePass.h/.cpp`, `vulkanWindow.cpp` | Finished, compiled per file with the option OFF and ON. |
| VRAM reserve (`vma.cpp`), display-change hook (`window.cpp`) | **Not done** (files outside this task's ownership): section 7. |
| Product setting, adapter key, `build-engines.sh` | Finished: section 7, items 3 and 4. Adapter tests pass; the release build with the option is not yet run. |

Stubs, all intended: without the build option `FrameGen::Requested()` logs "FSR frame generation: not built
in" once and everything else in `frameGen.h` is an empty inline; `ffxSetFrameGenerationConfigToSwapchainVK`
returns `FFX_OK` (AMD's backend stores its address, nothing calls it without the proxy); `ffxIsWave64`
answers "no".

To try it: configure Kyty with `-DKYTY_FSR_FRAMEGEN=ON` (needs network: one shallow clone), build
`kyty_renderer`, run with `ASTROPORT_FSR_FG=1` (the bridge already reads it, `bridge/src/config.rs`).

## 2. Design

- **Seven SDK files, no proxy.** `kyty_ffx` (static) holds `ffx_vk.cpp`, `ffx_frameinterpolation.cpp`,
  `ffx_opticalflow.cpp`, `ffx_assert.cpp`, `ffx_message.cpp`, `ffx_object_management.cpp`,
  `ffx_breadcrumbs_list.cpp`. Nothing of AMD's swapchain proxy, ffx-api, shader accessors, tools or CMake.
- **Presenter frame space.** Input is the game's `Presenter::Frame`; output is one more frame of the pool,
  with storage usage. For a new main frame the presenter shows the generated frame, waits half a measured
  frame interval (only without vertical sync), then shows the real frame, both through the same
  `PresentOnce` (the old body of the present loop). No extra queue, thread or swapchain: the three extra
  queues, the re-entrant queue lock and the proxy's present thread of the patch are not needed.
- **Inputs.** At the title's upscale pass (the pass DLSS replaces; same recognition and constants) depth and
  motion vectors are copied into a ring of 4 image pairs (`kSlots`) and the capture is numbered. A frame
  prepared for video out remembers the last number. The present thread finds the slot by that number.
- **One thread for AMD.** Prepare, optical flow and interpolation are recorded on the present thread's own
  command buffer, so AMD's backend (not thread-safe) needs no lock. The ring has one mutex.
- **Shaders.** The SDK's 18 GLSL compute shaders are built by Kyty's `glslangValidator` step in two variants
  each and embedded. Their binding lists are read from the SPIR-V at run time (`ffxReflect.h`), and
  `ffxBlobs.cpp` implements `ffxGetPermutationBlobByIndex` in place of AMD's generated accessors.
- **Every failure shows the real frame once**, as without the feature.

Activation: `Embed::Options::fsr_fg` (`ASTROPORT_FSR_FG=1`), in a build with the option. The low-latency
mode stays off only while frame generation is requested, built in and possible on the device
(`FrameGen::Requested() && FrameGen::DeviceSupports(...)` in `lowLatency.cpp`); a build without the option
or a device without the features keeps low latency. A generator that turns itself off later in the run
(AMD context creation failed, three AMD errors) does not bring low latency back: its extensions are chosen at
device creation. There is no DLSS frame generation on Linux for it to yield to.

Slot images (`frameGen.cpp`): a new output size or format remakes the ring. The old images go through the
guest scheduler's deferred operations (the guest's command buffers are through the GPU), then wait until
`Generator::Prepare` has waited for the presenter's own command buffers, and are deleted at the next
`Capture`. A second size within one title frame is refused, not remade. A size whose images could not be
made (video memory full) is refused until the size changes. `~Generator` deletes no image; `FrameGen::Shutdown`
does, called by `~WindowContext` after the render context is gone.

### Per frame (present thread, `Presenter::Impl::Present`)

1. `Generator::Prepare`: measures the interval; decides the gate; makes or remakes AMD's contexts for
   {frame size, frame format, HDR, slot size}.
2. The generated frame: taken from the pool once, `Configure(..., eStorage)`.
3. `Generator::Record` under the ring's mutex: `ffxFrameInterpolationPrepare` (slot depth and motion),
   `ffxOpticalflowContextDispatch` (the frame), `ffxFrameInterpolationDispatch` (frame -> generated frame).
4. Submit; `PresentOnce(generated)`; `Generator::Pace`; `PresentOnce(real)`.

Outcomes, counted per new main frame and logged every 10 s: `generated`, `primed` (dispatched with a
reset: only stores history), `no inputs` (no new capture for this frame), `lost` (the slot was overwritten
or the ring remade), `blank`, `display`, `format`, `error`.

Gates (the decision is logged when it changes):

- `display`: with FIFO the display must refresh at least twice per game frame
  (`refresh_mhz + 1000 >= 2000 * limit`); an unknown refresh rate under FIFO refuses, because a 60 Hz output
  would halve the game. Without FIFO it always runs and warns once when the display is slower than that.
- `format`: frame format not one of R8G8B8A8, B8G8R8A8, A2B10G10R10, R16G16B16A16F, B10G11R11F, or not
  usable as a storage image; or the device lacks `shaderStorageImageReadWithoutFormat`,
  `shaderStorageImageExtendedFormats` or subgroup arithmetic and quad operations in compute with
  `quadOperationsInAllStages` (the shaders' capabilities; `FrameGen::DeviceSupports`).
  A2R10G10B10 (BGR) frames are blitted to an A2B10G10R10 copy that AMD reads (+1 blit per real frame,
  +1 frame of memory); the generated frame is then A2B10G10R10.
- `error`: context creation failed (off for the run) or three AMD errors in a row.

Continuity: AMD's frame number goes up by one per dispatch. A dispatch resets the history when the new
main frame before it was not dispatched or the title's frame counter is not the previous plus one.

## 3. Facts from the study

SDK: **FidelityFX SDK v1.1.4**, https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK, tag `v1.1.4` =
commit `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55` (`git ls-remote`, 2026-11-25). Licence: MIT,
"Copyright (C) 2024 Advanced Micro Devices, Inc." (`LICENSE.txt`). Compatible with GPL-2.0; the notice is in
`NOTICE.md` section 5, which every package carries as `licenses/NOTICE.md`. Kyty's own `cmake --install`
also writes `licenses/fidelityfx-sdk/LICENSE.txt`, but the release staging (`scripts/release/stage.sh`) does
not run it.

Line numbers below are of that commit, under `sdk/`.

| Fact | Where | Consequence |
|---|---|---|
| The contexts are fixed arrays of `FFX_SDK_DEFAULT_CONTEXT_SIZE` (1024*128) `uint32_t`; with a 32-bit `wchar_t` the private frame-interpolation context is 768072 bytes against 524288 (static assert). | `include/FidelityFX/host/ffx_types.h:80`, `ffx_frameinterpolation.cpp:688` | One-line patch to 1024*256 (`3rdparty/patches/fidelityfx-context-size.patch`); no `-fshort-wchar`. |
| Outside `_WIN32` blocks the seven files need only `_countof`, `wcscpy_s`, `strcpy_s`, `sprintf_s`, `FFX_UNUSED`. | compile errors without them | `W/ffxCompat.h`, force-included into the SDK sources only. `-fdeclspec` is not needed (`FFX_API` is empty unless `FFX_BUILD_AS_DLL`). |
| The seven objects leave undefined, besides libc/libm/pthread/libstdc++: `ffxGetPermutationBlobByIndex`, `ffxIsWave64`, `ffxSetFrameGenerationConfigToSwapchainVK`, and eight Vulkan globals: `vkCreateBuffer`, `vkEnumerateDeviceExtensionProperties`, `vkGetDeviceProcAddr`, `vkGetPhysicalDeviceFeatures`, `vkGetPhysicalDeviceFeatures2`, `vkGetPhysicalDeviceMemoryProperties`, `vkGetPhysicalDeviceProperties`, `vkGetPhysicalDeviceProperties2`. | `nm -u` over the objects (this session) | The first two in `ffxBlobs.cpp`. The stub and eight hidden forwarders to Kyty's dispatch table in `frameGen.cpp`: Kyty links no Vulkan loader, and the link uses `--no-undefined`. |
| The backend takes the device's function lookup from the caller and asks for some functions by `...KHR` names it then calls unchecked. | `backends/vk/ffx_vk.cpp:1487` | `DeviceFunction` retries without the suffix. |
| An SRV view of a registered image gets its format from the description's format and usage: `R32_FLOAT` + `DEPTHTARGET` -> `D32_SFLOAT`, + `STENCILTARGET` -> `D32_SFLOAT_S8_UINT`; aspect depth. | `ffx_vk.cpp:771-818`, `2600-2760` | The depth slot is described `R32_FLOAT` with `DEPTHTARGET`, plus `STENCILTARGET` when the slot is D32S8. |
| Registered resources are walked back to the state they came in with. | `ffx_vk.cpp:2813` | Frame and slots are handed over in `eShaderReadOnlyOptimal` (`COMPUTE_READ`), the output in `eGeneral` (`UNORDERED_ACCESS`), and stay so. |
| AMD matches a shader's bindings by the variable's name (`wcscmp`); samplers are bound by the backend at binding 1000 + index, not from the shader's list. | `ffx_frameinterpolation.cpp:152-216`, `ffx_vk.cpp:3233-3340` | `ffxReflect.h` lists names from `OpName`; they survive `-Os`. |
| The entry point is `main`. | `ffx_vk.cpp` (`pName = "main"`), the GLSL files | `glslangValidator` default entry. |
| Shader options: frame interpolation `LOW_RES_MOTION_VECTORS`, `JITTER_MOTION_VECTORS`, `INVERTED_DEPTH`; optical flow `HDR_COLOR_INPUT`; plus five fixed defines. | `include/FidelityFX/gpu/*/CMakeCompile*Shaders.txt` | Built: 11 passes x `INVERTED_DEPTH` {0,1} with the first two fixed to 1, and 7 passes x `HDR_COLOR_INPUT` {0,1}: 36 modules. The 16-bit and 64-wide-wave variants are not built; requests for them get the plain module. |
| Capabilities of the 36 modules: `StorageImageWriteWithoutFormat` (6), `StorageImageReadWithoutFormat` (2, the inpainting pass), `GroupNonUniform` (8), `GroupNonUniformQuad` (6), `GroupNonUniformArithmetic` (2), `StorageImageExtendedFormats` (16). | `spirv-dis` over the modules | Kyty requires write-without-format already and enables read-without-format when the device has it; `VulkanCreateDevice` enables `shaderStorageImageExtendedFormats` when frame generation is requested and the device has it; the rest is the `format` gate. |
| `ffxPrintMessage` does nothing outside Windows. | `src/shared/ffx_message.cpp:59-62` | No message callback is installed; errors come as return codes. |
| The first dispatch of a context, a `reset`, or a frame number that does not go up by one resets the history; a reset dispatch still writes the output and stores the frame. | `ffx_frameinterpolation.cpp:920-945` | `Continuity`; a reset dispatch counts as `primed` and its output is not shown. |
| Two backend interfaces (1 context for shared resources, 2 for the effects), a shared backend context, then optical flow, frame interpolation and five shared resources; destroyed in the reverse order. | `ffx-api/src/ffx_provider_framegeneration.cpp:88-205` (read only) | `Generator::Impl::Create` / `Destroy`. One set of the three frame-interpolation resources instead of AMD's two: prepare and dispatch are in one command buffer. |
| Kyty's device has no `separateDepthStencilLayouts`; AMD's barriers for a depth resource name the depth aspect only. | `ffx_vk.cpp:762-768`, `vulkanWindow.cpp` | With a D32S8 depth target the validation layer will object. Section 8. |
| The adapter refuses a launch (`DAMAGED_INSTALLATION`) for a key of `patches/settings.json` that it neither sets itself nor finds a page default for. | `adapter/src/launch.rs:154-175` | The setting cannot be added to `settings.json` alone. Section 7. |

## 4. Files

| File | Lines | What |
|---|---|---|
| `3rdparty/CMakeLists.txt` | +56 | fetch (tag, shallow, commit checked), patch, `kyty_ffx`, licence install |
| `3rdparty/patches/fidelityfx-context-size.patch` | new | the context size |
| `3rdparty/patches/fidelityfx-scratch-align.patch` | new | the Vulkan backend's scratch arrays 16-byte aligned (clang's aligned stores crashed the first context) |
| `3rdparty/patches/fidelityfx-descriptor-pool.patch` | new | the backend's descriptor pool counts all six pool sizes (it said 5 and left out the storage buffers; validation warning) |
| `CMakeLists.txt` | +93 | the option; the shader block (between `# BEGIN/END KYTY_FSR_FRAMEGEN`), which removes `frameGen.cpp` and `ffxBlobs.cpp` from the sources when OFF; `frame_gen_tests` |
| `W/ffxCompat.h` | new | MSVC names for the SDK sources |
| `W/ffxReflect.h` | new | SPIR-V binding reader (pure) |
| `W/ffxBlobs.h`, `W/ffxBlobs.cpp` | new | module table, `ffxGetPermutationBlobByIndex`, `ffxIsWave64` |
| `W/frameGenLogic.h` | new | `Pacer`, `DisplayAllows`, `Continuity`, `Find`, `FrameTimeMs`, `Stats` (pure) |
| `W/frameGen.h`, `W/frameGen.cpp` | new | the ring, `Capture`, `Generator`, the Vulkan forwarders and the proxy stub |
| `W/swapchain.cpp` | +136 -22 | `Frame::capture`, `Configure(extra)`, `Swapchain::PresentMode`, `PresentOnce`, `Generate` (with `converted`, the A2B10G10R10 copy of an A2R10G10B10 frame), `new_main` |
| `W/vulkanWindow.cpp` | +11 -1 | `FrameGen::NoteDisplay` at window creation; enables `shaderStorageImageExtendedFormats` at device creation when requested (only with the option); the physical device for `LowLatency::AppendExtensions`; `FrameGen::Shutdown` in `~WindowContext` |
| `W/lowLatency.h`, `W/lowLatency.cpp` | +5 -2 | off for frame generation only when it is built in and the device can run it |
| `src/graphics/host_gpu/renderer/upscalePass.h` | +9 -2 | `Enabled()` also for frame generation |
| `src/graphics/host_gpu/renderer/upscalePass.cpp` | +41 -6 | the image check split; the capture before the DLSS part (with the guest scheduler, for the ring's deferred deletes); the DLSS line only with DLSS |
| `tests/FrameGenTests.cpp` | new | 9 tests, 11 with the option |

Not changed, against the plan: `videoOut.cpp`, `render.h`, `src/embed/` and `bridge/` (the option and the
variable exist already), `window.cpp`, `windowInternal.h`, `vma.cpp` (section 7).

## 5. Differences from the plan

- The option is named `KYTY_FSR_FRAMEGEN` (the plan said `KYTY_FFX`), also as the compile definition.
- `Generator` is split into `Prepare` / `Record` / `Pace`, and the presenter keeps the frame handling
  (`Presenter::Frame` is private to `swapchain.cpp`): the generator never sees a `Frame`.
- The Vulkan forwarders and the proxy stub are in `frameGen.cpp`, not `ffxBlobs.cpp`, so that the test can
  link `ffxBlobs.cpp` without Kyty's Vulkan table.
- The reflection is its own header (`ffxReflect.h`), so the test of it builds without the option.
- No message callback (AMD's is compiled out off Windows).
- No `-fdeclspec`.
- `FrameGen::Capturing()`: the upscale pass copies its images only while the generator can use them.
- Refresh rate: read once at window creation in `vulkanWindow.cpp` instead of a `WindowContext` field
  updated by `window.cpp` events (ownership; section 7).

## 6. What was checked, and how

All in this session, on the development PC, without the game and without a build of Kyty:

- SDK tag: `git ls-remote` gives `c6efa6bf...` for `refs/tags/v1.1.4`; a sparse clone of the tag has that
  HEAD and the patch applies to it (`git apply --ignore-whitespace --check`).
- The seven SDK files: compiled one by one with clang, once with plain flags and once with Kyty's global
  flags (`-fno-rtti -fno-exceptions ...`), with `ffxCompat.h` and the patched header. `nm -u` as in section 3;
  together with the objects of `frameGen.cpp` and `ffxBlobs.cpp` no `ffx*` or `vk*` symbol stays undefined.
- The shader block of `CMakeLists.txt`, cut out between its markers into a scratch CMake project: with
  the option ON it built 36 headers and `ffx_blob_table.h` (`ninja -j2`); with OFF it leaves no header and
  removes the two sources from the list.
- The fetch block of `3rdparty/CMakeLists.txt`, cut out into a scratch CMake project and run: see the
  note at the end of this section.
- `frameGen.cpp`, `ffxBlobs.cpp` (option ON) and `swapchain.cpp`, `upscalePass.cpp`, `vulkanWindow.cpp`
  (option OFF and ON): compiled per file with the flags of the `kyty_renderer` target from `build.ninja`.
  With OFF their objects reference no `FrameGen` symbol (`nm -u`). `tests/DlssTests.cpp` (includes
  `upscalePass.h`): syntax check.
- `frame_gen_tests`: built by hand against the existing `libkyty_test_gtest.a`; 9 tests pass with the option
  OFF, 11 with ON (the 36 modules).

Not checked: a configure of `engines/kyty` itself with either value of the option; any link; any run.

The fetch block of `3rdparty/CMakeLists.txt` was cut out into a scratch CMake project and run for real
(one shallow clone of the official repository into the scratch folder): configure took 64 s, HEAD was
`c6efa6bf...`, the patch was applied, and `ninja -j2 kyty_ffx` built `libkyty_ffx.a` with the undefined
symbols of section 3. **The clone is 972 MiB on disk** (542 MiB of it `.git`): see open item 6.

## 7. Open items

1. **VRAM reserve** (spec: a second reserve while the feature is requested). `src/graphics/host_gpu/vma.cpp`,
   `GraphicContext::GetTotalMemoryBudget`, discrete branch: subtract the reserve twice when
   `FrameGen::Requested()`. Three lines; the file was outside this task. AMD's allocations (reported in the
   log line as "AMD video memory") are outside VMA and outside the caches' budget until then.
2. **Refresh rate after a display change.** `W/window.cpp`, the handler of
   `SDL_EVENT_WINDOW_DISPLAY_CHANGED`: call `FrameGen::NoteDisplay(window)`. Until then a window moved to a
   display with another refresh rate keeps the first value (only matters under FIFO).
3. **Product setting: done.** AGS's standard key `perf.frame_generation` (AGS `docs/extensions/STANDARD.md`
   6.1: `off`, `2x`, `3x`, `4x`, optional `method`). `adapter/src/sheet.rs` declares it honoured, applier
   source, next launch, values `off` and `2x`, method `fsr`, local play only; the key takes no source default,
   so AGS's registry default `off` holds. `adapter/src/launch.rs` reads it (`3x`/`4x` -> `2x`, anything else
   -> `off`) and `patches/settings.json` maps `2x` -> `ASTROPORT_FSR_FG=1`, `off` -> `0`. A source row
   replaces AGS's own lsfg row for the key, so AGS no longer injects lsfg-vk for Astro Bot; the player's own
   lsfg-vk profiles still apply while the setting is Off. The game keeps its frame cap and Kyty presents one generated frame per real one: 60 fps shows 120, 30 fps shows 60. With vertical sync the display must refresh at least twice the cap, or generation stays off (a 60 Hz display at 60 fps). Low latency is off while it is on. Because the game declares the key, AGS does not inject its own lsfg-vk for it; lsfg-vk profiles the player set up outside AGS still apply when the setting is Off.
4. **Build switch: done.** `scripts/build-engines.sh` configures Kyty with `-DKYTY_FSR_FRAMEGEN=ON`; the
   release container fetches the SDK at configure time, like Kyty's other dependencies (it has git and network).
5. **design.md** (6.3 K2, 7.1 `ASTROPORT_FSR_DIR` is unused by this design, 10 decision 1) and section 4 of
   `engines/kyty/UPSTREAM.md` (the file list is made with `git diff` at commit time).
6. **Size of the fetch.** The shallow clone of the tag is 972 MiB on disk (the SDK's samples, media and
   prebuilt binaries come with it), for about 6 MiB of sources that are used. Before the option is turned
   on in the product build, replace the clone by a sparse checkout of `sdk/` done by a small script, or by
   a pinned archive with a hash, as Noto Sans is fetched.

## 8. Needs the game (one full build, one run)

Go/no-go, in this order; stop and leave the option OFF at the first that fails:

1. Configure with `-DKYTY_FSR_FRAMEGEN=ON`: the clone, the patch and the commit check.
2. Link of `libkyty_renderer.so` with `--no-undefined`: proves the compat header, the blob entry points and
   the Vulkan forwarders complete, and that the hidden `vk*` names clash with nothing.
3. Log: "FSR frame generation: requested ...", "contexts for frames of ... made in N ms", then every 10 s
   `captures` and `generated` counting; with DLSS on and off.
4. A run with the validation layer: the depth slot view (D32 or D32S8), the storage view of the output,
   layouts across the two threads, AMD's depth-aspect barrier on a D32S8 slot. If the title's depth target is
   D32S8 and the layer objects, copy depth through a buffer into an R32F slot instead.

Then:

- Picture: halos at disocclusions with the fixed camera values (0.1, 1000, 1 rad), HUD smear (no HUD-less
  colour is captured), jitter sign, depth direction (`ASTROPORT_DLSS_REVERSE_DEPTH`).
- `lost` and `no inputs` in normal play (raise `kSlots` if `lost` is above zero).
- Pacing: Wayland IMMEDIATE (tearing), Wayland FIFO at 120 and 144 Hz, Wayland MAILBOX, X11 IMMEDIATE; a
  60 Hz output stays off under FIFO; with and without `ASTROPORT_FPS_LIMIT`.
- The half-frame wait inside the flip: its effect on guest flip and vblank events.
- HDR switch, resize, minimise and restore, a session stream while generating; video memory at 4K.
- An output-size change in play: the log's AMD and Kyty video memory do not grow with each change, no
  validation message about a destroyed image, and a clean exit right after one.
- The frame counter the window title and the session socket show: it now counts both presents.
- RADV and ANV: not available on this machine.

## 9. Not built, by decision

HUD-less colour capture, the camera from the title's matrices, a jitter-sign option, the debug view,
asynchronous interpolation, a runtime folder variable, any other new variable. Add one only when the game
test shows the need.
