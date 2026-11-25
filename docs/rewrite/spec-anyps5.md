# Functional specification: the AnyPS5 side of hosting an external renderer

Clean-room specification. It describes what the AnyPS5 changes of the AstroHybrid snapshot achieve
and why, so that they can be re-implemented on the pristine upstream without reading the original
code. It contains no code from the snapshot. Identifiers that exist in upstream AnyPS5 at
`319621ba2e` are named freely; identifiers introduced by the snapshot are named only where a reader
needs the name to find a hook point or an environment variable.

Conventions used below:

- "upstream" = boykopovar/AnyPS5 at `319621ba2e`. "snapshot" = `2429864775` (branch `linux-native`).
- "host" = the AnyPS5 process side (relinked game plus AnyPS5 system libraries).
- "renderer" = the external GPU engine (today Kyty's renderer built as `libkyty_gpu.so`).
- "(inferred)" marks statements deduced rather than read in code or measured on the run folder.
- Paths are relative to the AnyPS5 tree root unless they start with `/`.
- Recommendation scale: MUST (the game does not run or the product breaks without it), SHOULD
  (product feature or measurable gain), DROP (diagnostic, experiment, Windows-only, not executed with
  the Kyty engine, or unused).

Evidence for run-time statements comes from the built run folder
`~/Games/astrobot-pc/native/run-hybrid-1019` (inspected with `readelf -d` and `nm -D`) and
from the product launch configuration (`patches/settings.json`, `adapter/src/launch.rs`,
`scripts/play-hybrid.sh`, `runner/src/{build,layout}.rs`).

---

## 1. How hosting the external renderer works today

### 1.1 Upstream facts the integration relies on

| Fact | Where (upstream) |
|---|---|
| The relinker turns `eboot.bin` and every `sce_module/*.prx` into a native ELF plus `app0/sce_module/<name>.guest.prx`. Imports stay bound **by NID symbol name**; the OS loader resolves them against `libs/*.prx` at process start. | `core/relinker`, `docs/dev/ARCHITECTURE.md` |
| The relinked executable has `RUNPATH $ORIGIN/libs` and one `DT_NEEDED` per system library **by soname** (`libSceAgc.prx`, `libSceAgcDriver.prx`, `libSceVideoOut.prx`, `libkernel.prx`, `libScePad.prx`, `libSceAudioOut.prx`, ...). Bundled guest modules are needed by path (`$ORIGIN/app0/sce_module/...`). | run folder, `readelf -d AstroBot` |
| Every directory under `core/libs/prx/` becomes a shared library target named after the directory, output `core/libs/libs/<name>.prx`. A directory with its own `CMakeLists.txt` is added with `add_subdirectory`; otherwise `Export.cpp` alone is compiled. Every such target links `libkernel` (private) and `libc` (public), gets `-z now` on Linux, RPATH `$ORIGIN`, and is post-processed by `nid_patcher`. | `core/libs/CMakeLists.txt` |
| `nid_patcher` renames exports to NIDs. A symbol ending in `_nid_no_patch` keeps its literal name. A symbol ending in `_nid_postfix` is exported under the NID of its name without the suffix (inferred from `StripNidPostfix` in `core/libs/nid/include/nid/NidPatcherUtils.hpp`). This is how AnyPS5 libraries call each other's internal helpers across `.prx` boundaries. | `core/libs/nid` |
| On Linux `GuestArena::GuestAddressOffset` is 0: guest addresses are PS5 addresses. | `core/libs/prx/libc/include/GuestArena.hpp` |
| libc's guest arena (`Arena` in `core/libs/prx/libc/src/GuestArena.cpp`, constructed by a static initialiser when `libc.prx` loads) reserves `0x10000 .. 0xFC00000000` on Linux with `PROT_NONE`, `MAP_NORESERVE`, `MAP_FIXED_NOREPLACE` mappings in 1 GiB chunks, skipping whatever the process already maps. The PS5 system-reserved range `0x7FFFFC000 .. 0x1000000000` is recorded as used and as a host region but is **not** mapped by the arena. Default allocations start at `0x200000000`. Public surface: `GuestArenaAllocate*`, `GuestArenaMarkUsed`, `GuestArenaRelease`, `GuestArenaHighestFree_nid_no_patch`, `GuestArenaNextUsed_nid_no_patch`, `GuestArenaRange`, `GuestArenaBeginHostWrite` / `GuestArenaEndHostWrite` and the `GuestArena::HostWrite` guard. | same file and header |
| libkernel's direct and flexible memory exports live in one file: `sceKernelAllocateDirectMemory`, `sceKernelAllocateMainDirectMemory`, `sceKernelDirectMemoryQuery`, `sceKernelGetDirectMemorySize`, `sceKernelMapDirectMemory`, `sceKernelMapDirectMemory2`, `sceKernelMapNamedDirectMemory`, `sceKernelMapFlexibleMemory`, `sceKernelMapNamedFlexibleMemory`, `sceKernelReleaseDirectMemory`, `sceKernelAvailableFlexibleMemorySize`, `sceKernelConfiguredFlexibleMemorySize`, `sceKernelMprotect`, `sceKernelMunmap`, `sceKernelVirtualQuery`, `sceKernelSetVirtualRangeName`, `sceKernelClearVirtualRangeName` (and others not touched: `sceKernelAvailableDirectMemorySize`, `sceKernelGetDirectMemoryType`, `sceKernelReleaseFlexibleMemory`, `sceKernelReserveVirtualRange`, `sceKernelMmap`, `sceKernelBatchMap`, `sceKernelMtypeprotect`, `sceKernelQueryMemoryProtection`, ...). | `core/libs/prx/libkernel/DirectMemory/Export.cpp` |
| Event queues: `EqueuePin_nid_postfix`, `EqueueAddEvent_nid_postfix`, `EqueueTriggerEvent_nid_postfix`, `EqueueDeleteEvent_nid_postfix`, with `KernelEqueueEvent` carrying a trigger function and a reset function. Upstream's own GPU event sources are the reference for behaviour: `sceAgcDriverAddEqEvent` / `sceAgcDriverDeleteEqEvent` and `AgcDriverDeliverEopInterrupt` in `core/libs/prx/libSceAgcDriver/Eq/src/Event.cpp` (filter −14), `registerVideoOutEvent` in `core/libs/prx/libSceVideoOut/src/Event.cpp` (filter `EVFILT_VIDEO_OUT` = −13). | `core/libs/prx/libkernel/Equeue/Equeue.hpp` |
| Pad library internals: `PadPublishInputForPort_nid_postfix(int port, const PadInputState&)`, `PadFetchOutputForPort_nid_postfix(int port, std::uint32_t* seenSequence, PadOutputState* out)`, `PadSetPortConnected_nid_postfix`, `PadSubmitAudioHaptics_nid_postfix`, `Pad::SetTriggerCommand`, `scePadSetTriggerEffect`, `scePadGetTriggerEffectState` (upstream returns an all-zero state). In upstream it is **libSceVideoOut** that owns the window, reads SDL input and publishes it (`core/libs/prx/libSceVideoOut/src/PadInput.cpp`), applies pad output, and calls `LibcRequestExit_nid_postfix(0)` when the window closes (`VideoOutDriver.cpp`). | `core/libs/prx/libScePad/include/PadState.hpp`, `Export.cpp`, `src/PadState.cpp` |
| Process end: `LibcRequestExit_nid_postfix(int code)` is idempotent, requests the shutdown token and exits on a detached thread through `LibcExitWithoutHandlers_nid_no_patch`. | `core/libs/prx/libc/src/Process.cpp`, `include/Shutdown.hpp` |
| HDR status between libraries: `AgcDriverHostHdr_nid_postfix(AgcDriver::HostHdrStatus&)` and `AgcDriverNoteReportedToneMapPeak_nid_postfix(float)` are exported by upstream's `libSceAgcDriver.prx` and imported by `libSceSystemService.prx` (`core/libs/prx/libSceSystemService/Export.cpp`, via `AgcDriver::CurrentHostHdr()`). Their NIDs are `doCCHPPfxkI` and `KMphJjTcNgk`. | `core/libs/prx/libSceAgcDriver/Execution/include/HostHdr.hpp` |
| Libraries other than the game that import graphics symbols: `libSceJobManager.prx` (links `libSceAgc`, `libSceAgcDriver`; uses `sceAgcDriverAddEqEvent`, `sceAgcDriverDeleteEqEvent`, `sceAgcDriverSubmitAcb` and eleven `sceAgc*` command-buffer helpers) and `libSceSystemService.prx` (the two HDR helpers). Upstream `libSceVideoOut` also links `libScePad`, `libSceMouse`, `libSceKeyboard`, `libSceIme` and feeds them. | `core/libs/prx/*/CMakeLists.txt`; run folder `nm -D` |
| APR file reads into guest memory: `_readFile` in `core/libs/prx/libkernel/Apr/src/Apr.cpp` holds a `GuestArena::HostWrite` and, on Linux, lets the stream read straight into the guest destination; on Windows it reads through a 64 KiB staging buffer and copies. | same file |
| AudioOut: classic ports in `core/libs/prx/libSceAudioOut/src/AudioOut.cpp` (`sceAudioOutOutput`, `sceAudioOutOutputs`, `outputVirtualPort`, `paceVirtualPort`, `openVibrationDevice`, port types pad speaker and vibration); AudioOut2 in `AudioOut2Context.cpp` (`Render`, `OpenPadDevice`, `QueuePadGrain`), `AudioOut2Port.cpp` (`AudioOut2MixPorts`), `AudioOut2PadMix.{hpp,cpp}` (`AudioOut2RouteForPort`, `AudioOut2IsPadAudioDevice`, `AudioOut2RumbleFromPadFrames`, `AudioOut2FinishPadMix`). The pad mix has separate speaker-left/right and vibration-left/right channels. | as named |
| Save data: `sceSaveDataTransferringMount` is already exported and returns "not found" (static helper `transferringMount`); `mount3` implements the regular mount. | `core/libs/prx/libSceSaveData.native/Export.cpp` |
| Environment variables read by engine code must have a row in `docs/dev/ENVIRONMENT.md`; no comments in code; an export that cannot be implemented throws; never special-case one title. | `AGENTS.md`, `CONTRIBUTING.md`, `docs/dev/CONVENTIONS.md` |

### 1.2 The engine switch: there are two mechanisms, not one

**Mechanism A, load time: three replacement libraries.** The game's graphics imports are not
re-routed inside AnyPS5 at all. The run folder's `libs/` simply does not contain AnyPS5's
`libSceAgc.prx`, `libSceAgcDriver.prx` and `libSceVideoOut.prx`. In their place sit three tiny
forwarder libraries with the same file names and sonames (14 to 22 KiB each, against many MiB for
the real driver). Each forwarder:

- exports exactly the NIDs the game and AnyPS5's other libraries import from that library
  (today 66 from `libSceAgc`, 7 from `libSceAgcDriver`, 11 from `libSceVideoOut`; the list is the
  renderer's NID table, a title-specific set);
- implements each export as one tail jump to the renderer's entry for that NID;
- has `DT_NEEDED libkyty_gpu.so` and RUNPATH `$ORIGIN`, so the dynamic loader loads the renderer at
  process start, before any game code runs;
- the `libSceAgcDriver` forwarder additionally exports the two HDR NIDs (`doCCHPPfxkI`,
  `KMphJjTcNgk`) as tail jumps into the host module, and therefore also needs
  `libAstroHybridHost.prx`.

The forwarders are built outside AnyPS5 (today by a script in the renderer tree) and installed by
`runner` after AnyPS5's libraries so that they overwrite the three originals
(`runner/src/layout.rs`, `install_libs`: first `anyps5/libs/*.prx` and the host module, then
`shims/*.prx` and the renderer).

Because every AnyPS5 library is linked `-z now`, the forwarders must export **every** symbol that
`libSceJobManager.prx` and `libSceSystemService.prx` import from the libraries they replace;
one missing symbol makes the process fail at load.

**Mechanism B, run time: `APS5_GPU_ENGINE=kyty` inside AnyPS5.** The game imports its memory
functions from `libkernel.prx`, which is still AnyPS5's. With the variable set to exactly `kyty`
(read once, cached), seventeen libkernel memory exports hand the call to the renderer instead of
running AnyPS5's implementation (section 2, F3). The same variable also switches APR reads to a
staged copy (section 3, O1c).

The two mechanisms are independent and both are required. Setting the variable without installing
the forwarders gives a process in which AnyPS5's own driver renders while the renderer owns the
memory; installing the forwarders without the variable gives a renderer that never sees the guest's
GPU memory.

### 1.3 Does AnyPS5's own AGC driver or shader recompiler run with the Kyty engine?

**No. Neither is loaded into the process.** Evidence:

1. `readelf -d` on the run folder: `AstroBot` needs `libSceAgc.prx`, `libSceAgcDriver.prx`,
   `libSceVideoOut.prx` by soname with RUNPATH `$ORIGIN/libs`; the three files there are the
   forwarders, whose only dependencies are `libkyty_gpu.so` (and the host module for the driver
   forwarder).
2. The only other dependants are `libSceJobManager.prx` (needs `libSceAgc.prx`,
   `libSceAgcDriver.prx`) and `libSceSystemService.prx` (needs `libSceAgcDriver.prx`); they bind to
   the same forwarders. Every one of their imports from those sonames is satisfied by a forwarder
   export (checked symbol by symbol with `nm -D`).
3. The shader recompiler is a static library (`shader_recompiler_agc_driver`, created by
   `add_shader_recompiler` in `core/libs/prx/libSceAgcDriver/CMakeLists.txt`) linked only into
   `libSceAgcDriver` and its tests. No `.prx` in the run folder contains recompiler or Vulkan
   driver code (string scan of every `.prx` in `libs/` for recompiler and Vulkan host-import
   markers: no hit).
4. `core/libs/prx/libSceAgcDriver/**` and `core/shader/recompiler/**` contribute no code to any
   other library target. `libSceJobManager` only borrows include directories.

Consequence: all twenty `perf(agc)` / `feat(agc)` / `test(agc)` / `perf(recompiler)` / `perf(libc)`
commits (section 2.1) change code that is never executed in the product and can be dropped
entirely. This includes the libc parts of two of them (CPU access traps, write alarms), which are
Windows-only and are consumed only by AnyPS5's own driver. It also makes one piece of **our own**
commit dead code (the driver-area guard in the AnyPS5 driver constructor, section 3, O1d).

The libraries must still be **built** (the product build builds every `.prx` target and
`libSceJobManager` links against them), but the built `libSceAgc.prx`, `libSceAgcDriver.prx` and
`libSceVideoOut.prx` are overwritten in the run folder.

### 1.4 End to end

1. **Launch.** The launcher starts `./AstroBot` with the run folder as working directory and the
   fixed environment of `patches/settings.json`: `APS5_GPU_ENGINE=kyty`,
   `KYTY_GPU_LIBRARY={run}/libs/libkyty_gpu.so`,
   `KYTY_GPU_HOST_MODULE={run}/libs/libAstroHybridHost.prx`, `APS5_PAD_TRIGGER_STATE=1`,
   `APS5_SAVEDATA_TRANSFER_FALLBACK=1`,
   `GLIBC_TUNABLES=glibc.rtld.optional_static_tls=0x200000`, plus renderer variables, and per
   launch `APS5_PAD_VIBRATION_GAIN`, `ANYPS5_LANGUAGE` (upstream variable) and, when the session
   provides one, `AGS_CONTROLLER_AUDIO_SINK`. `LD_PRELOAD` and `LD_LIBRARY_PATH` are removed.
   `KYTY_SO` and `KYTY_LIB` are shell variables of the developer script only; no engine reads them.
2. **Dynamic loading.** The loader maps the executable, the guest modules and all needed `.prx`.
   The three forwarders pull in the renderer and the host module; the host module pulls in
   `libScePad.prx`, `libkernel.prx`, `libc.prx`. The renderer's static initialiser points every
   exported NID entry at a "not started yet" trap. libc's static initialiser reserves the guest
   arena. The renderer has a large initial-exec TLS block (about 580 KiB), hence the glibc tunable.
3. **First renderer call starts the renderer (lazy start).** The first call that reaches any
   renderer NID entry (from the game through a forwarder, or from libkernel through mechanism B)
   lands in the trap. The renderer then, once and under a lock: finds the host module (path from
   `KYTY_GPU_HOST_MODULE`, default file name `libAstroHybridHost.prx`; it first asks the loader for
   the already loaded copy, then loads it), calls the host module's single entry point to obtain the
   host's capability table, validates its version and size, binds all NID entries, and starts
   graphics. The trapped call is then completed as if it had gone to its implementation directly.
   Which call is first is not fixed; it is most likely an early memory call made by the guest libc's
   heap initialisation (inferred, not traced).
4. **Graphics start (inside that first call, on the calling game thread).** The renderer asks the
   host to hand over three guest address ranges (F2): the fixed 512 MiB range at `0xFE0000000`
   (it maps its own AGC driver memory there), the fixed range `0x200000000 .. 0x7FFFF0000`
   (the PS5 system-managed area where titles place fixed mappings; Astro Bot maps 2 GiB of direct
   memory at `0x300000000`), and a floating window of 64 GiB by default (renderer variable
   `KYTY_GPU_GUEST_GIB`) which the host places at the highest free 1 GiB-aligned position of the
   arena (normally `0xEC00000000 .. 0xFC00000000`). It then starts its renderer thread (window,
   Vulkan device, presentation loop; waits up to 120 s for it), an event pump thread, a pad relay
   thread, and installs its guest-memory fault handler in front of the process's existing handlers.
5. **Game initialisation.** `sceVideoOutOpen`, buffer registration and so on go straight to the
   renderer. `sceVideoOutAddFlipEvent` and `sceAgcDriverAddEqEvent` (from the game and from
   `libSceJobManager`) reach the renderer, which calls back into the host to register the event on
   the game's AnyPS5 event queue (F4). All direct and flexible memory calls reach the renderer
   through libkernel (F3).
6. **Frames.** The game writes command buffers into memory the renderer mapped and submits them to
   the renderer. The renderer raises end-of-pipe interrupts and flips; its pump thread hands each
   to the host, which triggers the matching events on the game's queues; game threads blocked in
   `sceKernelWaitEqueue` wake. The first presented frame needs nothing more from the host.
7. **Steady state services.** Every 4 ms the relay thread publishes pad state (and motion) to the
   host and fetches pad output (F7, F9, F11). AnyPS5's audio threads push controller speaker and
   haptic audio to the renderer (F13, O2).
8. **Exit.** When the player closes the window the renderer's window loop returns and it asks the
   host to exit with code 0; the host calls `LibcRequestExit_nid_postfix`. The renderer keeps its
   subsystems alive and idles until the process ends, because the game may still submit work.
   **The host never calls the renderer's shutdown entry**; the process exit tears the renderer down.
   A game-initiated exit ends the process the upstream way, again without a renderer shutdown.

### 1.5 What crosses the boundary (capabilities, not an ABI)

Direction "R→H" = renderer calls host, "H→R" = host calls renderer. "Game thread" = a guest thread
running relinked game code or an AnyPS5 library on its behalf.

| # | Capability | Dir | Data | Calling thread | Lifetime / notes |
|---|---|---|---|---|---|
| C1 | Discover the host | R→H | Module path (env or default name), one well-known entry symbol returning the capability table | Game thread that triggers lazy start | Once. Host table is static for the process. |
| C2 | Version negotiation | both | Interface major/minor, table size | same | Renderer refuses a different major or a table smaller than its oldest supported layout; missing newer capabilities are treated as absent. |
| C3 | Log | R→H | Level (info, warning, error), text | any renderer thread | Whole process. Host prints to stderr with a prefix. May be absent. |
| C4 | Hand over a guest address range | R→H | Size; optional exact base; returns base | Game thread, during graphics start, before the renderer thread exists | Three calls today. Permanent: never returned. |
| C5 | Register a GPU event on a game event queue | R→H | Queue handle (AnyPS5 `KernelEqueue`), kind (EOP interrupt or flip), ident (AGC event id or video-out handle), user data pointer | Game thread inside `sceAgcDriverAddEqEvent` / `sceVideoOutAddFlipEvent`; also `libSceJobManager` threads | Until deleted or process end. |
| C6 | Delete a GPU event | R→H | Queue handle, kind, ident | Game thread | — |
| C7 | Deliver a GPU event | R→H | Kind, ident, 64-bit payload | Renderer's event pump thread (not a guest thread) | Per interrupt / flip. Must be cheap and must not block on game threads. |
| C8 | Publish pad input | R→H | Port, buttons (PS5 bit layout), four stick bytes (128 = centre), L2 and R2 analog bytes, connected flag, two touch points (x, y, id, active) | Renderer's pad relay thread, about every 4 ms | From graphics start to process end. Port is always 0 today. |
| C9 | Publish pad motion | R→H | Port, acceleration x/y/z in m/s² including gravity, angular velocity x/y/z in rad/s | Relay thread, immediately before C8 for the same port | Only when the renderer's motion switch is on. |
| C10 | Fetch pad output | R→H | Port, caller-owned "last seen sequence" cursor; returns changed/unchanged and, when changed: game rumble large/small, audio-derived rumble large/small, light bar valid + RGB, per trigger "a command was set" + the raw 56-byte trigger command | Relay thread, every 4 ms | The renderer owns the physical controller and applies it. |
| C11 | Request process exit | R→H | Exit code | Renderer thread (window loop) | Once. |
| C12 | Memory calls of the game | H→R | The exact arguments of the 17 libkernel functions of F3, PS5 calling convention (System V), results returned unchanged | Game threads | Whole process. On Linux no calling-convention change is involved. |
| C13 | Graphics calls of the game | game→R | The NID exports of the three replaced libraries | Game threads, `libSceJobManager` threads | Through the forwarders; AnyPS5 code is not involved. |
| C14 | Queue controller audio | H→R | Sample rate, target (speaker or haptics), interleaved samples, frame count, channel count (1 or 2), sample format (float or 16-bit), gain; returns queued duration in microseconds, 0 = not accepted | AnyPS5 audio threads: AudioOut2 render path, the haptics worker thread, callers of `sceAudioOutOutput(s)` | Looked up by symbol name in the already loaded renderer; absent symbol = feature off. |
| C15 | Read live haptic gain | H→R | Returns a float (0..4 expected) | AudioOut2 render path | Ours (section 3, O10). Absent symbol = launch value. |
| C16 | HDR status for SystemService | SystemService→H | Fills `AgcDriver::HostHdrStatus`; notes the tone-map peak the title reported | Game thread | Routed by the driver forwarder. |
| C17 | Guest memory faults | implicit | The renderer's fault handler runs before the host's handlers for faults in memory it tracks and passes every other fault on | Any thread | No call crosses; it is a process-wide contract (section 4, R6). |

Present in today's interface but **unused or unimplemented**, do not carry over: a host callback to
query guest memory mappings (host passes none), a host callback returning the `app0` path (host
passes none), renderer entries to register / unregister / invalidate guest memory (return "not
implemented"), renderer entries to submit command buffers through the C interface (the game uses
the NID exports), NID table introspection, an explicit `init` / `start_graphics` / `shutdown`
lifecycle driven by the host (the host never calls any of them; start is lazy, shutdown never
happens), and a "vblank" event kind (defined, rejected by the host).

Cross-cutting requirements for any new interface:

- C++ exceptions must not cross it. `PadPublishInputForPort_nid_postfix` rethrows a stored input
  failure, and the arena functions throw on invalid arguments; the host side must catch at the
  boundary and decide (log and continue, or request exit).
- Only plain C data crosses. Today both sides duplicate the structure layouts and guard them with
  size assertions.
- The host must look the renderer up as the module the loader already loaded. Today libkernel
  opens it again by the path in `KYTY_GPU_LIBRARY`; if that path names a different file than the
  one the forwarders loaded, the process contains two renderers (section 4, R3).

---

## 2. Feature catalogue (faycal's 27 commits)

### 2.1 AnyPS5's own graphics driver and recompiler: 20 commits, all DROP

None of this code is loaded when the product runs (section 1.3). Every switch below is read only
inside `libSceAgcDriver` (or its static recompiler), so none of them has any effect in the product
whatever its value. The product sets none of them. The numbers in the commit messages are
measurements of AnyPS5's own Vulkan driver on Windows and say nothing about the Kyty engine.

| Commit | Subject (short) | Switch | Touches | Platform | Rec. |
|---|---|---|---|---|---|
| `4fcfa6a278` | draw snapshots in resizable-BAR memory | (doc row only) | `libSceAgcDriver/Graphics` (BufferPool, Context, Resources, Recorder, Draw, ShaderResources), tests | both | DROP |
| `624f0a8f77` | print `PUSH_MARKER` paths with per-frame draw and dispatch counts | `APS5_TRACE_MARKERS` | `Execution/src/Driver/Packets/PacketExecution.cpp` | both | DROP (diagnostic) |
| `da1d520d23` | skip copy-in of unchanged device-local regions; per-program dispatch trace filter; block-granular write-watch collect | `APS5_RESIDENT_STAGING`, `APS5_TRACE_PROGRAM`, `APS5_WINDOWS_BLOCK_COLLECT` | Dispatch, GuestMemory, VulkanDevice, GuestBufferMemory | both / Windows | DROP |
| `1d381cb088` | record `DMA_DATA` stores into GDS on the GPU instead of draining | `APS5_GDS_GPU_STORES` | `Packets/PacketMemory.cpp` | both | DROP |
| `67d70b0b59` | plain GPU stores for GDS; sampled queue choice; CPU write-back sync counter | `APS5_GDS_PLAIN_STORES`, `APS5_SAMPLE_QUEUE` | VulkanDevice, PacketMemory, QueueWorker, ShaderResources | both | DROP |
| `bf05858826` | bind GPU-produced draw buffers in place; CPU access trap for staged results | `APS5_DRAW_SKIP_GPU_SNAPSHOTS`, `APS5_RESIDENT_TRAP` | GuestBufferMemory, ShaderResources; **libc**: five trap exports in `GuestArena.{hpp,cpp}` and a caller-attribution helper in `specifics/windows/CrashReport.cpp` | libc part Windows-only | DROP (the trap is a measurement tool) |
| `0b4fd12522` | draws read GPU-produced buffers from the dispatch's device-local shadow | `APS5_DRAW_READ_SHADOWS` | GuestBufferMemory, ShaderResources, Draw, VulkanDevice | both | DROP |
| `06d1f8941f` | GPU copy-ins read the producing dispatch's video-memory shadow | `APS5_COPY_IN_FROM_SHADOWS` | GuestBufferMemory | both | DROP |
| `9f66f80d4f` | write alarms on resident guest pages instead of polling the write watch | `APS5_WRITE_ALARMS` | **libc** `include/WindowsMappings.hpp`, one stats export in `GuestArena.{hpp,cpp}`; `Execution/src/GuestMemory.cpp` | Windows-only | DROP |
| `3e571e1555` | address-based builds skip publishing unit shadows over their heaps | `APS5_BDA_NO_SHADOW_PUBLISH` | GuestBufferMemory | both | DROP |
| `f189943d10` | lane-separated recorder batches for async compute | `APS5_COMPUTE_LANE` | QueueWorker, Recorder | both | DROP |
| `0e5d40167f` | compute-lane batches on a second Vulkan queue | `APS5_COMPUTE_QUEUE` | VulkanDevice, Context, Recorder, DeferredDestruction | both | DROP |
| `3792f9c5af` | profile storage traffic by resource | optional switch | `Graphics/src/Texture.cpp` | both | DROP (diagnostic) |
| `fb5785aaae` | opt-in block shadows for thin linear storage images | opt-in switch | Texture, tests | both | DROP |
| `f4c78be33e` | test: sized deallocation beyond cache limits | — | `tests/HostAllocator.cpp` | both | DROP |
| `72c090368c` | wait only for completion-stored labels before resource reads | `APS5_SYNC_COMPLETION_LABELS_ONLY` | ShaderResources | both | DROP |
| `8e64644062` | configurable floor for the sampled texture cache budget | `APS5_SAMPLED_BUDGET_FLOOR_MIB` | ShaderResources | both | DROP |
| `4ab160e3b3` | recompiler: skip `call_once` once a memo entry is ready | — | `core/shader/recompiler/Recompiler.cpp` | gain is MinGW-specific | DROP |
| `0a05659009` | shed the device pool tier while video memory is over budget | `APS5_POOL_SHED_OVER_BUDGET` | BufferPool | both | DROP |
| `ce5b4170ed` | reuse a data-only draw stage rebased to moved user-data pointers | `APS5_REBASED_STAGE_REUSE` | Draw cache (DrawLookup, DrawRelocate, DrawCapture, Driver.hpp), tests | both | DROP |

Also drop the `docs/dev/ENVIRONMENT.md` rows these commits added.

If AnyPS5's own driver is ever wanted as a second GPU engine, these are upstream-style
optimisations that belong in upstream pull requests, not in this product.

### 2.2 Renderer hosting: 7 commits, split into features

#### F1. Engine selection and locating the renderer and host module — MUST

- **Commit:** `803c689a71` (Windows), extended for Linux by our `8be0cd4d0e`.
- **Purpose:** decide at run time that the external renderer owns GPU memory, and obtain the
  renderer's entry points from inside libkernel.
- **Behaviour today:** `APS5_GPU_ENGINE` is read once; only the exact value `kyty` enables the
  engine. On first use libkernel opens the renderer (`KYTY_GPU_LIBRARY`, default the bare file name
  `libkyty_gpu.so` resolved by the loader; opened immediately-bound and with global symbol
  visibility) and resolves each forwarded function by its NID string as a symbol of that module,
  caching the result per function. A renderer that cannot be opened, or that lacks a NID, raises an
  error naming the library and the NID. Global visibility matters: the audio code later finds the
  renderer's audio entry through the process-wide symbol scope (C14, C15).
- **Host module side:** a new library target `core/libs/prx/libAstroHybridHost` (own
  `CMakeLists.txt`, excluded from `all`, linked against `libScePad`; `libkernel` and `libc` come
  from the generic loop; the product build names it explicitly in `scripts/prx-targets.txt`).
  It exports one `_nid_no_patch` entry point returning a pointer to a static capability table
  (C1 to C11), plus the two HDR helpers of F5. It has no threads, no initialiser and no teardown:
  it is passive, every function runs on the caller's thread. Its only state is the GPU event
  registration list (mutex-protected), the latest motion sample per port, and one float.
- **Recognising its absence:** without the variable AnyPS5 maps guest memory itself and the
  renderer sees none of it (black window or renderer abort). Without the host module the renderer
  stops the process at the first graphics call with a message that no host module provides the
  entry symbol.
- **Configuration:** `APS5_GPU_ENGINE` (`kyty` or unset), `KYTY_GPU_LIBRARY` (path),
  `KYTY_GPU_HOST_MODULE` (path, read by the renderer, not by AnyPS5).
- **Platform:** both. **Product uses it:** yes. **Depends on:** forwarders (mechanism A), F2 to F5.
- **Re-implementation notes:** the selection variable, the module lookup and the capability table
  are ours to redesign. Requirements: (1) one authoritative switch; (2) the host must bind to the
  renderer instance the loader already loaded; (3) failure to find a required renderer entry is a
  start-up error with the entry's name, never a silent fallback to AnyPS5's own mapping; (4) the
  host module is found without depending on the working directory.

#### F2. Handing guest address ranges to the renderer ("donation") — MUST

- **Commit:** `803c689a71` (libc part and host callback).
- **Problem:** AnyPS5's arena owns the whole PS5 application address range. The renderer brings its
  own guest memory manager (direct memory, flexible memory, fixed-address mappings, CPU write
  tracking by page protection) and must place mappings at PS5 addresses without AnyPS5 ever
  allocating there. Without it the renderer cannot map the title's fixed-address direct memory and
  graphics start fails with "the host could not release ..." messages.
- **Hook points:** `Arena` in `core/libs/prx/libc/src/GuestArena.cpp` and its header (three new
  libc exports), called from the host module.
- **Required behaviour:**
  - *Floating donation* (size, power-of-two alignment): choose the **highest** free aligned range
    of that size inside the arena, searching downward from the arena end, exactly like upstream
    `GuestArenaHighestFree_nid_no_patch`; return 0 when none exists. The host module uses 1 GiB
    alignment.
  - *Fixed donation* (address, size): the range must lie inside the arena bounds.
  - In both cases mark the range used, add it to the arena's permanent holes (so that a later
    `GuestArenaRelease` overlapping it re-marks it used, as upstream does for holes), and remember
    it in a list of donated ranges. A donated range is never handed out by the arena again and is
    never returned.
  - *Ownership query* (address, size): true when the whole range lies inside one donated range.
  - On Linux the snapshot performs **no** operating-system call for a donation: it is pure
    bookkeeping, and the renderer then maps over the arena's `PROT_NONE` reservation at fixed
    addresses (inferred from the renderer side). On Windows the reservation placeholder is split so
    that the donated range becomes a placeholder of its own, and a fixed donation is refused unless
    every page is still reserved-but-uncommitted.
  - Errors: invalid size or alignment and an unavailable arena are programming errors (throw);
    "no room" and "not free" are reported to the renderer as failure, which aborts graphics start.
- **The three ranges requested today** (renderer-side constants, listed because the host must be
  able to satisfy them): `0xFE0000000` + `0x20000000` fixed; `0x200000000` up to `0x7FFFF0000`
  fixed; 64 GiB floating (`KYTY_GPU_GUEST_GIB`). Note that the first lies in the PS5 system-reserved
  range, which the Linux arena treats as used and does not map; the second starts exactly at the
  arena's default allocation start.
- **Platform:** both; Linux needs only the bookkeeping. **Product uses it:** yes.
- **Known weakness to fix in the rewrite:** on Linux a fixed donation is not checked against
  ranges AnyPS5 already allocated. It works today only because nothing lives in those ranges when
  the first renderer call happens (inferred). Either validate and fail loudly, or perform the
  donations before any guest allocation can happen (section 4, R1).

#### F3. Forwarding the game's memory calls to the renderer — MUST

- **Commit:** `803c689a71`; Linux loader by our `8be0cd4d0e`.
- **Problem:** GPU-visible memory must be mapped and tracked by the renderer's memory manager.
  With the engine on, the renderer becomes the owner of **all** of the title's direct and flexible
  memory, not only of GPU ranges.
- **Hook points:** the seventeen exports of `core/libs/prx/libkernel/DirectMemory/Export.cpp`
  listed below. Each check happens before any upstream argument validation; the renderer's result
  is returned unchanged.
- **Always forwarded when the engine is on** (twelve): allocate direct memory, allocate main direct
  memory, direct memory query, direct memory size, map direct memory, map direct memory 2, map
  named direct memory, map flexible memory, map named flexible memory, release direct memory,
  available flexible memory size, configured flexible memory size.
- **Forwarded by address** (five): `sceKernelMprotect`, `sceKernelMunmap`, `sceKernelVirtualQuery`,
  `sceKernelSetVirtualRangeName`, `sceKernelClearVirtualRangeName` go to the renderer only when the
  engine is on **and** the whole argument range lies inside one donated range (a zero length counts
  as one byte; the virtual query tests one byte at the address). Everything else stays with
  AnyPS5's implementation, which keeps serving AnyPS5-owned mappings (thread stacks, guest heap,
  module images).
- **Not forwarded** (stay AnyPS5's even with the engine on): `sceKernelAvailableDirectMemorySize`,
  `sceKernelGetDirectMemoryType`, `sceKernelReleaseFlexibleMemory`, `sceKernelReserveVirtualRange`,
  `sceKernelMmap`, `sceKernelBatchMap`/`2`, `sceKernelMtypeprotect`,
  `sceKernelQueryMemoryProtection`, `sceKernelCheckedReleaseDirectMemory`, the internal and system
  flexible variants, memory pools. Astro Bot evidently does not depend on them for
  renderer-owned memory; a general implementation should decide each explicitly (section 4, R2).
- **Recognising its absence:** the renderer logs faults or aborts on the first command buffer
  whose addresses it has never mapped; nothing is drawn.
- **Platform:** both. On Linux the PS5 calling convention equals the native one.
- **Product uses it:** yes. **Depends on:** F1, F2.

#### F4. GPU events on the game's event queues — MUST

- **Commit:** `803c689a71` (host module).
- **Problem:** the game waits for end-of-pipe interrupts and flips on event queues that belong to
  AnyPS5's libkernel; the renderer raises those events but cannot touch AnyPS5 queue handles.
  Without it the game blocks forever in its first wait for a frame event (frozen after the first
  submit, renderer idle).
- **Hook points:** the four `Equeue*_nid_postfix` functions; behaviour modelled on upstream
  `sceAgcDriverAddEqEvent` and `registerVideoOutEvent`.
- **Required behaviour:**
  - *Register* (queue, kind, ident, user data). Reject a zero or unknown queue and an unknown kind
    with the kernel "bad descriptor"-class error `0x80020009` (the snapshot returns an error where
    upstream's own functions throw; decide per the conventions). Two kinds exist:
    - **EOP interrupt:** filter id −14, event ident = the AGC event id, flags "add" and "clear",
      the caller's user data. Triggering sets the event's data to 1 if it was idle and otherwise
      increments it; resetting clears the triggered state and the data. This is exactly upstream's
      driver event.
    - **Flip:** filter id −13, event ident 0 regardless of the video-out handle, flag "add" only,
      the caller's user data. Triggering produces an event whose filter-flags counter is one higher
      (saturating at 15) and whose data is the 64-bit payload delivered by the renderer, **passed
      through unmodified** (the renderer has already encoded it the way its own
      `sceVideoOutGetEvent*` decode it; upstream AnyPS5 would add a counter and a time stamp here,
      which must not happen). If the event is still triggered, the new one is queued behind it
      instead of overwriting it. Resetting clears triggered state, counter and data.
  - Remember each successful registration as (queue, kind, ident), without duplicates.
  - *Delete* (queue, kind, ident): forget the registration, remove the event from the queue,
    return the queue's error when it was not registered.
  - *Deliver* (kind, ident, payload): for every remembered registration of that kind, and for EOP
    additionally with the same ident, trigger the event on its queue with the payload. A flip is
    delivered to every flip registration whatever handle it was registered with. Take a snapshot of
    the list under the lock and trigger outside it.
- **Threads:** register and delete on game threads; deliver on the renderer's pump thread.
- **Platform:** both. **Product uses it:** yes. **Depends on:** our equeue fix O8 (several waiters).

#### F5. HDR helpers for SystemService — MUST (as a link requirement)

- **Commit:** `803c689a71` (host module); forwarder entries in the renderer tree.
- **Problem:** `libSceSystemService.prx` imports two helpers from `libSceAgcDriver.prx`. Once the
  driver is replaced by a forwarder, those two symbols must still resolve or the process does not
  load.
- **Behaviour:** the status query fills a default `AgcDriver::HostHdrStatus` (HDR not available,
  no HDR10 surface, generations 0) and sets its reported tone-map peak to the last value noted;
  the note call stores that value (initially 0). The effect is that the title is told the display
  is SDR.
- **Platform:** both. **Product uses it:** yes (indirectly).
- **Re-implementation notes:** the minimum is "these two NIDs resolve and report SDR". If the
  renderer ever presents HDR, the real status has to come from it (new capability).

#### F6. Trace of forwarded map calls — DROP

- **Commit:** `803c689a71`. `APS5_GPU_ENGINE_TRACE` (presence) prints one line per forwarded
  allocate or map call (requested address, length, flags, status, result). It also prints for
  AnyPS5's own flexible map when the engine is off. Diagnostic, not set by the product.
  The upstream `APS5_TRACE_CALLS` facility already counts system-library calls.

#### F7. Pad input from the renderer's window into libScePad — MUST

- **Commit:** `fe8ecec02a`.
- **Problem:** in upstream the window owner (`libSceVideoOut`) reads keyboard and controllers and
  publishes them to `libScePad`. With the renderer owning the window and `libSceVideoOut`
  replaced, nothing publishes input: the game sees a pad that never changes.
- **Hook point:** `PadPublishInputForPort_nid_postfix`.
- **Behaviour:** translate one pad sample (C8) into a `PadInputState`: buttons copied bit for bit,
  the four stick bytes, L2 and R2 analog values, both touch points (active, x, y, id); everything
  else left at its default. If a motion sample is latched for that port (F11), attach it and set
  the has-motion flag. Then publish for the given port. `libScePad` itself drops unchanged samples
  and time-stamps changed ones.
- **Not consumed today:** the sample's "connected" flag (upstream has
  `PadSetPortConnected_nid_postfix`; the rewrite should use it), device kind, the two "touch
  button" booleans.
- **Rate / thread:** about every 4 ms on the renderer's relay thread; port 0 only.
- **Platform:** both. **Product uses it:** yes.
- **Also lost with upstream's VideoOut:** keyboard, mouse and IME feeding (`libSceKeyboard`,
  `libSceMouse`, `libSceIme`) and anything else AnyPS5 drew or read through its own window
  (section 4, R7).

#### F8. Window close ends the process — MUST

- **Commit:** `2f05504cd1`.
- **Problem:** the renderer owns the window. Without this capability closing the window leaves a
  windowless process running (the renderer explicitly waits for a shutdown that never comes).
- **Behaviour:** on the renderer's request call `LibcRequestExit_nid_postfix(code)`, exactly what
  upstream's own window does. The call returns immediately; the exit runs on libc's detached exit
  thread.
- **Platform:** both. **Product uses it:** yes.

#### F9. Pad output (rumble, light bar, adaptive triggers) to the renderer — SHOULD

- **Commit:** `a8413f36b2` (host callback and the libScePad change it needs).
- **Problem:** the renderer owns the physical controller, so what the game asks of the pad through
  `libScePad` must be handed to it. Without it there is no rumble, light bar colour or adaptive
  trigger effect.
- **Hook points:** `PadFetchOutputForPort_nid_postfix`; `Pad::SetTriggerCommand` and
  `PadTriggerRequest` in libScePad.
- **Behaviour:**
  - libScePad keeps, per port and trigger, the **raw 56-byte trigger command** exactly as the game
    passed it to `scePadSetTriggerEffect` (a 4-byte mode, then parameters from offset 8), plus a
    flag that a command has been set. Upstream keeps only its own DualSense encoding of the effect,
    which the renderer cannot use because it encodes effects itself.
  - The host answers a fetch (C10) by asking libScePad for output newer than the caller's sequence
    cursor and, if there is any, returning: sequence, both game rumble strengths, both
    audio-derived rumble strengths, light bar validity and colour, and for each trigger the flag
    and the raw command.
  - The renderer applies the larger of game and audio rumble per motor and only re-sends what
    changed.
- **Platform:** both. **Product uses it:** yes (DualSense is the declared controller).
- **Depends on:** F7's relay thread on the renderer side.

#### F10. Computed trigger effect state — SHOULD

- **Commit:** `a8413f36b2`.
- **Problem:** upstream `scePadGetTriggerEffectState` always reports "off" for both triggers. A
  title that waits for a trigger effect to engage (for example "weapon fired") never sees it.
- **Hook point:** `scePadGetTriggerEffectState` in `core/libs/prx/libScePad/Export.cpp`; needs the
  raw trigger command stored by F9 and the current input state.
- **Configuration:** `APS5_PAD_TRIGGER_STATE`, on when the value starts with `1`, read once;
  otherwise upstream behaviour. The product sets it to 1. Special pad handles keep returning zeros.
- **Behaviour when on:** for each trigger on which the game has set an effect, derive the state
  from the effect's mode, its parameter bytes and the trigger's current travel.
  - Travel is the analog value 0..255; if the analog value is 0 but the digital L2/R2 button bit is
    set, travel is 255 (digital-only input devices).
  - "Pressed" means travel of at least 8. "Position" is travel scaled to ten zones (0..9).
  - States follow the PS5 numbering: 0 off, 1 feedback standby, 2 feedback active, 3 weapon
    standby, 4 weapon pulling, 5 weapon fired, 6 vibration standby, 7 vibration active.

  | Effect mode | State reported |
  |---|---|
  | 0 off, or no effect set | 0 |
  | 1 feedback (start zone, strength) | active when pressed, strength non-zero and position ≥ start zone; otherwise standby |
  | 2 weapon (start zone, end zone, strength) | fired when position ≥ end zone; pulling when position ≥ start zone; otherwise standby |
  | 3 vibration (start zone, amplitude, frequency) | active when pressed, amplitude and frequency non-zero and position ≥ start zone; otherwise standby |
  | 4 multi-position feedback (ten strengths) | active when pressed and the strength of the current zone is non-zero; otherwise standby (feedback numbering) |
  | 5 slope feedback (start zone, ...) | active when pressed and position ≥ start zone; otherwise standby (feedback numbering) |
  | 6 multi-position vibration (frequency, ten amplitudes) | active when pressed, frequency non-zero and the amplitude of the current zone non-zero; otherwise standby (vibration numbering) |

- **Caveat:** the thresholds (8, ten equal zones) and the state rules are the snapshot author's
  model, not measured on a console. Upstream's rule is that unverified behaviour is recorded in
  `docs/dev/debt/libScePad.md`; that is also the reason it sits behind a switch.
- **Platform:** both. **Product uses it:** yes.

#### F11. Pad motion sensors from the renderer — SHOULD

- **Commit:** `fcb81313ab`.
- **Problem:** the controller's accelerometer and gyroscope are read by the renderer; the game
  reads orientation through `libScePad`. Without it, motion-controlled sections do not respond.
- **Behaviour:** a motion sample (C9) for a port (0..3) is stored and flagged valid; it is not
  published by itself. The next input publication for that port (F7) carries it in
  `PadInputState` (`accel`, `gyro`, has-motion). `libScePad` derives orientation from angular
  velocity as upstream does. Units: m/s² including gravity, rad/s. The store is a plain latch
  written and read by the same relay thread.
- **Configuration:** on the renderer side only (`ASTROHYBRID_PAD_MOTION=1`, set by the product).
- **Platform:** both. **Product uses it:** yes. **Depends on:** F7.
- **Rating note:** SHOULD by the scale used here; treat as MUST if any mandatory part of the game
  needs motion (not verified from code).

#### F12. CUDA address window in the guest arena — DROP on Linux

- **Commit:** `87d9eb9668`.
- **Problem it solves (Windows):** NVIDIA's DLSS through NGX on Vulkan needs
  `VK_NVX_binary_import`, whose CUDA backing reserves a large address window (about 48 GiB) below
  1 TiB when the Vulkan device is created. AnyPS5's arena reserves that whole region first, so
  device creation with the extension fails and DLSS is unavailable.
- **Behaviour:** with `ASTROHYBRID_CUDA_HOLE` present, the Windows arena constructor leaves
  `0xDC00000000 .. 0xEC00000000` (64 GiB, directly below the default floating donation)
  unreserved and records it as a permanent hole so that the arena never allocates there.
- **Platform:** the code is inside the Windows branch of the arena constructor. **It has no
  effect on Linux**, and the product does not set the variable.
- **Recommendation:** DROP. But see section 4, R4: the Linux arena reserves the same region, the
  product ships DLSS, and why DLSS works on Linux without a hole has not been established here.

#### F13. Controller speaker and haptic audio through the renderer (wireless DualSense) — SHOULD

- **Commit:** `9123102d58` (Windows lookup); Linux lookup by our `8be0cd4d0e`.
- **Problem:** a DualSense on USB exposes a four-channel sound card that AnyPS5 can open for
  speaker and haptic channels. On Bluetooth there is no sound card; the renderer's controller
  backend can still deliver speaker audio and haptics over its own controller connection. Without
  this, Bluetooth pads get only coarse rumble derived from the haptic signal and the pad speaker
  is folded into the main mix.
- **Hook points:** `AudioOut.cpp` (`sceAudioOutOutput`, `sceAudioOutOutputs`, next to
  `outputVirtualPort` / `paceVirtualPort`), `AudioOut2Context.cpp` (`Render`),
  `AudioOut2Port.cpp` (`AudioOut2MixPorts`), `AudioOut2PadMix.{hpp,cpp}` (new helper for C14).
- **Behaviour:**
  - *Helper (C14):* find the renderer's "queue pad audio" entry by name in the loaded renderer
    (cache it); if absent return 0; otherwise pass the audio and return the queued microseconds.
  - *Classic AudioOut ports:* for a port of type pad speaker or vibration with one or two channels
    and a non-null buffer, convert the grain to float (16-bit samples divided by 32768), scale each
    channel by the port's volume relative to the default volume, and queue it to the renderer at
    the port's sample rate (speaker or haptics according to the port type). If the renderer accepts
    it, pace the port as a virtual port and skip AnyPS5's own output for this grain; if not, fall
    through to the upstream path unchanged. The renderer path is tried **before** the port's own
    device, so it wins whenever the renderer accepts.
  - *AudioOut2 mixing rule:* a pad-speaker port is mixed into the pad mix whenever a pad mix buffer
    exists, even when no controller sound card is open (upstream folded it into the main mix in
    that case). Ports are mixed into the main mix only when they are main ports or no pad mix
    exists.
  - *AudioOut2 render, no controller sound card open:* build stereo speaker and stereo haptic
    grains from the pad mix (clamped to ±1) and queue both to the renderer at the AudioOut2 sample
    rate. If the speaker grain is not accepted, add the pad-speaker channels to the main mix (the
    upstream fallback). If the haptic grain is not accepted, fall back to upstream's derived rumble
    (`AudioOut2RumbleFromPadFrames` then `PadSubmitAudioHaptics_nid_postfix`).
  - *AudioOut2 render, controller sound card open:* upstream path (plus our O2).
- **Platform:** both. **Product uses it:** yes for Bluetooth pads (inferred: the product offers
  DualSense and does not restrict the connection type).
- **Known weakness:** in the "no sound card" branch the renderer is called while the AudioOut2
  context lock is held; our O4 fixed a deadlock of exactly that shape in the USB branch
  (section 4, R8).

#### F14. Save-data transfer mount falls back to a regular mount — SHOULD

- **Commit:** `9123102d58` (the export itself already exists upstream).
- **Problem:** some Astro Bot builds mount their first normal save through
  `sceSaveDataTransferringMount`. Upstream answers "not found" (no PS4-to-PS5 transfer data), and
  such a build then cannot create its save.
- **Hook point:** `transferringMount` in `core/libs/prx/libSceSaveData.native/Export.cpp`.
- **Behaviour:** always clear the result structure and trace user, title id and directory name.
  With `APS5_SAVEDATA_TRANSFER_FALLBACK` set to anything other than `0`, and a request that carries
  a directory name and a result pointer, perform a regular mount instead: same user and directory,
  mode "create if missing, read-write", through the existing regular mount path, and return its
  result. Otherwise return "not found" as upstream.
- **Configuration:** `APS5_SAVEDATA_TRANSFER_FALLBACK` (default off; the product sets 1). Read on
  every call.
- **Platform:** both. **Product uses it:** yes.
- **Caveat:** this is title-driven behaviour behind a switch, which upstream's conventions
  discourage; record it in `docs/dev/debt/libSceSaveData.native.md`. Whether the supported versions
  (1.018, 1.019) take this path can be confirmed with `APS5_TRACE_CALLS`.

### 2.3 Summary of recommendations

| Rec. | Entries |
|---|---|
| MUST (7) | F1 engine selection and host module, F2 donation, F3 memory forwarding, F4 GPU events, F5 HDR helpers, F7 pad input, F8 window close |
| SHOULD (5) | F9 pad output, F10 trigger state, F11 motion, F13 wireless pad audio, F14 save-data fallback |
| DROP (22) | the 20 driver / recompiler commits of 2.1, F6 map trace, F12 CUDA window (Linux) |

---

## 3. Our own changes on top

### 3.1 Inside the snapshot range

| # | Commit | What it does | Depends on faycal's code? | Re-home |
|---|---|---|---|---|
| O1a | `8be0cd4d0e` (Claude), libkernel `DirectMemory/Export.cpp` | Linux branch of the renderer lookup: open `KYTY_GPU_LIBRARY` (default `libkyty_gpu.so`) once, immediately bound and globally visible, resolve NIDs as symbols; errors name the library and NID. | Yes: it fills the Linux half of faycal's lookup helper (14 lines inside his 127). | Becomes part of the new F1. |
| O1b | same, `AudioOut2PadMix.cpp` | Linux lookup of the renderer's pad-audio entry through the process-wide symbol scope. | Yes: Linux half of faycal's helper of F13. | Becomes part of the new C14 helper. |
| O1c | same, `libkernel/Apr/src/Apr.cpp` | **APR staging — MUST.** With the engine on (Linux), `_readFile` reads the file through a staging buffer of at most 64 KiB and copies into the guest destination, like upstream's Windows branch, instead of letting the kernel write into guest memory. Reason: the renderer write-protects GPU-visible guest pages to track CPU writes; a kernel `read` into such a page fails with `EFAULT` instead of faulting, so the read comes back short and APR throws "read failed" / "returned N of M bytes", which aborts the process while the game streams assets. An ordinary copy faults, the renderer's handler records the write and lifts the protection. | Only upstream code is edited (24 lines); it reuses the engine variable of F1. | Re-apply on upstream. Better: make it general (section 4, R5). |
| O1d | same, `libSceAgcDriver/Execution/src/Driver/Lifecycle.cpp` | "Driver area": under the engine on Linux, AnyPS5's driver constructor skips `MapDriverMemory` because the renderer owns the PS5 driver area at `0xFE0000000`. | Upstream code only. | **DROP: dead code.** AnyPS5's driver is not loaded (section 1.3). The live part of the "driver area" story is the first fixed donation of F2. |
| O2 | `e6b134da2a` (Ali), `AudioOut2Context.cpp` | Linux, USB DualSense (controller sound card open): the two haptic channels of each pad grain are sent to the renderer's DualSense backend and zeroed in the grain that goes to the sound card; speaker channels stay on the sound card. Reason: the renderer's rumble reports switch the pad to rumble emulation, which silences audio haptics; its backend mutes those reports while haptic audio plays. `APS5_PAD_VIBRATION_VIA_HOST=0` restores sound-card routing. Never on Windows. | Calls faycal's helper of F13; inserted in `Render` right after his block (4 lines there). | Re-home on the new C14 helper. |
| O3 | `7cade313aa` (Ali) | `APS5_PAD_VIBRATION_GAIN` (default 1.0, clamped 0..4, non-finite → 1.0): multiplies the haptic channels sent by O2, clipped to ±1. Product sets it per launch (haptics 100..400 % → 1.0..4.0). | Builds on O2 only. | With O2. |
| O4 | `4e4b939318` (Ali) | Haptic grains of O2 are handed to a dedicated worker thread through a bounded queue (at most 32 grains, oldest dropped) instead of being queued under the AudioOut2 context lock; fixes a deadlock (the renderer's backend opens an SDL stream on the controller's sound card, whose device thread waits for the same lock). Also adds "is the renderer's pad-audio entry present" (O2 is active only if it is). The worker is created on first use and never stops. | Uses faycal's helper; adds an availability probe next to it in `AudioOut2PadMix.cpp`. | With O2. Apply the same hand-off to F13's branch (R8). |
| O5 | `9503760288` (Ali) | `APS5_PAD_VIBRATION_DUMP=<path>`: writes the mixed haptic channel pair of every grain as raw 32-bit floats. Diagnostic used by `tools/rec-haptics.sh`. | No (upstream `Render`, one call line). | Independent; optional. |
| O6 | `eb188dd62a` (Ali), `libSceAudioPropagation` | A source's input buffer (attribute id 4) is rendered into channel 0 of the output bed, which is cleared first; the input is consumed once. Without it every sound sent through propagation is silent (Astro Bot's contact haptics). | **No.** Upstream files only. | Re-apply unchanged. |
| O7 | `2429864775` (Ali), `libSceAudioPropagation` + test | Path calculation without rays and setting audio paths succeed instead of failing or throwing (crash on entering Slo-Mo Casino). | **No.** | Re-apply unchanged. |
| O8 | `5122ed27b7` (Ali), `libkernel/Equeue/Equeue.cpp` + test | Wake every thread waiting on an event queue when an event is added, triggered or edited (backport of upstream `daad982457`). | **No.** | Re-apply unchanged, or pick a newer upstream base that contains it. |

### 3.2 After the import (`engines/anyps5`, `4eb79627..HEAD`)

| # | Commit | What it does | Depends on faycal's code? | Re-home |
|---|---|---|---|---|
| O9 | `9c821b5c` (part 1) | `AGS_CONTROLLER_AUDIO_SINK`: when set and non-empty, both controller-sound-card searches (`openVibrationDevice` in `AudioOut.cpp`, `OpenPadDevice` in `AudioOut2Context.cpp`) accept only the audio device with exactly that name; unset or empty keeps upstream's `AudioOut2IsPadAudioDevice` match. New pure helper with unit tests. | **No.** Replaces two upstream call sites. | Re-apply unchanged. |
| O10 | `9c821b5c` (part 2) | Live haptic gain: the gain of O3 is taken from the renderer's "pad vibration gain" entry (C15) on every grain when that symbol exists, clamped 0..4; a non-finite value or a missing symbol keeps the launch value of `APS5_PAD_VIBRATION_GAIN`. Lets the settings page change haptic strength while the game runs. | Sits on O3/O4; placed next to faycal's helper file. | With O2 to O4. |
| — | `888eea21` | `UPSTREAM.md` text only. | — | — |

Independent of faycal's code and re-appliable unchanged on the pristine base: **O5, O6, O7, O8,
O9**. Re-appliable on upstream code but tied to the engine switch: **O1c**. Must be re-homed on the
new bridge: **O1a, O1b, O2, O3, O4, O10**. Dead: **O1d**.

---

## 4. Open questions and risks

- **R1. Donation over live allocations (Linux).** A fixed donation is bookkeeping only and is not
  checked against ranges the arena already handed out. The second fixed range starts at the arena's
  default allocation start, so any guest allocation made before the first renderer call would be
  mapped over by the renderer. It works today presumably because the guest libc's first memory call
  starts the renderer before anything else is allocated there (inferred, not traced). The rewrite
  should either donate at a defined point before guest code runs, or verify and fail with a clear
  message. Deciding the start point also removes the "whichever call comes first opens a window and
  a Vulkan device on a game thread" behaviour.
- **R2. Unforwarded memory functions.** Several libkernel memory functions stay with AnyPS5 even
  for renderer-owned memory (list in F3), and the virtual query forwards only when the queried
  address itself is donated, so a "find next" query starting below a renderer mapping never sees
  it. Unknown whether any supported game version depends on this; `APS5_TRACE_CALLS` on a play
  session would tell.
- **R3. Two ways to load the renderer.** The forwarders load it by soname from `libs/`, libkernel
  loads it by `KYTY_GPU_LIBRARY`. They are the same file in the product; if they ever differ the
  process holds two renderers with separate state. One lookup of the already loaded module removes
  the variable and the risk.
- **R4. DLSS address window on Linux.** F12 does nothing on Linux, the Linux arena reserves up to
  `0xFC00000000` (leaving 16 GiB below 1 TiB), and the product ships DLSS without setting
  `ASTROHYBRID_CUDA_HOLE`. Either the Linux driver has different address requirements, or DLSS
  works because of ordering, or it silently falls back. Needs one measurement (the renderer has a
  probe switch for exactly this) before F12 is finally dropped; if a hole is needed, the Linux
  arena constructor needs the equivalent of F12.
- **R5. Kernel writes into renderer-tracked pages.** O1c fixes APR only. Every other path where
  the kernel writes into a guest buffer (`read`, `pread`, socket receives, decoders writing through
  syscalls) has the same `EFAULT` failure if the destination is a write-protected GPU-visible page.
  Upstream's convention is the `GuestArena::HostWrite` guard around such writes; a general fix is a
  new capability "open / close this range for a host write" that the guard calls for donated
  ranges. Not known to be hit by Astro Bot outside APR.
- **R6. Fault-handler order.** The renderer installs its handler in front of whatever exists at
  graphics start. AnyPS5's Linux write watch installs its own `SIGSEGV`/`SIGBUS` handler lazily
  (`core/libs/prx/libc/src/GuestWriteWatch.cpp`). If that happens after the renderer's
  installation, correctness depends on AnyPS5 chaining to the previous handler for addresses it
  does not own. Verify, or make the order explicit.
- **R7. Services lost with upstream's VideoOut.** Keyboard, mouse and IME feeding, AnyPS5's HDR
  probe, its cursor and any host-drawn dialog relied on AnyPS5's window. Only pad input (F7) and
  window close (F8) were replaced. Astro Bot does not need the rest (inferred from the product
  working); a second title might.
- **R8. Lock held across the renderer call (F13).** The Bluetooth branch queues audio to the
  renderer while holding the AudioOut2 context lock, the pattern that deadlocked in the USB branch
  before O4. Route it through the same worker hand-off.
- **R9. No renderer shutdown.** The host never asks the renderer to stop; the process exit ends it
  mid-flight. If the renderer persists anything at shutdown (pipeline or shader caches), that is
  lost on every exit (inferred). A "stop" capability driven from libc's shutdown callbacks
  (`LibcRegisterShutdown_nid_postfix`) would close this.
- **R10. Rust and AnyPS5's internal interfaces.** The host-side helpers the bridge needs are C++
  typed: `PadPublishInputForPort_nid_postfix` takes a C++ reference to a struct with `std::array`
  members, `EqueuePin_nid_postfix` returns a `std::shared_ptr`, `KernelEqueueEvent` holds a
  `std::deque` and function pointers, and several throw. They are also exported under NIDs, not
  under their names. A Rust host module therefore needs a thin engine-internal C++ layer with a
  plain C surface, or these pieces stay C++.
- **R11. Title-specific forwarder list.** The forwarders export only the NIDs Astro Bot imports.
  A game update that imports one more graphics function fails at load with an unresolved symbol.
  The list should be generated from the relinker's import registry (`AstroBot.registry.json`,
  `AstroBot.needed.json` in the run folder) or checked against it at build time.
- **R12. Trigger-state model unverified.** F10's thresholds and state rules are not measured on
  hardware.
- **R13. Static TLS tunable.** `GLIBC_TUNABLES=glibc.rtld.optional_static_tls=0x200000` is required
  by the renderer's TLS model; it is a launch requirement of the current renderer build, not of
  AnyPS5, and can go away if the renderer is built with a different TLS model.
- **R14. Upstream conventions versus product needs.** F10 and F14 are switch-guarded behaviours
  justified by one title, and F4 returns an error where upstream's equivalents throw. If the
  engine tree is ever offered upstream these need debt entries or a different shape; as a private
  fork they only need `docs/dev/ENVIRONMENT.md` rows and debt notes.
