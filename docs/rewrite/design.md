# AstroPort bridge: final design

Status: milestones M0 to M4 are built and run (section 8); the game starts on the bridge and reaches
the title level. For those parts the text states the system as built; everything from M5 on is still
design. Base: the "thin-seams" proposal (both judges chose it), with the grafts the judges named and
the facts established separately.

Every upstream path, function and line number in this document was checked by grep or by reading the
pristine trees (`engines/kyty` at `dd9f9901`, `engines/anyps5` at `319621ba2e`) on 2026-11-25, unless
it carries **[unverified]**. Each unverified item names the milestone whose first check settles it.

Rule of the design: logic that needs no engine-internal C++ type lives in one Rust library. Each
engine gets one new file group with a plain C function table, and as few edits to upstream files as
possible. Hooks in upstream files read their options from one header-only file, so the stand-alone
emulator and the upstream tests still build and link.

---

## 0. Findings that shape the design (all verified in source)

| # | Finding | Consequence |
|---|---|---|
| 1 | Kyty's Linux `ReserveGuestRegions()` (`src/kernel/memoryAddressSpace.inc:2177`) reads `/proc/self/maps` and reserves every free gap of three regions (`0x40000..0x7FFFFBFFF`, `0x7FFFFC000..0xFFFFFFFFF`, `0x1000000000..HOST_USER_MAX`) with `MAP_FIXED_NOREPLACE`. A gap it cannot take is skipped silently. | Donation = the host unmaps a range of AnyPS5's `PROT_NONE` arena before Kyty's memory starts. Kyty adopts it by itself. **No edit in `memory.cpp`, `memory.h`, `memoryAddressSpace.inc`.** It can never map over a live host mapping. |
| 2 | `HOST_USER_ALLOC_END = kAmmWindowBase = 0xEC00000000` (`memoryAddressSpace.inc:17`, `memory.h:61`); ordinary searches stop there. | The general donation is fixed at `0xDC00000000..0xEC00000000`. No search-bound patch (spec-kyty 3.6 is not needed). |
| 3 | AnyPS5's arena (`core/libs/prx/libc/src/GuestArena.cpp`) reserves `0x10000..0xFC00000000` with `MAP_FIXED_NOREPLACE` and records, but does not map, the system-reserved range. It already exports `GuestArenaNextUsed_nid_no_patch`, `GuestArenaMarkUsed_nid_postfix` (throws on overlap), `GuestArenaRange_nid_postfix`. | Donation bookkeeping needs **no libc change**. |
| 4 | Kyty HLE functions are plain `KYTY_SYSV_ABI` functions declared in headers (`src/libs/controller.h:128-144`, `src/kernel/eventQueue.h:103-109`) and registered by `LIB_FUNC("<nid>", fn)` (`src/libs/libs.h:49`). `libPad.cpp` registers "wheel aware" wrappers, not the `Controller::` functions. | Graphics and memory exports are bound by NID (`SymbolDatabase::FindByNid`). Pad and event-queue helpers the bridge calls itself are handed over as **direct function addresses**, not by NID, so the wheel library (never started) is not involved. |
| 5 | Upstream `Emulator::Run` (`src/emulator.cpp:168-209`) calls `Libs::Graphics::SetPipelineListExecutable`, `Loader::SystemContentLoadParamSfo` (line 106), starts Memory (line 122) before Graphics (line 126), and registers `Common::Subsystems::EmergencyShutdownActive` with `at_quick_exit` (line 199). | Cache title id (2.16), K3 and K4 become "the seam does what upstream Run does". Shutdown is upstream's emergency flush. Address space is reserved before any Vulkan library is loaded. |
| 6 | Upstream has `Libs::Graphics::Dlss` (`src/graphics/host_gpu/renderer/dlss.h/.cpp`). It loads the driver's `libnvidia-ngx.so.1` (`dlss.cpp:92`) with a hand-declared ABI and is already called from `vulkanWindow.cpp:2218, 2223`. | DLSS needs no NVIDIA header or library at build time. We write only the title hook and three small gaps (6.4). |
| 7 | `Common::HostException::InstallFirstAccessHandler` is a stub on Linux (`src/common/hostException.cpp:340`). AnyPS5's write watch chains to the previous action (`GuestWriteWatch.cpp:541-557`). | The fault handler is Rust `sigaction` in the bridge. Kyty installs nothing. |
| 8 | Missing from the "pristine plus fixes" trees (grep): `KYTY_WAYLAND_NATIVE`, the DualSense switches, any `Streaming::Transport` construction, the frame exporter, `AGS_CONTROLLER_AUDIO_SINK`, `APS5_PAD_VIBRATION_*`. Present: `src/streaming/{protocol,transport,metrics*,sessionEnv}`, `PresentedFrames()` in `window.cpp:163`, the two audio-propagation fixes and the equeue fix (per the reset commit message). | Our own earlier engine features must be written again from the spec text (section 10, decision 3). |

---

## 1. Components and languages

### 1.1 Artifacts in the game process

| Artifact | Language | Built by | Exports | DT_NEEDED |
|---|---|---|---|---|
| `libkyty_renderer.so` | C++ (whole Kyty object set minus `main.cpp`, plus `src/embed/kytyEmbed.cpp`) | CMake target `kyty_renderer` in `engines/kyty/CMakeLists.txt` | exactly `kyty_embed_api` | system libraries only |
| `libAstroportHost.prx` | C++, AnyPS5 conventions | AnyPS5 CMake, new directory `core/libs/prx/libAstroportHost/` | `AstroportHostApi_nid_no_patch`; the two HDR helpers under their NIDs | `libScePad.prx`, `libkernel.prx`, `libc.prx` |
| `libastroport_bridge.so` | Rust (`bridge/`, cdylib) | cargo | `astroport_entry_<hash8>` only | `libkyty_renderer.so`, `libAstroportHost.prx` |
| `libSceAgc.prx`, `libSceAgcDriver.prx`, `libSceVideoOut.prx` | generated x86-64 assembly (generator is Rust) | `cc -shared -nostdlib` in `scripts/build-engines.sh` | one symbol per NID of that library (148, 16 and 36 in the tree) | `libastroport_bridge.so`, by file name: the bridge has no soname, so each forwarder is linked with `-l:libastroport_bridge.so` from the folder that holds it and finds it at run time through its `$ORIGIN` run path |
| `libastro_patcher.so` | Rust (`patcher/`, unchanged) | cargo | unchanged | - |

Three real forwarder files, not symlinks: the release stage test forbids symlinks, and it removes one
unverified glibc behaviour.

Why assembly forwarders: NIDs contain `+` and `-` and may start with a digit (`23LRUSvYu1M`,
`-Ozn0F1AFRg`). A Rust cdylib cannot export such names. A two-instruction thunk in a file we assemble
ourselves has no such problem and holds no logic.

Load chain, all by `DT_NEEDED`, so the loader resolves everything before any guest code runs and the
renderer's large TLS block is static:

```
AstroBot -> libSceAgc.prx (forwarder) -> libastroport_bridge.so -> libkyty_renderer.so
                                                                 -> libAstroportHost.prx -> libScePad.prx, libkernel.prx, libc.prx
```

The static TLS block holds only while the renderer is loaded at program start. Two cases need
`GLIBC_TUNABLES=glibc.rtld.optional_static_tls=<bytes>` (the block is about 600 KB): a process started
with `LD_AUDIT` (the patcher), because glibc then sizes the static TLS area before it loads the
program's libraries; and stand-alone loading of `libkyty_renderer.so` with `dlopen`
(`bridge/examples/embed_smoke.rs`). See 7.3.

AnyPS5's own `libSceAgc`, `libSceAgcDriver`, `libSceVideoOut` are still built (`libSceJobManager`
links against them) and stay in AnyPS5's library folder; the runner does not copy them into a run
folder (7.4). All libraries of a run folder sit in one `libs/` folder, where the forwarders carry
those three file names.

### 1.2 Tools

| Tool | Language | Purpose |
|---|---|---|
| `bridge/src/bin/nidgen.rs` | Rust | Scans Kyty's `LIB_FUNC` lines; writes three `.S` files and `astroport-exports.txt` (index, NID, library, Kyty function). |
| `runner` (existing) | Rust | New: installs the libraries; checks the game's graphics imports against `astroport-exports.txt` (4.2). |
| `scripts/smoke.sh` (new, ours) | shell | Runs the game windowed under `timeout` with `DISABLE_LSFGVK=1`, then greps the log for a pattern. Every run-based milestone check uses it. |

No Python build scripts. The old `tools/hybrid/*.py` do not exist in the pristine tree and are not
re-created.

### 1.3 Cargo layout

No workspace. The three existing crates have their own `Cargo.lock`, profile and `target/`, and
`ags-extension.toml` and `scripts/release/stage.sh` name those paths. A fourth independent crate:

```
bridge/
  Cargo.toml       name = "astroport-bridge"; [lib] crate-type = ["cdylib", "rlib"]; deps: libc
                   [profile.release] opt-level = 2, panic = "abort"
  build.rs         runs the scanner on engines/kyty/src/libs -> OUT_DIR/nids.rs (table + hash8);
                   if ASTROPORT_ENGINE_LIBS is set: -L, --no-as-needed, -l:libkyty_renderer.so,
                   -l:libAstroportHost.prx, -rpath $ORIGIN (cdylib only)
  nidscan.rs       LIB_VERSION / LIB_FUNC scanner; included by build.rs and bin/nidgen.rs via #[path]
  src/lib.rs       constructor, start state, shutdown callback
  src/abi.rs       #[repr(C)] mirrors of the engine tables and PS5 structs, with size asserts
  src/entry.rs     naked astroport_entry, wait stub, slot array (200 exports + 17 memory), 17 memory thunks
  src/config.rs    environment -> Config (pure; an invalid value is an error naming the variable)
  src/memory.rs    donation plan, owned-range list, owns(), adoption check
  src/fault.rs     SIGSEGV / SIGBUS action and chaining
  src/events.rs    three event exports, proxy queue, pump thread
  src/pad.rs       relay thread: input, motion, output differences
  src/limiter.rs   frame limiter and the video override (pure)
  src/audio.rs     pad audio and haptic gain entries
  src/live.rs      live settings: JSON apply -> actions (pure part tested)
  src/bin/nidgen.rs
  tests/           thunk ABI test, fake-engine tests (start, fault, cdylib, forwarders)
  examples/embed_smoke.rs   loads libkyty_renderer.so alone (M1)
```

The bridge finds both engines with `dlsym(RTLD_DEFAULT, ...)` for one table-getter symbol each, so
`cargo test` needs neither engine: tests pass fake tables. Nothing is compiled out under test. A
process in which neither symbol exists (a test, a tool) is not a game process: the constructor
returns without doing anything. Exactly one of the two missing is a start-up error.

### 1.4 Build (`scripts/build-engines.sh`, rewritten in place)

`B` is `$BUILD_DIR` (default `build`); `JOBS=<n>` is passed to ninja and cargo as `-j<n>`.

```
cmake -S engines/kyty -B $B/kyty -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo \
      -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC -DKYTY_BUILD_LAUNCHER=OFF
ninja -C $B/kyty kyty_renderer

cmake -S engines/anyps5 -B $B/anyps5 -G Ninja -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++
xargs -a scripts/prx-targets.txt ninja -C $B/anyps5   # libAstroportHost.prx is lines 1 and 371
ninja -C $B/anyps5 relinker

mkdir -p $B/libs $B/fwd
ln -sf ../kyty/libkyty_renderer.so ../anyps5/core/libs/libs/libAstroportHost.prx $B/libs/
ASTROPORT_ENGINE_LIBS=$B/libs cargo build --release --locked --manifest-path bridge/Cargo.toml
cp bridge/target/release/libastroport_bridge.so $B/libs/
bridge/target/release/nidgen engines/kyty/src/libs $B/fwd
for l in libSceAgc libSceAgcDriver libSceVideoOut; do
  cc -shared -nostdlib -Wl,-z,now -Wl,-soname,$l.prx -Wl,-rpath,'$ORIGIN' \
     -o $B/fwd/$l.prx $B/fwd/$l.S -L$B/libs -l:libastroport_bridge.so
done
```

`-fPIC` goes through the compiler flags, not `CMAKE_POSITION_INDEPENDENT_CODE`, because only the flags
also reach Kyty's third-party builds. The bridge has no soname, so a forwarder linked against it by
path would record that build path as its `DT_NEEDED`; `-l:libastroport_bridge.so` records the bare
file name.

AnyPS5's tests are not part of this build: they exist only in a tree configured with
`-DBUILD_TESTING=ON` (the option is off by default), and the M2 cases are registered under it.

Then three checks, a few lines of shell with `nm -D` each, which fail the script:

1. `libkyty_renderer.so` defines exactly `kyty_embed_api`.
2. Every symbol that `libSceJobManager.prx` and `libSceSystemService.prx` import and that AnyPS5's own
   three graphics libraries define is defined by a forwarder or by `libAstroportHost.prx`.
3. The `astroport_entry_<hash8>` the forwarders import is the one the bridge defines.

Last, the script writes the install tree `$B/install`, made of links into the build folder; it is
what `astroport-run --install` reads, and the script installs nothing into a run folder:

```
install/lib/            libkyty_renderer.so, libastroport_bridge.so, the three forwarders,
                        astroport-exports.txt (and the DLSS runtime when present)
install/anyps5/libs     -> AnyPS5's library folder (all .prx, including its own three graphics
                           libraries and libAstroportHost.prx)
install/anyps5/relinker
install/patches, install/ags-extension.toml
```

`install/lib` and `install/anyps5/libs` both hold files named `libSceAgc.prx`, `libSceAgcDriver.prx`
and `libSceVideoOut.prx`; in a run folder the ones from `install/lib` must be the ones present.

`ags-extension.toml` build list is unchanged (the bridge is built inside `build-engines.sh` because
the forwarder link needs it). `python3` leaves `depends` only if AnyPS5's own build does not need it.

### 1.5 The renderer target

`engines/kyty/CMakeLists.txt`, after the Linux block that follows the `kyty_emulator` target, 12 lines.
It reuses `kyty_emulator_src` (line 289), `kyty_emulator_link_libraries` (645), `inc_headers` (653)
and `kyty_cpu_check` (1015); `main.cpp` is not in the source list, and `src/embed/` is not in its
globs, so nothing has to be excluded and `kyty_emulator` is unaffected:

```cmake
if(LINUX)
  add_library(kyty_renderer SHARED EXCLUDE_FROM_ALL ${kyty_emulator_src}
              "${KYTY_SOURCE_DIR}/embed/kytyEmbed.cpp")
  target_sources(kyty_renderer PRIVATE $<TARGET_OBJECTS:kyty_cpu_check>)
  target_link_libraries(kyty_renderer PRIVATE ${kyty_emulator_link_libraries})
  target_include_directories(kyty_renderer PRIVATE ${inc_headers})
  target_link_options(kyty_renderer PRIVATE -Wl,-Bsymbolic -Wl,--no-undefined
      "-Wl,--version-script=${KYTY_SOURCE_DIR}/embed/kytyEmbed.map")
endif()
```

`kytyEmbed.map` is `{ global: kyty_embed_api; local: *; };`. The whole emulator object set is linked
(spec-kyty 1.1 allows it).

---

## 2. The engine seams

### 2.1 Kyty: `src/embed/`

| New file | Lines | Contents |
|---|---|---|
| `kytyEmbed.h` | 80 | The C table of section 3.1. |
| `kytyEmbed.cpp` | 291 | Implementation (below). Only in `kyty_renderer`. |
| `embedOptions.h` | 69 | Header-only: `namespace Embed { struct Options; inline Options& GetOptions(); }` with inline storage, plus the hook pointers and inline helpers `HasPresentGate`, `PresentGate`, `Presented`, `FrameLimit`. Times are `Common::SteadyNs` ticks (`TICKS_PER_S` = 10^9). All-default in the stand-alone emulator, so hooks in upstream files are no-ops there. |
| `kytyEmbed.map` | 1 | Version script. |

What each table entry wraps:

| Entry | Upstream code it calls |
|---|---|
| `run(config, hooks)` | Runs on the bridge's renderer thread and mirrors `Emulator::Run` + `Init` (`src/emulator.cpp:98-209`) without what the host owns. In order: `Common::InitializeThreads()`; fill `Config::ConfigOptions` (`screen_width`, `screen_height`, `fullscreen_enabled`, `hide_cursor_enabled`, `present_mode`, `shader_optimization_type`, `vblank_frequency`, `controller_vibration_intensity`, `dlss_folder`, `dlss_preset`, `splash_enabled = false`; `src/common/emulatorConfig.h:57-98`); copy the rest into `Embed::GetOptions()`; `Libs::Graphics::SetPipelineListExecutable(app0/eboot.bin)`; a heap-allocated `Common::Subsystems` that is never destroyed; `Initialize<Config::Lifecycle>()`, `Config::Load`, `Initialize<Log::Lifecycle>()`; `Loader::SystemContentLoadParamSfo(app0/sce_sys/param.json)` when the file exists; `Libs::LibKernel::PthreadLifecycle`, `Profiler::Lifecycle`, `Libs::LibKernel::Memory::Lifecycle`; **`hooks->reserved()`**; `Libs::LibKernel::FileSystem::Lifecycle` and `FileSystem::Mount(app0, "/app0")` (the pipeline journal reads the boot level from `/app0/args.txt` through Kyty's mounts); `Libs::Controller::Lifecycle`, `Libs::Graphics::Lifecycle`; `Libs::InitAgcDriver_1`, `Libs::LibGen5::InitVideoOut_1`, `InitLibKernel_1_Mem` (`src/libs/libKernel.cpp:3240`) on a static `Loader::SymbolDatabase`; the session socket client when `AGS_SESSION_SOCKET` is set (6.5); **`hooks->ready()`**; `Libs::Graphics::WindowRun()`. Return codes: 0 when the window loop ended; 1, before anything is started, for a null or too small `config` or `hooks`, a null `app0_dir`, or a second call (one renderer per process). Not started: `Libs::Network`, `Audio`, `Wheel` lifecycles. |
| `find(nid)` | `symbols.FindByNid(nid, Loader::SymbolType::Func)->vaddr` (`src/loader/symbolDatabase.h:46`), or null. |
| `fault(address, access, instruction)` | The live part of `TryHandleGuestAccessFault` (`src/loader/runtimeLinker.cpp:744-837`): `Libs::Graphics::FaultCost::SetFaultInstruction` when `FaultCost::MapEnabled()` or `AttribEnabled()` (line 781), then `Libs::LibKernel::Memory::HandleGpuFault(access, address)` (`src/kernel/memory.h:254`, `noexcept`) inside a `FaultCost::RetriedFaultScope`. When that does not handle a read or write fault and `Memory::PendingAmmMapCovers(address)`, it waits for the queued AMM map (at most `KYTY_FAULT_AMM_WAIT_MS`, default 5000, read once in `run`) and returns 1 when the map is done. An access outside 0..2 returns 0. |
| `quit()` | Pushes `SDL_EVENT_QUIT` (handled at `window.cpp:615`). Used by the stand-alone smoke test only. |
| `flush()` | Closes the pad-audio streams and refuses later pad audio; then, only when `run` has reached `ready`, `Common::Subsystems::EmergencyShutdownActive()`. |
| `user_id()` | `Config::GetUserId()`. |
| `pad_audio(...)` | `Libs::Controller::GetActiveControllerId()`; one `DualSenseHaptics::Open(freq, speaker)` stream per target, reopened on a rate change; `DualSenseHaptics::Queue(...)` (`src/libs/dualSenseHaptics.h:12-19`) with gain times `Libs::Controller::GetSettingScale(...)`. Shortcut: only the active controller is used; the old "enumerate SDL and open a pad" fallback is dropped, because pad selection (2.26) gives Kyty an active controller whenever one is connected. |
| `set_low_latency_interval(us)`, `set_window_size(w, h)` | Setters of the low-latency module (6.1) and the window's pending size. |
| `fns` | A static struct of **addresses of upstream functions**, no wrapper code: `Libs::Controller::PadInit`, `PadOpen`, `PadGetHandle`, `PadReadState`, `PadSetVibration`, `PadSetLightBar`, `PadResetLightBar`, `PadSetTriggerEffect`; `EventQueue::KernelCreateEqueue`, `KernelWaitEqueue`, `KernelAddUserEvent`, `KernelTriggerUserEvent`; `Gen5Driver::AgcDriverAddEqEvent`, `VideoOut::VideoOutAddFlipEvent`. All are `KYTY_SYSV_ABI`. |

Edits to upstream Kyty files for the seam itself:

| File | Change | Lines |
|---|---|---|
| `CMakeLists.txt` | target `kyty_renderer` (1.5) | 12 |
| `src/graphics/presentation/videoOut.cpp` | New `bool FlipQueue::Ready()`: a complete flip group is queued (its owner request exists, none of its requests is still a CPU request, all are in state `Ready`); it may not be due yet. `VideoOutDriver::Impl::PresentThread`: before each of the two `m_flip_queue.Flip()` calls it asks `Embed::PresentGate(now, m_flip_queue.Ready())` when a gate is installed (`Embed::HasPresentGate()`); a non-zero answer skips the flip, and in the between-vblank loop bounds the next sleep by the returned tick; after a flip that presented it calls `Embed::Presented` with the tick the gate was asked at, not the tick the present ended at. The second call site asks once per vblank, so a host limit finer than the vblank needs the between-vblank loop (`KYTY_PRESENT_ABS_VBLANK=1` with the Immediate present mode). | 39 |
| `src/graphics/presentation/window/window.cpp` | `WindowContext::UpdateTitle`: use `Embed::GetOptions().window_title` when set. `Streaming::ReadLiveOutput`: the frame limit is `Embed::FrameLimit()`, which derives it from the low-latency interval (`1 000 000 / interval`, rounded; 60 when the interval is 0), so the session socket reports the bridge's effective limit without a further setter. | 6 |
| `src/common/CMakeLists.txt`, `src/common/hleWaitStats.h`, `src/graphics/host_gpu/renderer/pipeline/descriptors.cpp`, `src/graphics/shader/recompiler/ir/ProgramCodec.cpp` | Not seam code: one line or a few each so that upstream compiles with clang on Linux (`-fexceptions` for three more sources, `<x86intrin.h>`, an explicit lambda return type, an explicit `emplace` argument). | 8 |

Seam total: 441 lines in four new files, 57 lines in 3 upstream files, plus the four build fixes. No edit in
`memory.cpp`, `memory.h`, `memoryAddressSpace.inc`, `hostException.cpp`, `runtimeLinker.cpp`,
`libAgcDriver.cpp`, `libVideoOut.cpp`, `libKernel.cpp`, `libPad.cpp`, `pipelineCache.cpp`.
(`InitLibKernel_1_Mem` has no header declaration; the seam declares it itself, as `libs.cpp` does for
its neighbours.)

### 2.2 AnyPS5: engine switch and host services

All code follows `engines/anyps5/AGENTS.md`: no comments, a row in `docs/dev/ENVIRONMENT.md` per new
variable, a note in `docs/dev/debt/<library>.md` for behaviour that is a model.

**Engine switch, in libkernel (every library already links it).** New
`core/libs/prx/libkernel/include/ExternalRenderer.hpp` (39 lines) and
`core/libs/prx/libkernel/System/src/ExternalRenderer.cpp` (18 lines). The 17 memory members are
`APS5_VABI` function pointers; the struct is 176 bytes with `owns` at offset 144 (asserted on both
sides); the setter throws when a table is smaller than that:

```cpp
struct ExternalRenderer {
    std::uint32_t size;
    ... 17 function pointers with the exact signatures of DirectMemory/Export.cpp ...
    bool (*owns)(std::uintptr_t address, std::size_t bytes);
    std::uint64_t (*padAudio)(std::uint32_t rate, int speaker, const void* samples, std::uint32_t frames,
                              std::uint32_t channels, int isFloat, float gain);
    float (*hapticGain)();
    void (*videoActive)(int active);
};
extern "C" const ExternalRenderer* ExternalRendererGet_nid_no_patch();
extern "C" void ExternalRendererSet_nid_no_patch(const ExternalRenderer*);
```

"A renderer table is installed" **is** the engine switch. There is no engine variable, no library
path variable, no `dlopen` and no `dlsym` in AnyPS5. This meets F1: one switch; the host binds to the
instance the loader loaded; a missing entry is a start-up error raised by the bridge before the table
is installed; nothing depends on the working directory. A null table means upstream behaviour.

| Upstream file | Change | Lines |
|---|---|---|
| `core/libs/prx/libkernel/DirectMemory/Export.cpp` | First statement of 17 exports: `if (const auto* r = ExternalRendererGet_nid_no_patch()) return r->...(...)`. Twelve always; five (`sceKernelMprotect`, `sceKernelMunmap`, `sceKernelVirtualQuery`, `sceKernelSetVirtualRangeName`, `sceKernelClearVirtualRangeName`) only when `r->owns(address, length ? length : 1)`. | ~20 |
| same file | Logging only: the unforwarded functions listed in F3 print one line the first time they are called with an address the renderer owns (detects spec risk R2). | ~8 |
| `core/libs/prx/libkernel/Apr/src/Apr.cpp` | `_readFile` (line 160): take the existing staged-copy branch (upstream: `_WIN32` only) also when a renderer table is installed; the direct read stays for Linux without a table. | 18 |

**Host services: new `core/libs/prx/libAstroportHost/`** (`CMakeLists.txt` 17 lines:
`EXCLUDE_FROM_ALL`, links `libScePad`, and under `BUILD_TESTING` the test program with its six cases
`donate conflict events renderer apr pad`; `libAstroportHost.hpp` 58 lines: the table and the two pad
structs of 3.1; `Export.cpp` 250 lines). The generic loop of
`core/libs/CMakeLists.txt` (line 165) adds `libkernel`, `libc`, `-z now`, the `$ORIGIN` run path and
the NID patcher. Passive: no thread, no initialiser. Every function body catches all C++ exceptions.

| Entry | Upstream AnyPS5 functions it wraps |
|---|---|
| `donate(base, bytes)` | Fixed ranges only. Check with `GuestArenaNextUsed_nid_no_patch` that no used range intersects (else print the conflicting range and return 0); `GuestArenaMarkUsed_nid_postfix(base, bytes)`; `munmap(base, bytes)`. Returns the base or 0. |
| `event_add` / `event_delete` / `event_deliver` | `EqueuePin_nid_postfix`, `EqueueAddEvent_nid_postfix`, `EqueueDeleteEvent_nid_postfix`, `EqueueTriggerEvent_nid_postfix` (`libkernel/Equeue/Equeue.hpp:142-145`) with `KernelEqueueEvent` trigger and reset functions exactly as F4 specifies (EOP: filter -14, add + clear, data counts; flip: filter -13, ident 0, counter saturating at 15, payload passed through unmodified, queued behind a still-triggered event; every flip registration receives every flip, whatever handle it was registered with). The registration list and its mutex live here. Deliver snapshots the list under the lock and triggers outside it. Unknown queue or kind returns `0x80020009`; recorded in `docs/dev/debt/libAstroportHost.md` as a deliberate difference from upstream's throwing functions. |
| `pad_publish(port, input)` | Builds `PadInputState`; `PadPublishInputForPort_nid_postfix`; `PadSetPortConnected_nid_postfix` on change. Motion is a field of the same struct, so no latch. |
| `pad_fetch_output(port, &seq, out)` | `PadFetchOutputForPort_nid_postfix`, copied into a plain struct (3.1). The struct has room for the raw 56-byte trigger commands, but `trigger_set` stays 0 until F9 makes `libScePad` keep them (debt note). |
| `request_exit(code)` | `LibcRequestExit_nid_postfix(code)`. |
| `on_shutdown(callback)` | `LibcRegisterShutdown_nid_postfix(callback)` (`libc/include/Shutdown.hpp:8`). |
| `install_renderer(table)` | `ExternalRendererSet_nid_no_patch`. |
| HDR helpers | `AgcDriverHostHdr_nid_postfix(AgcDriver::HostHdrStatus&)` and `AgcDriverNoteReportedToneMapPeak_nid_postfix(float)` are **defined here** (default SDR status, last noted peak). The NID patcher exports them as `doCCHPPfxkI` and `KMphJjTcNgk`; `libSceSystemService.prx` resolves them from the global scope. They never touch the bridge or a forwarder. |

Seam total in AnyPS5: 382 lines in 5 new files (plus the test `core/libs/tests/GuestAstroportHost.cpp`),
about 50 lines in 3 upstream files (`DirectMemory/Export.cpp` 31, `Apr.cpp` 18, one source line in
libkernel's `CMakeLists.txt`), two lines in `scripts/prx-targets.txt` (ours). No libc change.

---

## 3. Our ABI between host and renderer

Three plain C tables. Two are owned by the engines and consumed by the bridge (`KytyEmbedApi`,
`AstroportHostApi`); one is produced by the bridge and consumed by AnyPS5 (`ExternalRenderer`). The
bridge is the only component that knows both sides.

### 3.1 The tables

```c
#define KYTY_EMBED_ABI 1

typedef struct KytyEmbedConfig {
    uint32_t size;
    uint32_t width, height;             /* 0 = Kyty default */
    uint32_t vblank_hz;                 /* 0 = 60 */
    uint32_t vibration_intensity;       /* 0..100 */
    uint32_t rumble_scale;              /* percent 0..200 */
    uint32_t rumble_switch_floor;       /* 0..255 */
    uint32_t low_latency_interval_us;   /* initial value; 0 = none */
    uint32_t dlss_preset;               /* 0 = model default */
    uint64_t upscale_pass_hash;         /* pixel shader hash of the pass DLSS replaces; 0 = none */
    uint8_t  fullscreen, present_mode /*0 immediate 1 fifo 2 mailbox*/, shader_optimization /*0 none 1 size 2 performance*/;
    uint8_t  wayland_native, low_latency, dlss, dlss_reverse_depth;
    uint8_t  pad_prefer /*0 off 1 auto 2 ps 3 xbox 4 switch 5 generic*/, ds_rumble, fsr_fg;
    const char* app0_dir;               /* UTF-8; param.json, icon0.png, eboot.bin are read from it */
    const char* window_title;           /* NULL = Kyty's diagnostic title */
    const char* dlss_dir;               /* folder of the DLSS runtime (read-only is fine) */
    const char* ngx_data_dir;           /* writable folder for NGX */
    const char* fsr_dir;
} KytyEmbedConfig;

typedef struct KytyEmbedHooks {
    uint32_t size;
    void        (*reserved)(void);      /* guest address space is reserved; run thread */
    void        (*ready)(void);         /* all subsystems are up; run thread */
    int64_t     (*present_gate)(int64_t now, int64_t ticks_per_s, int flip_ready);
                                        /* 0 = present now; >0 = do not present before this tick */
    void        (*presented)(int64_t now, int64_t ticks_per_s);
    const char* (*live_setting)(const char* apply_json);   /* reply JSON, valid until the next call */
} KytyEmbedHooks;

typedef struct KytyEmbedFns {           /* addresses of upstream KYTY_SYSV_ABI functions */
    void *pad_init, *pad_open, *pad_get_handle, *pad_read_state, *pad_set_vibration,
         *pad_set_light_bar, *pad_reset_light_bar, *pad_set_trigger_effect;
    void *equeue_create, *equeue_wait, *equeue_add_user_event, *equeue_trigger_user_event;
    void *agc_add_eq_event, *video_out_add_flip_event;
} KytyEmbedFns;

typedef struct KytyEmbedApi {
    uint32_t abi, size;
    uint32_t pad_data_size;             /* sizeof(Libs::Controller::PadData) = 120 */
    const KytyEmbedFns* fns;
    int      (*run)(const KytyEmbedConfig*, const KytyEmbedHooks*);   /* blocks until the window loop ends */
    void*    (*find)(const char* nid);
    int      (*fault)(uint64_t address, int access /*0 read 1 write 2 execute*/, uint64_t instruction);
    void     (*quit)(void);
    void     (*flush)(void);
    int      (*user_id)(void);
    uint64_t (*pad_audio)(uint32_t rate, int speaker, const void* samples, uint32_t frames,
                          uint32_t channels, int is_float, float gain);   /* microseconds queued; 0 = refused */
    void     (*set_low_latency_interval)(uint32_t us);
    void     (*set_window_size)(uint32_t width, uint32_t height);
} KytyEmbedApi;

const KytyEmbedApi* kyty_embed_api(void);
```

```c
#define ASTROPORT_HOST_ABI 1
typedef struct AstroportHostApi {
    uint32_t abi, size;
    uint64_t (*donate)(uint64_t base, uint64_t bytes);                 /* returns base, or 0 */
    int      (*event_add)(uint64_t queue, int kind /*0 EOP 1 flip*/, uint64_t ident, void* user_data);
    int      (*event_delete)(uint64_t queue, int kind, uint64_t ident);
    void     (*event_deliver)(int kind, uint64_t ident, uint64_t data);
    void     (*pad_publish)(int port, const AstroportPadInput*);       /* connected flag, optional motion */
    int      (*pad_fetch_output)(int port, uint32_t* sequence, AstroportPadOutput*);
    void     (*request_exit)(int code);
    void     (*on_shutdown)(void (*callback)(void));
    void     (*install_renderer)(const void* external_renderer);
} AstroportHostApi;
const AstroportHostApi* AstroportHostApi_nid_no_patch(void);

typedef struct AstroportTouch { uint16_t x, y; uint8_t id, active; } AstroportTouch;   /* 6 bytes */

typedef struct AstroportPadInput {      /* 48 bytes */
    uint32_t buttons;                   /* PS5 bit layout */
    uint8_t  sticks[4];                 /* left x, left y, right x, right y; 128 = centre */
    uint8_t  l2, r2;
    uint8_t  connected, has_motion;
    AstroportTouch touch[2];
    float    accel[3];                  /* m/s^2 including gravity */
    float    gyro[3];                   /* rad/s */
} AstroportPadInput;

typedef struct AstroportPadOutput {     /* 122 bytes */
    uint8_t rumble_large, rumble_small;
    uint8_t audio_rumble_large, audio_rumble_small;
    uint8_t light_bar_valid, light_bar[3];
    uint8_t trigger_set[2];             /* per trigger (0 = L2, 1 = R2): a command was set */
    uint8_t trigger[2][56];             /* raw trigger commands */
} AstroportPadOutput;
```

The two pad structs are defined in `bridge/src/abi.rs` and mirrored in `libAstroportHost.hpp`; their
fields follow spec-anyps5 C8 to C10. `abi.rs` asserts every size at compile time: `KytyEmbedConfig`
104, `KytyEmbedHooks` 48, `KytyEmbedFns` 112, `KytyEmbedApi` 96, `AstroportHostApi` 80,
`ExternalRenderer` 176, `PadData` 120, the kernel event 32.

`ExternalRenderer` is in 2.2.

### 3.2 Versioning

Each table starts with `abi` and `size`. The bridge requires the exact `abi` and `size >=` the size it
was compiled against; fields are only appended. A null pointer among the entries the bridge needs is a
start-up failure that names the entry. All libraries ship in one tarball, so nothing more is built.
`pad_data_size` guards the one structure the bridge mirrors from Kyty's headers. The forwarders are
tied to the bridge by the hash of the export table in the symbol name `astroport_entry_<hash8>`: a
stale forwarder fails at load with an undefined symbol.

### 3.3 Ownership and threading rules

| Rule | |
|---|---|
| Data | Only plain C data crosses. Pointers passed to a call are valid during the call only; the callee copies. Guest pointers are opaque. Config strings live for the process. |
| Exceptions / panics | Host seam functions catch everything. The bridge is built with `panic = "abort"`. Kyty seam functions wrap code that already aborts on failure. |
| Bridge threads | renderer thread (calls `run`), event pump, pad relay. |
| Guest threads | graphics NIDs (through the forwarders), the 17 memory functions (through `ExternalRenderer`), `event_add` / `event_delete` inside the NID call. They take write faults. |
| Pump thread | `event_deliver` only; it must not block on game threads. |
| AnyPS5 audio threads | `padAudio`, `hapticGain`: lock-free in the bridge (atomics), then `KytyEmbedApi.pad_audio`. AnyPS5 must not hold its AudioOut2 context lock across the call (worker hand-off, spec R8). |
| Kyty session socket worker | `live_setting`. |
| Donated ranges | Lent to Kyty until process end. Never returned, never unmapped by either side. |

### 3.4 Start sequence

Phase 1 runs in the bridge's ELF constructor (`.init_array`), after both engines were initialised by
the loader, before any guest code:

1. `dlsym(RTLD_DEFAULT, "kyty_embed_api")` and `"AstroportHostApi_nid_no_patch"`; check versions.
   Neither found: not a game process, return (1.3).
2. Parse the environment into `Config` (section 7). An invalid value aborts naming the variable.
3. Donate the ranges (5.1).
4. Install the `SIGSEGV` / `SIGBUS` action (5.3).
5. `install_renderer(&EXTERNAL_RENDERER)`: the 17 memory entries are bridge thunks that wait for ready.
6. `on_shutdown(shutdown_callback)`.
7. Spawn the renderer thread, which calls `api.run(&config, &hooks)`.
8. **Wait for `hooks.reserved()`** (bound: 10 s), then verify the adoption (5.1). Do **not** wait for
   `ready`.

Any failure prints one line naming what failed and calls `abort()`.

Why this split. Waiting for the reservation makes Kyty's one-shot scan of `/proc/self/maps` happen
while the main thread is parked and no guest thread exists, so nothing else can unmap arena memory
during the scan. In upstream's order nothing before the Memory lifecycle loads a library
**[unverified for the Log and Profiler lifecycles; M0 constructor test]**. Not waiting for `ready`
means the constructor never waits on a thread that loads the Vulkan driver, so the design holds even
if start-up constructors turn out to run under the loader lock. All Kyty subsystems are initialised
on one thread, as upstream does.

Phase 2, renderer thread (8 MiB stack, the size of a process main stack), inside `hooks.ready()`: the
bridge creates the proxy queue and starts the pump, resolves every export with `api.find(nid)` (a
null result aborts with the NID and the Kyty function name; the table was generated from Kyty's
source) except the three event exports, which get its own functions, opens the pad and starts the
relay thread, and only then binds all slots, sets `READY` and wakes waiters. The slots are bound last
because the fault handler passes every fault on until `READY`: no guest thread may reach Kyty earlier.
A `run` that returns non-zero aborts the process with the code.

The renderer starts eagerly, in parallel with the game's own start-up. No guest thread performs the
bring-up, and the donation point is defined (before guest code). This closes spec-anyps5 R1 and
spec-kyty risk 5.

### 3.5 Callers that arrive before ready

Every slot initially points at one wait stub (4.3). It recovers the slot index, saves the argument
registers, calls `wait_ready(index)` (a condition variable with a 120 s limit; on time-out it prints
the export's NID and Kyty function name and aborts), restores the registers and dispatches through
the slot again.

Possible simplification, permitted by the M0 constructor test below and **not taken**: the
constructor would wait for `ready` and the wait stub would be deleted. The wait stub is kept, so the
game's start-up overlaps the renderer's.

**M0 constructor experiment, 2026-11-25** (`bridge/tests/ctor_experiment.sh`; glibc 2.44, NVIDIA
RTX 3090). A stub library's constructor spawned a thread that did `dlopen("libvulkan.so.1")`,
`vkCreateInstance` and `vkEnumeratePhysicalDevices` (so the GPU drivers were loaded too). With the
stub as a `DT_NEEDED` of the program and the constructor waiting for the thread: the thread finished
in 67 to 373 ms, the constructor returned and `main` ran; the same with an `LD_AUDIT` library
present. Without waiting: also fine. Control case, the stub loaded by `dlopen` and the constructor
waiting: deadlock (the wait timed out after 10 s, the thread's `dlopen` completed only after the
constructor returned). Conclusion: start-up constructors do not run under the loader lock, so a
start-up constructor may block on a thread that loads libraries. Risk 1 does not occur, the
**[unverified]** note in 3.4 is moot (a lifecycle that loads a library before `reserved` cannot
block), and the simplification above is permitted: the constructor may wait for `ready` and the
wait stub can go. This holds only while the bridge is loaded by `DT_NEEDED` at program start; it
must never be loaded with `dlopen`. Cost of taking the simplification: the game's start-up no
longer overlaps the renderer's.

### 3.6 GPU events and flips

All in `bridge/src/events.rs`, using the direct function addresses of `KytyEmbedFns`.

* Three exports are bound to Rust functions instead of Kyty's: `w2rJhmD+dsE` (`AgcDriverAddEqEvent`),
  `DL2RXaXOy88` (`AgcDriverDeleteEqEvent`), `HXzjK9yI30k` (`VideoOutAddFlipEvent`)
  (`src/libs/libAgcDriver.cpp:188-189`, `libVideoOut.cpp:25`). The generator marks them by Kyty
  function name, not by a hand-typed NID.
  * add: `host.event_add(queue, kind, ident, user_data)`; a host error is returned unchanged. The first
    time an ident (or video-out handle) is seen, register it on the Kyty proxy queue with
    `fns.agc_add_eq_event(proxy, id, null)` / `fns.video_out_add_flip_event(proxy, handle, null)`.
  * delete: `host.event_delete` only; the proxy registration stays.
* Proxy queue: `fns.equeue_create`, plus one user event (`fns.equeue_add_user_event`) to wake the pump
  for stop (`fns.equeue_trigger_user_event`).
* Pump thread: `fns.equeue_wait(proxy, events[16], 16, &n, null)`; filter -14
  (`KERNEL_EVFILT_GRAPHICS`) gives `event_deliver(EOP, ident, data)`, filter -13
  (`KERNEL_EVFILT_VIDEO_OUT`) gives `event_deliver(FLIP, ident, data)`. The PS5 kernel event
  structure is mirrored in `abi.rs`.
* Trigger semantics for the game's queues live in the host seam (2.2).
* Not built: vblank events, flip-event deletion (`-Ozn0F1AFRg` stays bound to Kyty's own function).

### 3.7 Input, pad output, motion, pad audio

Relay thread, 4 ms period, port 0 only, `bridge/src/pad.rs`.

* Open: `fns.pad_init()`, `fns.pad_open(api.user_id(), 0, 0, null)` falling back to
  `fns.pad_get_handle`.
* Input: `fns.pad_read_state` into a mirror of `PadData` (120 bytes, `src/libs/padData.h:54`); publish
  buttons, four stick bytes, L2 / R2, connected flag, two touch points.
* Motion: always relayed (the old switch is dropped). Acceleration times 9.80665, angular velocity as
  is. Fallback when all three rates are exactly zero and a previous sample exists: body rate from the
  orientation quaternion delta, `2 * vec(conj(prev) * cur) / dt`, sign flipped when the scalar part is
  negative, only for `0.5 ms <= dt <= 100 ms` (spec-kyty 2.18).
* Output: `host.pad_fetch_output(0, &seq, &out)`; apply only differences: rumble = per motor
  max(game, audio-derived), sent when either motor changed or on the first update; light bar set or
  reset; trigger effect block (mask byte, padding to 8, two 56-byte commands) with only new or changed
  triggers flagged.
* Pad audio: `ExternalRenderer.padAudio` is a bridge function that forwards to
  `KytyEmbedApi.pad_audio`; it returns 0 before ready and after shutdown started.
* Haptic gain: `ExternalRenderer.hapticGain` returns an atomic float owned by the bridge (initial
  value from `APS5_PAD_VIBRATION_GAIN`, clamped 0..4, changed by live settings).
* Video playing: `ExternalRenderer.videoActive(int)` sets an atomic that the limiter reads (K9).

### 3.8 Window close, process exit, shutdown

Decision: **the renderer is never torn down; it is flushed.** Upstream does exactly this in its own
exit path, and tearing subsystems down under running guest threads is the larger hazard.

* Player closes the window: `WindowRun` returns, `run` returns, the renderer thread calls
  `host.request_exit(0)` and parks forever with all subsystems alive.
* Any exit through libc's shutdown (window close, game-initiated exit): AnyPS5 runs the callback
  registered with `LibcRegisterShutdown_nid_postfix`. The bridge's callback, once: sets the stop flag
  (relay and pump leave their loops, `padAudio` returns 0), wakes the pump, calls `api.flush()` on a
  helper thread and waits at most 3 s. It does not call `quit`.
* Hard kill: nothing runs, as with upstream Kyty.

If `EmergencyShutdownActive` turns out not to write the pipeline cache **[unverified, M5]**, `flush`
also calls the cache's own save.

---

## 4. How the game's PS5 graphics imports reach the renderer

### 4.1 Path of one call

```
game: call <NID>@plt
  -> forwarder .prx   "<NID>":  movl $index, %r11d ; jmp *astroport_entry_<hash8>@GOTPCREL(%rip)
  -> bridge           astroport_entry:  shl $3, %r11 ; add SLOTS@GOTPCREL(%rip), %r11 ; jmp *(%r11)
  -> Kyty HLE function (KYTY_SYSV_ABI), or a Rust function (three event exports)
```

The dispatch is position independent: a shared library cannot use the absolute form
`jmp *SLOTS(,%r11,8)`, so the slot's address is computed in `r11` from the array's GOT entry. At the
target, `r11` therefore holds the address of the slot, not the index. Two jumps, no stack use, no
register touched except `r11` (and the flags), which the System V convention leaves free at a call
boundary. Integer, vector and variadic arguments pass untouched.

### 4.2 Export list: title-independent

`nidscan.rs` reads `engines/kyty/src/libs/libAgcDriver.cpp` and `libVideoOut.cpp`. A `LIB_VERSION`
line sets the current library: `Graphics5` (line 10) = `libSceAgc`, `Graphics5Driver` (line 173) =
`libSceAgcDriver`, `VideoOut` = `libSceVideoOut`. Every `LIB_FUNC` line adds a row (164 + 36 lines in
the tree). A duplicate NID, an empty library or a data registration is a build error. From
`InitLibKernel_1_Mem` it takes the 17 memory functions by Kyty function name; these are not exported
by a forwarder, they reach Kyty through `ExternalRenderer`.

**Every** NID Kyty registers for the three libraries is exported, so the artifacts do not depend on a
title or game version. "Fail when the game imports something unimplemented" moves to where the game is
known: `runner` compares every NID the relinked executable and its bundled modules import from the
three libraries (every `AstroBot*.registry.json` the relinker writes into the run folder; a `#`
suffix is stripped) with `install/lib/astroport-exports.txt` and fails the job naming the missing
NIDs. A NID counts as provided when **any** of the three forwarders exports it, not only the one
named after the importing library: the loader resolves symbols by name alone, and Kyty files some
NIDs under another library than the game imports them from. Imports of AnyPS5's own libraries
are checked at build time (1.4, check 2).

### 4.3 The wait stub

One naked Rust function, used only before ready:

* recovers the slot index from `r11`, which the dispatch left pointing at the slot:
  `sub SLOTS@GOTPCREL(%rip), %r11 ; shr $3, %r11`;
* saves `rdi rsi rdx rcx r8 r9 rax r10 r11` and `xmm0..xmm7` (`rax` carries the vector-register count
  of variadic calls, `r11` the index) in a 200-byte frame, which also aligns the stack to 16 bytes at
  the call; calls `wait_ready(index)`;
* restores everything and runs the same dispatch sequence as `astroport_entry` on the index.

Constraints: no red-zone use, no callee-saved register modified, nothing left on the stack at the tail
jump. The 17 memory thunks are generated by one macro: `mov $index, %r11d` followed by the dispatch
sequence inline (slots 200..216, after the graphics exports).

### 4.4 Symbol isolation of the renderer library

Hard requirement: the host has its own SDL2; Kyty links SDL3 and more statically. Three layers: the
version script (one global), `-Bsymbolic`, and build check 1. The bridge exports one symbol; the
forwarders export NIDs only; neither links C++.

---

## 5. Memory and faults

### 5.1 Donated ranges

Performed in phase 1 through `host.donate`:

| Range | Call | Why |
|---|---|---|
| `0x200000000 .. 0x7FFFF0000` | fixed | PS5 system-managed area; Astro Bot maps 2 GiB at `0x300000000`; Kyty's default area. |
| `0xDC00000000 .. 0xEC00000000` (64 GiB) | fixed | General room for direct and flexible memory, directly below Kyty's `kAmmWindowBase`, so ordinary searches reach it. One constant in `memory.rs`. |
| `0xFE0000000 + 0x20000000` (Kyty's AGC driver block, `DriverDmemBase`, `src/libs/agc.cpp:53`) | **no call** | It lies in the system-reserved range, which the arena records as used but never maps. Kyty's scan finds it free and reserves the whole range `0x7FFFFC000 .. 0x1000000000`. |

A refused donation aborts with the requested and the conflicting range.

Kyty adopts **every** free gap in its regions, not only donated ones. The bridge's owned list is
therefore the two donated ranges plus the system-reserved range, and `owns(address, bytes)` is "inside
one of them". After `hooks.reserved()` the bridge checks each of the three once:
`mmap(base, 4096, PROT_NONE, MAP_PRIVATE|MAP_ANONYMOUS|MAP_FIXED_NOREPLACE)` must fail with `EEXIST`.
If not, it aborts with the range. This catches a Kyty that did not adopt a range (for example an
upstream change to `ReserveGuestRegions`). During the window between `munmap` and Kyty's reservation
only the parked main thread and the renderer thread exist.

No floating donation, so no size variable and no "highest free" code.

### 5.2 Forwarding libkernel memory calls

Section 2.2: twelve functions always, five by address through `owns`. The functions behind them are
Kyty's own (`Libs::LibKernel::Memory::KernelAllocateDirectMemory` ... `KernelClearVirtualRangeName`),
found by NID. Results are returned unchanged. The unforwarded functions of F3 stay with AnyPS5; their
first call on a renderer-owned address is logged once (2.2).

### 5.3 Write faults

The bridge owns the process's first-chance fault action. Installed in phase 1 with `sigaction` for
`SIGSEGV` and `SIGBUS`, flags `SA_SIGINFO | SA_ONSTACK | SA_RESTART`, previous actions saved.

Handler (async-signal-safe, no allocation):

1. If not ready, or the fault address is not inside an owned range: chain.
2. Access from the page-fault error code in the signal context (bit 1 write, bit 4 execute); take the
   instruction pointer.
3. `api.fault(address, access, rip)`; non-zero: return (the instruction is retried).
4. Otherwise chain: previous `sa_sigaction`, or previous classic handler, or restore the default
   action and return so the retry terminates the process.

Order with AnyPS5: its write watch installs lazily and chains to the previous action for faults it
does not own. The bridge installs at process start, so later handlers chain into it; if something was
installed earlier, the bridge chains to it. Both orders work, and the range check keeps foreign
faults cheap. Kyty installs nothing. The bridge counts handled and passed-on faults and prints both
in its exit summary; "handled = 0 after a frame" is the early sign of a broken chain.

### 5.4 Kernel writes into protected pages

A system call writing into a write-protected GPU-visible page fails with `EFAULT`. The one path the
game needs is APR: `_readFile` uses its staged copy when a renderer is installed (2.2). Other paths
are not changed (spec R5; detection in section 9).

---

## 6. Feature mapping

"Rust" = `bridge/`. "Kyty" / "AnyPS5" = C++ in that engine. "Options" = a field of
`Embed::GetOptions()`, filled from `KytyEmbedConfig`; Kyty feature code reads no new environment
variable. Sizes are estimates.

### 6.1 spec-kyty

| Entry | Rating | Where | How |
|---|---|---|---|
| 2.3 export table and binding | MUST | Rust + generated asm | `nidscan`, `nidgen`, `entry.rs`. |
| 2.4 renderer inside the host | MUST | Kyty seam `run`; Rust renderer thread | 2.1, 3.4. |
| 2.5 start, memory exports, event proxy | MUST | Rust | Eager start; memory through `ExternalRenderer`; `events.rs`. |
| 2.6 donated address space, fault handling | MUST | Rust + host seam; Kyty unchanged | 5.1, 5.3. |
| 2.7 input relay | MUST | Rust `pad.rs` | 3.7. |
| 2.9 renderer options | MUST | Rust `config.rs` -> `KytyEmbedConfig` | Defaults in use: Immediate, Performance, 60 Hz (upstream's own defaults are Mailbox and None, so the seam always sets them). |
| 2.10 window close | MUST | Rust | 3.8. |
| 2.13 pad output | SHOULD | Rust `pad.rs` | 3.7. |
| 2.16 cache title id | SHOULD | Kyty seam | `Loader::SystemContentLoadParamSfo(app0/sce_sys/param.json)`; upstream `PipelineCacheTitleId()` (`pipelineCache.cpp:218`) then works unpatched. No title-id variable. |
| 2.17 frame-rate limiter | SHOULD | Rust `limiter.rs` + 15 lines in `videoOut.cpp` | State machine as specified (interval = ceil(f / limit); cadence rule with 1 ms lateness; restart from now otherwise; `unlimited`; reset on change). `present_gate` returns the due tick whether or not a flip is ready (Kyty passes `FlipQueue::Ready()` as `flip_ready`), so the between-vblank loop sleeps instead of polling; `presented` receives the tick the gate was asked at (2.1). The absolute-vblank timeline is selected by upstream `KYTY_PRESENT_ABS_VBLANK=1` in `fixed_env`. Ctrl+F2 cycling, overlay text and title suffix are not built (optional in the spec). |
| 2.18 motion relay | SHOULD | Rust `pad.rs` | 3.7, with the quaternion fallback. |
| 2.20 fixed window title | SHOULD | Kyty, `window.cpp` `UpdateTitle` | Options `window_title`. |
| 2.21 DLSS | SHOULD | Kyty | 6.4. |
| 2.25 low-latency mode | SHOULD | Kyty, new `src/graphics/presentation/window/lowLatency.{h,cpp}` (~230) + hooks: `vulkanWindow.cpp` (`VulkanCreateDevice` line 679, `WindowContext::CreateVulkan` 2203; ~12), `swapchain.cpp` (`Swapchain::Create` 403, `Destroy` 571, `Present` 1260; ~15), `videoOut.cpp` (`VideoOutSubmitFlip`, `FlipQueue::Reserve` 974, `Flip`; ~10), `src/libs/agc.cpp` (`AgcDcbSetFlip` 4308 and the submit helper; ~6) | As specified: only with both `VK_NV_low_latency2` and `VK_KHR_present_id`; latency create-info chained at swapchain creation; low latency on, boost on; one timeline semaphore; the sleep on the guest thread bounded to 100 ms; one mutex around every call naming the swapchain, also across `vkQueuePresentKHR`. No "measure" mode, no frame-generation branch. **The minimum interval is derived**: the bridge calls `set_low_latency_interval(1 000 000 / effective limit)` whenever the effective frame limit changes (launch, live setting, video override), 0 for none or `unlimited`. The same value is the one channel that tells Kyty the effective frame limit: `Embed::FrameLimit()` turns it back into frames per second for the session socket (2.1). No interval variable. Off while FSR frame generation is requested. |
| 2.26 controller selection, rumble strength | SHOULD | Kyty, `src/libs/controller.cpp` (near `RefreshPlayers` 1549, `GameController::ApplyVibration` 1358; ~110) + the controller button handler in `window.cpp` (~6) | Options `pad_prefer`, `rumble_scale`, `rumble_switch_floor`. |
| 3.1 pad audio entry, rumble scale seeding | ours | Kyty seam `pad_audio`; Rust config | `rumble_scale` also seeds `controller_vibration_intensity`. |
| 3.1 / 3.5 DualSense edits (`src/libs/dualSenseBluetooth.cpp`, `dualSenseHaptics.cpp`) | ours | Kyty | Not in the tree. Written again from the spec text; the rumble switch is Options `ds_rumble`. The two wireless opt-out switches are not re-created (never set by the product). See decision 3. |
| 3.3 Wayland native surface | ours | Kyty, `window.cpp` `WindowCreate` (line 870) | Not in the tree. Options `wayland_native`. |
| 3.4 low-latency interval | ours | - | Derived, see 2.25. |
| 3.6 whole-range search | ours | - | Not needed (5.1). |
| 3.8 session socket client | ours | Kyty | `src/streaming/` is in the tree, nothing constructs `Streaming::Transport`. The seam constructs it (6.5); no upstream file is edited for it. |
| 3.9 live settings | ours | Rust `live.rs` + Kyty seam | 6.5. |
| 3.10 native frames on the session socket | ours | Kyty | Not in the tree and described in one paragraph only. See decision 3. |
| 3.11 NGX on Linux | ours | - | Obsolete: upstream's loader is used (6.4). |
| 3.2, 3.7, 3.12 | ours | - | In the tree (launcher out, clang build fix) or re-applied when the compiler asks. |

### 6.2 spec-anyps5

| Entry | Rating | Where | How |
|---|---|---|---|
| F1 engine selection, host module | MUST | AnyPS5 `ExternalRenderer.hpp/.cpp`, `libAstroportHost` | 2.2. |
| F2 donation | MUST | host seam `donate`, existing arena exports | 5.1. No libc change. |
| F3 memory forwarding | MUST | AnyPS5 `DirectMemory/Export.cpp` | 2.2. |
| F4 GPU events | MUST | host seam (semantics) + Rust (proxy, pump) | 3.6. Depends on the equeue wake-all fix, which is in the tree. |
| F5 HDR helpers | MUST | host seam | 2.2. |
| F7 pad input | MUST | host seam `pad_publish` | Uses `PadSetPortConnected_nid_postfix`. |
| F8 window close | MUST | host seam `request_exit` | 3.8. |
| O1c APR staging | MUST | AnyPS5 `Apr.cpp` | 5.4. |
| F9 pad output | SHOULD | AnyPS5 `libScePad` (`include/PadState.hpp`, `src/PadState.cpp`, `Export.cpp`, ~20 lines: keep the raw 56-byte command and a "set" flag per port and trigger) + host seam | |
| F10 trigger effect state | SHOULD | AnyPS5 `libScePad/Export.cpp` `scePadGetTriggerEffectState` (line 229) + a pure helper with a unit test (~60) | Behind `APS5_PAD_TRIGGER_EFFECT_STATE`; debt note in `docs/dev/debt/libScePad.md`. |
| F11 motion | SHOULD | host seam | Inside `pad_publish`. |
| F13 pad audio through the renderer, with O2, O3, O4, O10 | SHOULD | AnyPS5 `libSceAudioOut` (`src/AudioOut.cpp`, `AudioOut2Context.cpp`, `AudioOut2Port.cpp`, `AudioOut2PadMix.{hpp,cpp}`, ~150) | Calls `ExternalRendererGet_nid_no_patch()->padAudio / hapticGain`; a null table is the upstream path. Both the USB and the Bluetooth branch hand grains to one bounded worker queue (32 grains, oldest dropped), never under the context lock (closes R8). `APS5_PAD_VIBRATION_GAIN` and `APS5_PAD_VIBRATION_VIA_HOST` keep their names. |
| F14 save-data transfer fallback | SHOULD | AnyPS5 `libSceSaveData.native/Export.cpp` `transferringMount` (line 884), reusing `mount3` (609) (~15) | Behind `APS5_SAVEDATA_TRANSFER_AS_MOUNT`; debt note in `docs/dev/debt/libSceSaveData.native.md`. |
| O9 controller audio sink | ours | AnyPS5 `AudioOut.cpp`, `AudioOut2Context.cpp` + pure helper with unit test | `AGS_CONTROLLER_AUDIO_SINK`. Not in the tree. |
| O5 haptics dump | ours, optional | AnyPS5 | Re-added only when `tools/rec-haptics.sh` is needed again. |
| O6, O7, O8 | ours | - | In the tree. |
| F6, F12, O1d | DROP | - | Not built. F12 has a late-donation fallback (risk 13). |

### 6.3 The 14 ideas

| Idea | Where | How |
|---|---|---|
| A1 odx lookup, wrapped-root check | AnyPS5 `libc/src/AppArchive.cpp` (`AppArchiveLookup_nid_no_patch` line 97, `AppArchiveOnly_nid_no_patch`) + tests | As specified. |
| A2 stdio on archived files | AnyPS5 libc (`include/FileStream.hpp`, `src/FileAndHeap.cpp`, `StandardStreams.cpp`, `Formatting.cpp`, `ScanningWide.cpp`, `StreamPurge.cpp`, two new files) + tests | As specified; the largest AnyPS5 item (~800 + ~400 lines of tests). |
| A3 inline archive flag | AnyPS5 `FileStream.hpp` | Built into A2 from the start. |
| A4 audio propagation accepts rays | AnyPS5 `libSceAudioPropagation/Export.cpp` (`requireNoPropagation` line 136, called from `sceAudioPropagationSystemSetRays` line 249) | Accept and ignore, log once. |
| A5 avplayer tells the renderer | AnyPS5 `libSceAvPlayer` (`include/AvPlayer.hpp`, `src/Player.cpp`, ~40) | Calls `ExternalRendererGet_nid_no_patch()->videoActive(0/1)` on counter edges; no symbol lookup; a null table is a no-op. Unit test with a fake table. |
| K1 gyro offset | Kyty, new header `src/libs/gyroBias.h` (~90) + `controller.cpp` (`GameController::Motion` line 1161, `SetDevices` 956; ~10) | As specified; the relayed values are corrected too. Unit test with synthetic data. |
| K2 FSR frame generation | Kyty, new `src/graphics/presentation/window/frameGen.{h,cpp}` + hooks in `vulkanWindow.cpp`, `swapchain.cpp`, `vma.cpp` (`GetTotalMemoryBudget`), `renderDraw.cpp`; plus a Linux build of AMD's FSR 3.1 frame generation from its MIT source | Off by default. Reuses the pass recognition and depth / motion / jitter capture of 6.4. AMD ships Windows binaries only and its Vulkan swapchain does not compile on Linux as shipped. Approach: decision 1. |
| K3 window icon fallback | none | The seam loads system content from `app0`, so the unchanged first choice `Loader::SystemContentGetIconPath` finds `icon0.png` **[verify in M4]**. No variable. If it does not, the two-step fallback of the spec goes into `WindowContext::UpdateIcon` (line 1042). |
| K4 pipeline list | none | Items 1 and 3 are covered by the seam (`SystemContentLoadParamSfo`, `SetPipelineListExecutable(app0/eboot.bin)`); item 2 (`KYTY_PIPELINE_LIST_CROSS`) is upstream. |
| K5 evict before spill | Kyty `src/graphics/host_gpu/vma.cpp` (`GraphicContext::CreateImage`, `BindSparseImageLevels` line 575, ~40) + an eviction entry in `renderer/cache/textureCache.h/.cpp` built on `RunPressureGarbageCollector` / `RunBudgetGarbageCollector` (~60) | As specified. Only "out of device memory" counts as a memory failure, as upstream does today. |
| K6 spirv-val option | none | Already upstream at all call sites. A grep in `scripts/check-repo.sh` that every validator call passes options. |
| K7 hidden window during prepare | Kyty `window.cpp` `WindowCreate` (~6) | Plus: the adapter runs the prepare step with present mode Immediate, so a never-mapped Wayland surface cannot block. |
| K8a cooldown, K8b draw guard | Kyty `vma.cpp` (~50); `renderDraw.cpp` (`RenderExecutor::DrawIndex` line 9095, `DrawAuto`; ~15) | As specified. Unit tests of the cooldown state machine (injected clock) and of the predicate at 2^30 - 1 and 2^30. |
| K9 60 FPS during video | Rust `limiter.rs` | Effective limit = 60 while the video flag is set; cadence resets when the effective limit changes; the low-latency interval follows it. No Kyty code. Sender is A5. |

### 6.4 DLSS: upstream's NGX module plus a title hook

Reused unchanged: `Dlss::Load`, `AppendExtensions`, `Init`, `Capabilities`, `Choose`, `Takes`,
`CreateFeature`, `Evaluate`, `ReleaseFeature`, and the feature cache `PssrState::Upscale`
(`renderer/renderPssr.cpp`). Not used: `RenderExecutor::ProcessPssr` (line 172). It takes the render
lock, speaks guest texture descriptors, and falls back to a bilinear pass; we need the game's own
pass as the fallback.

What we add:

| Piece | File | Size |
|---|---|---|
| Gate | `dlss.cpp` `Load()` (line 227): proceed when `Embed::GetOptions().dlss` is set and a folder is given, not only in Pro mode. `pro_enabled` stays false, so no Pro-mode side effect. Update the test `LoadsOnlyInProModeWithAFolder`. | 2 lines + test |
| Writable NGX folder | `dlss.cpp` `Init()` (line 279): pass `Embed::GetOptions().ngx_data_dir` as the application data path when set; the model folder stays the feature search path. The bridge creates `$XDG_CACHE_HOME/astroport/ngx` (or `~/.cache/astroport/ngx`). | 3 lines |
| Jittered motion vectors | `src/graphics/guest_gpu/pssr.h`: an internal flag `kJitteredMotion = 1u << 31` (not a guest bit). `dlss.cpp`: add it to `FIXED_FLAGS` (line 39) and map it in `SetCreation` (line 179) to NGX's "motion vectors jittered" creation flag (bit 2). One test case in `tests/DlssTests.cpp`. | 5 lines + test |
| Second entry | `renderPssr.cpp` + `render.h`: `bool RenderExecutor::UpscaleResolved(CommandBuffer&, const Pssr::Request&, colour, depth, motion, output)` taking already-resolved cache images. It ends rendering, transitions the four images to general layout, builds `Dlss::Inputs` as `ProcessPssr` does (lines 301-345, depth with its backing format and depth aspect), and returns the result of `PssrState::Upscale`. No lock (the caller holds it), no linear fallback. | ~70 lines |
| Pass hook | `renderDraw.cpp` `RenderExecutor::ExecutePreparedDraw` (line 6815), after the late checks: if the pixel shader hash equals Options `upscale_pass_hash`, call the new module; on success the draw records nothing else and draw-run continuation is invalidated, like a skipped draw. Every draw also feeds the colour-to-depth ring. | ~15 lines |
| New module | `renderer/upscalePass.{h,cpp}`: read the pass constants (jitter words 0, 1; output size 4, 5; input size 6, 7; frame counter 11; motion flag 13), look up depth in a 16-entry colour-to-depth ring, build a `Pssr::Request` (width / height = input size; jitter = UV jitter times input size; `motion_scale` = (-input width, -input height); flags `kAutoExposure | kJitteredMotion`, `kReverseDepth` from Options, `kReset` when the frame counter is not the previous plus one), call `UpscaleResolved`, then mark the output written (`Image::NoteContentWrite`, `TextureCache::CommitGpuWrite`). A ten-second summary counts evaluations and fallbacks by reason. | ~200 lines |

Any precondition failure leaves the game's own pass to run: DLSS off or not loaded, unexpected
bindings, no depth known, motion vectors unused this frame, `Choose` finds no quality mode for the
size, creation or evaluation failed.

Deviation from spec 2.21: the quality family is chosen by upstream `Dlss::Choose` from the render
and output sizes, so there is no quality variable. The preset letter stays.

Consequences for the product: no NGX SDK at build time; the GPL renderer contains no NVIDIA code; the
only NVIDIA file shipped is the runtime `libnvidia-ngx-dlss.so.310.9.16`, copied by the runner into
`libs/`; the bridge passes the folder of its own library as `dlss_dir` (found with `dladdr`).

Verification aids: upstream log line "DLSS: upscaling PSSR with the model in <folder>" at start; our
ten-second summary; the driver indicator `__NGX_SHOW_INDICATOR=1024`.

### 6.5 Live settings and the session socket

`src/streaming/transport.h:37` takes a `SettingsHook` (`std::function<Json(const Json& apply)>`).
The seam (`kytyEmbed.cpp`) constructs `Streaming::Transport` after the Graphics lifecycle when
`AGS_SESSION_SOCKET` is set (`src/streaming/sessionEnv.h`), with a hook that serialises the payload,
calls `hooks->live_setting`, and parses the reply. `bridge/src/live.rs` owns the map:

| Setting key | Action |
|---|---|
| `display.fps` | Set the limiter's limit (which also moves the low-latency interval). If the cap crosses the 30 / 60 group, toggle the patch groups through the patcher library found via `LD_AUDIT` (`astro_patch_live` in `patcher/src/lib.rs`). |
| `display.resolution` | `api.set_window_size(w, h)`; consumed by the window loop (`WindowContext::Run`, `window.cpp:839`, ~10 lines for the pending size). |
| `controller.haptics` | Store the gain atomic read by `ExternalRenderer.hapticGain`. |

---

## 7. Configuration

### 7.1 Variables read by the bridge (all new names; Kyty reads none of them)

An invalid value is a start-up error naming the variable and the accepted range, never a silent
default.

| Variable | Meaning | Default |
|---|---|---|
| `ASTROPORT_WIDTH`, `ASTROPORT_HEIGHT` | Window size, 1..7680 | Kyty's defaults (1280x720) |
| `ASTROPORT_FULLSCREEN` | `1`: desktop fullscreen, hidden cursor | 0 |
| `ASTROPORT_PRESENT_MODE` | `immediate`, `fifo`, `mailbox` | immediate |
| `ASTROPORT_SHADER_OPT` | `none`, `size`, `performance` | performance |
| `ASTROPORT_VBLANK_HZ` | Guest vblank frequency | 60 |
| `ASTROPORT_WAYLAND_NATIVE` | `1`: Wayland video driver first, true-resolution surface | 0 |
| `ASTROPORT_APP0` | Game folder (title id, icon, executable for the pipeline list) | `app0` under the working directory |
| `ASTROPORT_WINDOW_TITLE` | Fixed window title | unset |
| `ASTROPORT_FPS_LIMIT` | 10..240 or `unlimited` | unset = no limiter |
| `ASTROPORT_LOW_LATENCY` | `1`: NVIDIA low-latency mode; the interval follows the frame limit | 0 |
| `ASTROPORT_DLSS` | `1`: replace the upscale pass | 0 |
| `ASTROPORT_DLSS_PASS` | Pixel shader hash of the pass (hex) | unset = DLSS cannot engage |
| `ASTROPORT_DLSS_REVERSE_DEPTH` | `1`: the title's depth is reversed | 0 |
| `ASTROPORT_DLSS_DIR` | Folder of the DLSS runtime | folder of the bridge library |
| `ASTROPORT_DLSS_PRESET` | Preset letter `A`..`O` | model default |
| `ASTROPORT_PAD_PREFER` | `auto`, `ps`, `xbox`, `switch`, `generic` | unset = first pad |
| `ASTROPORT_RUMBLE_SCALE` | Percent 0..200; also seeds vibration intensity | 100 |
| `ASTROPORT_RUMBLE_SWITCH_FLOOR` | 0..255 | 0 |
| `ASTROPORT_DS_RUMBLE` | `0`: never send motor rumble to a DualSense | 1 |
| `ASTROPORT_FSR_FG`, `ASTROPORT_FSR_DIR` | FSR frame generation (M10) | 0, folder of the bridge library |

Title facts (the shader hash `2936e5eca99d75e7`, reversed depth) live in `patches/settings.json`,
not in an engine.

### 7.2 Variables read by AnyPS5 (its `APS5_` convention; each gets a row in `docs/dev/ENVIRONMENT.md`)

| Variable | Meaning | Default |
|---|---|---|
| `APS5_PAD_TRIGGER_EFFECT_STATE` | `1`: compute trigger effect state (F10) | off |
| `APS5_SAVEDATA_TRANSFER_AS_MOUNT` | not `0`: the transfer mount falls back to a regular mount (F14) | off |
| `APS5_PAD_VIBRATION_GAIN`, `APS5_PAD_VIBRATION_VIA_HOST`, `AGS_CONTROLLER_AUDIO_SINK` | ours already; names kept | as before |

### 7.3 Kept, removed

Kept names (upstream or ours, nothing to implement): `KYTY_HANG_WATCHDOG`, `KYTY_CP_PRIORITY`,
`KYTY_DRAW_PREP_SPIN_US`, `KYTY_DRAW_PREP_COLD_SPIN_US`, `KYTY_BUFFER_RANGE_FIXES`,
`KYTY_AGC_DONE_MODE`, `KYTY_PIPELINE_LIST_PREPARE`, `KYTY_PIPELINE_LIST_CROSS`,
`KYTY_PRESENT_ABS_VBLANK`, `ANYPS5_LANGUAGE`, `AGS_SESSION_SOCKET`, `AGS_SESSION_PROTOCOL`,
`LD_AUDIT`, `ASTRO_PATCH_PROCESS`, `GLIBC_TUNABLES`. The last one stays:
`glibc.rtld.optional_static_tls=0x200000` is required whenever the patcher is loaded with `LD_AUDIT`
(1.1); without the tunable the game starts only without the patcher (`scripts/play.sh`:
`STATIC_TLS=0` works only with `CHEATS=0`).
Driver variables used as is: `NVPRESENT_ENABLE_SMOOTH_MOTION`, `__NGX_SHOW_INDICATOR`,
`DISABLE_LSFGVK`.

Removed without replacement: the engine switch and the two module path variables, the cache title id
variable, the motion switch, the separate low-latency interval, the floating donation size,
`KYTY_DRAW_PREP_PRIORITY` (no effect on Linux), the commit-epoch switch, and every variable with the
removed project's prefix. `KYTY_WAYLAND_NATIVE` becomes `ASTROPORT_WAYLAND_NATIVE` (it was ours and
is no longer read by Kyty).

### 7.4 Changes outside the engines

| File | Change |
|---|---|
| `patches/settings.json` | `fixed_env`: drop the removed variables; rename the rest per 7.1 / 7.2; add `ASTROPORT_APP0={run}/app0`, `ASTROPORT_DLSS_PASS=2936e5eca99d75e7`, `KYTY_PRESENT_ABS_VBLANK=1`. `settings`: `display.fps` sets only `ASTROPORT_FPS_LIMIT`; `graphics.ultrawide` -> `ASTROPORT_WIDTH/HEIGHT`; `controller.rumble` -> `ASTROPORT_DS_RUMBLE`; `perf.low_latency` -> `ASTROPORT_LOW_LATENCY`; `fullscreen` -> `ASTROPORT_FULLSCREEN`; `graphics.dlss` -> `ASTROPORT_DLSS`. |
| `adapter/src/launch.rs`, `settings.rs` | The same renames where names are built in code; the low-latency interval assertion in the tests goes. Nothing structural. |
| `runner/src/layout.rs` | `install_libs` reads the install tree of 1.4 (`--install`): copy every `.prx` of `anyps5/libs` except the three graphics ones (`libAstroportHost.prx` is required), then from `lib/` the three forwarders, `libkyty_renderer.so`, `libastroport_bridge.so` and any `libnvidia-ngx-dlss.so.*`, all into the run folder's `libs/` as copies; files it did not install are pruned from `libs/`. A hand refresh of a run folder must keep that order: AnyPS5's libraries first, `install/lib/*` last, so the forwarders win over AnyPS5's files of the same name. |
| `runner/src/build.rs` | Import check against `lib/astroport-exports.txt` across the three forwarders (4.2), with a unit test on a small registry. |
| `scripts/build-engines.sh` | 1.4. |
| `scripts/prx-targets.txt` | Lines 1 and 371: `libAstroportHost.prx`. |
| `scripts/release/inside.sh` | Drop `-DKYTY_NGX_SDK_DIR` (line 18). |
| `scripts/release/fetch-dlss.sh` | Require only the runtime `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.*` and `LICENSE.txt`; sparse checkout shrinks accordingly. The pin in `dlss.env` is unchanged. |
| `scripts/release/stage.sh` | New file names; no `shims/`; the NGX symbol check (line 83) is replaced by "the runtime file is present"; `ASTROPORT_REQUIRE_DLSS=1` fails when it is not. |
| `scripts/release/test.sh`, `package.sh` | Follow the above (no fake NGX symbol, no fake static library). |
| `scripts/play-hybrid.sh` -> `scripts/play.sh` | Rewritten on the new names; `KYTY_SO` goes (use the run folder's `libs/`). `RUN_DIR` names the run folder; `ASTROPORT_*` variables already set win over its defaults; it sets `GLIBC_TUNABLES` unless `STATIC_TLS=0` (7.3). |
| `scripts/smoke.sh` (new) | 1.2. |
| `scripts/check-repo.sh` | Required-file list updated; a grep that no file outside `docs/rewrite/` contains the removed project's prefix or library names; the K6 grep. |
| `README.md`, `NOTICE.md`, both `UPSTREAM.md` | Describe the new layout; `UPSTREAM.md` section 3 and 4 list our carried changes only. |

---

## 8. Milestones

Each ends in a check that runs on this machine. Run-based checks use `scripts/smoke.sh` (windowed,
`timeout`, `DISABLE_LSFGVK=1`, grep of the single log). "Files" is what the milestone owns;
milestones that own disjoint files can run in parallel.

**State: M0 to M4 are done** (the tree builds with `scripts/build-engines.sh`, all three build checks
pass, and the game starts on the bridge and reaches the title level). Measured on the development
machine (RTX 3090, windowed 1280x720, warm caches): the log shows "astroport: adopted 3/3",
"astroport: renderer ready in N ms" with N about 700, and "astroport: first frame presented after
N ms" with N about 1400, both counted from the bridge's constructor. M5 to M11 are open.

| # | Milestone | Verified by | Files | Parallel with |
|---|---|---|---|---|
| M0 | **Done.** Baseline and unknowns. Pristine engines build; bridge skeleton with `nidscan` + `nidgen`; forwarders link; constructor experiment; `smoke.sh`. | `ninja -C build/kyty kyty_emulator dlss_tests && ctest --test-dir build/kyty -R '^dlss$'` passes; AnyPS5 `prx-targets` build; `cargo test` in all four crates (the scanner finds 200 `LIB_FUNC` lines, three libraries, 17 memory functions on the real tree); three forwarders assemble and link against a stub bridge, and a C program linked against them calls one NID through the stub. Constructor experiment: a stub library as `DT_NEEDED` whose constructor spawns a thread that loads `libvulkan.so.1`, once with the constructor waiting and once not; result written into this document (decides 3.5). | `bridge/` skeleton, `scripts/smoke.sh` | - |
| M1 | **Done.** Kyty seam | `ninja -C build/kyty kyty_renderer`; `nm -D --defined-only` shows only `kyty_embed_api`; `ninja kyty_emulator` still links. A Rust example (`bridge/examples/embed_smoke.rs`) loads the library alone with `dlopen` (so it needs `GLIBC_TUNABLES=glibc.rtld.optional_static_tls=1048576`, see 1.1), calls `run` on a second thread, receives `reserved` then `ready`, a black window appears, `find("23LRUSvYu1M")` is non-null, `fns.equeue_create` / `equeue_wait` and `fns.pad_read_state` work from a plain thread, `quit` ends it with exit code 0. No AnyPS5 involved. **This is also the upstream-merge check.** | `engines/kyty/src/embed/`, `CMakeLists.txt`, 2 hooks | M2, M3 |
| M2 | **Done.** AnyPS5 seam | New ctest cases under `core/libs/tests/` (`guest_astroport_host_*`, in a tree configured with `-DBUILD_TESTING=ON`): a donation leaves the range unmapped and marked used; a non-free donation returns 0 and names the conflict; event add / deliver / delete on a real event queue with EOP and flip semantics; a fake `ExternalRenderer` receives the 17 calls and `owns` gating works; an APR read into a write-protected page succeeds. | `core/libs/prx/libAstroportHost/`, `libkernel/` (3 files) | M1, M3 |
| M3 | **Done.** Bridge core against fake tables | `cargo test`: thunk ABI test (a C-ABI function with six integer, eight double and variadic arguments is called through a thunk before and after binding and sees all values); strict config parsing; limiter (a 30 FPS cap delivers 30.00 over simulated timer overshoot; a late frame restarts cadence; video override; interval derivation); pad output differences; quaternion fallback; event registry; fault handler with a prior handler installed first: an unowned page reaches the prior handler, an owned page reaches the fake `fault`. | `bridge/src/` | M1, M2 |
| M4 | **Done.** Integration: first frame | `scripts/build-engines.sh` passes its three checks; the run folder starts with `LD_BIND_NOW=1` without unresolved symbols; `smoke.sh` finds "donated", "adopted 3/3", "renderer ready in N ms" (about 0.7 s) and "first frame presented after N ms" (about 1.4 s warm), well inside the 120 s limit; pipeline cache files are named after the title id; one run without `GLIBC_TUNABLES` (result: needed with the patcher, 7.3); one run on X11 shows the window icon. | `scripts/build-engines.sh`, `runner/` | - |
| M5 | MUST complete | Pad input moves the menu; closing the window ends the process with code 0 within 3 s and cache files have a fresh time stamp; the exit summary shows handled faults > 0; a ten-minute session streams assets without APR errors and without an "unforwarded memory call" line. | `bridge/src/pad.rs`, `events.rs` fixes | - |
| M6 | Host SHOULD features | Rumble, light bar, adaptive triggers on a DualSense; a motion section responds; unit test of the F10 table; Bluetooth DualSense speaker and haptics, no deadlock after ten minutes; a first save is created; O9 helper unit test. | `libScePad`, `libSceAudioOut`, `libSceSaveData.native` | M7, M8, M9 |
| M7 | Kyty presentation features | Limiter measured at 30.0 and 60.0 FPS (`KYTY_PACE_LOG` or the session metrics); window title fixed; the driver's latency report shows filled time stamps with the mode on, "unavailable" is logged without the extension; a second pad takes over on a button press; a live frame cap and haptic gain change during play; the studio intro plays in sync at caps 30, 120 and off (A5 + K9); Wayland native surface at true resolution. | `engines/kyty/src/graphics/presentation/`, `src/libs/controller.cpp`, `dualSense*.cpp`, `libSceAvPlayer`, `bridge/src/live.rs` | M6, M9 (not M8: shares `vulkanWindow.cpp`, `swapchain.cpp`) |
| M8 | DLSS | 8a spike, written down, no code kept: with the gate open, does NGX start on this machine ("DLSS: upscaling PSSR with the model in"); does device creation still succeed with the arena reserved (the address-window question); which of the four render sizes does `Choose` accept; formats of the pass's colour, motion, depth and output images; is depth reversed. 8b: the ten-second summary shows evaluations and zero fallbacks during play, `__NGX_SHOW_INDICATOR=1024` shows the indicator, and with the runtime file removed the game's own pass runs. `dlss_tests` pass with the new cases. | `engines/kyty/src/graphics/host_gpu/renderer/`, `guest_gpu/pssr.h`, release scripts | M6, M9 |
| M9 | Small ideas | K1 unit test + log line; K7 on X11 and Wayland; K8b unit test; K6 grep; A4 unit test; K5 then K8a (unit tests; counter of system-memory images under a budget cap); A1 then A2 / A3 with their test suites; K3 and K4 confirmed from the M4 log ("Pipeline list" lines). | disjoint per idea: `libc/` (A1-A3), `libSceAudioPropagation`, `vma.cpp` + `textureCache` (K5 / K8a), `renderDraw.cpp` draw guard (K8b; after M8), `controller.cpp` (K1; after M7), `window.cpp` (K7; after M7) | M6; A1-A3 with everything |
| M10 | Frame generation | Per decision 1. Smooth Motion setting: `VK_LOADER_DEBUG=layer` shows `VK_LAYER_NV_present` loaded (cannot be exercised on the RTX 3090). K2: porting spike result written down; if positive, the ten-second line shows about 2.0 frames shown per game frame and validation layers are clean on resize. | `window/frameGen.*`, a fetch script for AMD's source | after M8 |
| M11 | Product wiring and release | `scripts/check-repo.sh` passes including the prefix grep; `scripts/release/test.sh` passes; `scripts/build-release.sh` produces the tarball; AGS installs and launches it; every setting in `patches/settings.json` has a visible effect. | `adapter/`, `patches/`, `scripts/release/`, docs | from M4 on |

Critical path: M0 -> (M1, M2, M3 in parallel: three different trees) -> M4 -> M5. Everything after M5
is independent work on a running game, limited only by the shared files named in the table.

---

## 9. Risks and early detection

| # | Risk | Detected by | Fallback |
|---|---|---|---|
| 1 | Start-up constructors run under the loader lock, or a lifecycle before Memory loads a library; the 10 s wait for `reserved` then times out. | M0 constructor experiment (3.5): does not occur; the abort message names the step. | Do not wait in the constructor; rely on the adoption check after `ready` only. |
| 2 | Kyty does not adopt an unmapped range (upstream changes `ReserveGuestRegions`, or something maps into the gap). | The `EEXIST` check aborts with the range; M1 covers the scan itself. | Fix at the seam; the check is the guard. |
| 3 | A fixed donation is not free (something already lives at `0x200000000` or `0xDC00000000` when the constructor runs). | `donate` returns 0 and prints both ranges (M2 test, M4 run). | Move the general range (one constant). For the system-managed range there is no alternative; the message says what was there. |
| 4 | Kyty subsystems or HLE helpers expect a Kyty thread (thread-local state, profiler, watchdog) when called from bridge threads or guest threads. | M1 calls the helpers from a plain thread; M4 / M5 play test with `KYTY_HANG_WATCHDOG=0`. | Create pump and relay through a seam function that uses Kyty's own thread helper. |
| 5 | A guest call waits for a renderer that never becomes ready. | The 120 s wait prints the export and aborts. | - |
| 6 | `EmergencyShutdownActive` does not persist the pipeline or program cache. | M5 time-stamp check and a second launch without recompiling. | One extra save call in `flush`. |
| 7 | A fault handler installed later does not chain; or Kyty needs a branch of `TryHandleGuestAccessFault` the seam did not carry (lines 782-837). | M3 chain test; the exit summary's handled / passed-on counters; a crash at a GPU-visible address in M5. | Port the missing branch into `fault`. |
| 8 | Kernel writes into protected pages outside APR (spec R5). | Any "returned N of M bytes" or `EFAULT` in the M5 session. | The same staging pattern at that call site; a general host-write capability only if a second path appears. |
| 9 | Unforwarded memory functions matter (spec R2). | The one-time log line of 2.2. | Forward the specific function; one line each. |
| 10 | The static TLS tunable is still required. | M4 runs with and without `GLIBC_TUNABLES`. Result: required with `LD_AUDIT` (7.3). | Taken: it stays in `fixed_env` and in `scripts/play.sh`. |
| 11 | Symbol leakage from the renderer (SDL2 against SDL3). | Build check 1 on every build. | - |
| 12 | Stale forwarder or bridge after a Kyty update changes the `LIB_FUNC` set; scanner misreads a changed macro. | `astroport_entry_<hash8>` mismatch at link or load; `find` aborts naming the NID; scanner errors on an empty library; the runner's import check. | Fix the scanner. |
| 13 | NGX's device extension needs a large free address window below 1 TiB, and the arena plus Kyty hold `0x10000..0xFC00000000` (spec R4). | M8a: device creation with the NGX extensions. | A third `donate` made **after** `reserved` (for example 48 GiB below `0xDC00000000`): the arena marks it used and unmaps it, Kyty has already scanned, so it stays free for the driver. No engine change. |
| 14 | `Dlss::Choose` refuses the two middle render sizes (2432x1368, 3328x1872), or the pass's image formats or depth convention do not suit NGX. | M8a list; the fallback counter by reason in 8b. | Build the `Creation` for the pass's own sizes directly instead of through `Choose` (same module, still no SDK). |
| 15 | The shader hash or constant layout changes with a game update. | The DLSS summary shows zero evaluations; the game's pass runs. | The hash is product data in `patches/settings.json`. |
| 16 | NGX refuses the unmodified runtime or the writable folder. | M8a log. | `__NV_SIGNED_LOAD_CHECK` is not touched; use the SDK's `rel` library as is. |
| 17 | The low-latency sleep blocks a guest thread up to 100 ms and assumes one flip per submission. | Sleep time-out counter in the ten-second report (M7). | The mode is a setting; off is the upstream path. X11 was not checked for the extension. |
| 18 | `app0/sce_sys/param.json` or `app0/eboot.bin` is missing in the run folder (archive-only `app0.zar`), or loading `param.json` changes Kyty behaviour beyond the title id. | M4 log: empty title id means caches are off; "cannot be read: ignored" for the list. | One more config field for the title id. |
| 19 | Whole-emulator linkage: static initialisers of unused subsystems run at load; a static dependency is not position independent. | M1, the first build and smoke test. | Per-dependency PIC; worst case drop unused HLE sources from the renderer's source list. |
| 20 | K7: a hidden Wayland surface never presents and the prepare run hangs. | M9 test on Wayland under `timeout`. | Immediate present mode for the prepare run (already planned). |
| 21 | Frame generation: AMD's Vulkan swapchain is Win32-bound; no Linux port exists. | M10 spike before any implementation. | Decision 1: lsfg-vk and Smooth Motion stay the shipped answer. |
| 22 | Our earlier engine features are under-specified and the clean-room rule forbids reading our own old commits. | Now. | Decision 3. |
| 23 | lsfg-vk on the development machine doubles frame counts and forces FIFO for the process name `AstroBot`. | Would show as 60 FPS at a 30 cap. | `smoke.sh` always sets `DISABLE_LSFGVK=1`. |
| 24 | Upstream merges touch the edited lines. | By design the seam edits are small: Kyty 3 upstream files / 57 lines (plus four one-line build fixes), AnyPS5 3 upstream files / ~50 lines. | Merge check = `kyty_renderer` builds, M1 smoke test and M2 ctest pass. |

Not built on purpose: lazy start, a host-driven init / shutdown lifecycle, memory register /
unregister / invalidate entries, a log callback (each side writes stderr), vblank events, multi-port
relay, Ctrl+F2 limit cycling, a floating donation, a DLSS quality variable, native DLSS frame
generation (no Linux runtime exists), a Cargo workspace, any Windows branch.

---

## 10. Decisions for the owner

Only choices the facts leave open.

| # | Decision | Options | Recommendation |
|---|---|---|---|
| 1 | **Frame generation (idea K2).** AMD ships FSR frame generation as Windows binaries only. Its MIT source for FSR 3.1 (not FSR 4) is public but includes `<Windows.h>` unconditionally in the swapchain, has no build files and no compiled shaders. Native DLSS frame generation does not exist on Linux. Two zero-code paths work today: lsfg-vk (any GPU, already documented by the product) and NVIDIA Smooth Motion (`NVPRESENT_ENABLE_SMOOTH_MOTION=1`, RTX 40 and newer, cannot be tested on the RTX 3090). | (a) Port AMD's FSR 3.1 frame generation to Linux ourselves: a small Win32 compatibility layer for about ten primitives, our own build of `api` + `backend/vk` + `framegeneration/fsr3`, HLSL to SPIR-V with `dxc`, then the ~1400-line renderer integration of the spec. The largest item of the whole plan, with unknown pacing behaviour under Wayland. (b) Ship lsfg-vk plus a Smooth Motion setting as the product's frame generation and close K2 as "not available on Linux". (c) Both: (b) now, (a) time-boxed. | **(c)**: add the Smooth Motion setting in M10 (one variable in `patches/settings.json`), keep lsfg-vk, and give the port a three-day spike ("does AMD's source compile and create a swapchain context on Linux"). Continue only if the spike is positive. The capture layer it needs is shared with DLSS, so nothing is wasted. |
| 2 | **Redistributing the DLSS runtime.** The renderer now contains no NVIDIA code, but `libnvidia-ngx-dlss.so.310.9.16` is under NVIDIA's SDK licence, and the open checkbox in `docs/release.md:45` is still unticked. The licence text was not read for this design. | (a) Ship the runtime in the release tarball. (b) Do not ship it; the user supplies a folder (`ASTROPORT_DLSS_DIR`) or the runner fetches it at install time. | Read `LICENSE.txt` and tick the box, then **(a)**. Until then build with (b); the design supports both without code change. |
| 3 | **Our own removed engine features.** Several changes we wrote ourselves are not in the trees and are described in one paragraph each: the native frame exporter (3.10), the DualSense Bluetooth / speaker edits (3.1, 3.5), the Wayland native surface (3.3), the output-size part of live settings (3.9), and on the AnyPS5 side O2-O5, O9, O10. The clean-room rule forbids reading history before `0c140c49`, including our own commits. | (a) Allow implementers to read our own commits (authors Ali / Claude) restricted to files the third party never touched. (b) The owner exports those diffs into `docs/rewrite/own-patches/`. (c) Extend the specs and write everything again. | **(b)** for the frame exporter and the DualSense edits (hardware-tuned code that is expensive to rediscover); (c) is acceptable for the rest, which the specs cover well enough. Needed before M6 and M7. |
| 4 | **DLSS input route.** The baseline recognises the game's upscale pass by shader hash (title facts of spec 2.21). An alternative may exist: run the game in PS5 Pro mode so that it calls `libScePsml` and hands over depth, motion and jitter itself; the forwarder would then route `libScePsml` to Kyty's own `libPsml` and upstream's `ProcessPssr` would do everything with no title-specific code. Unknown whether Astro Bot 1.019 calls it; Pro mode also changes how the game renders, and the fallback would be upstream's bilinear pass instead of the game's own. | (a) Shader-hash hook (6.4). (b) Pro-mode route. | **(a)**. Add a half-day experiment to M8a (trace calls with `APS5_TRACE_CALLS` in Pro mode) and revisit only if the game does call it. |
| 5 | **Game data for integration runs.** No run folder was found, and the existing game folder under `~/Games` is off limits for this work. M4 and every later run-based check need a run folder built by `runner` from a dump. | The owner provides a dump path (or a ready run folder) outside the forbidden folder. | Provide it before M4; M0 to M3 do not need it. |
