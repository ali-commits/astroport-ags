# Clean-room functional specification: the Kyty side of the hybrid renderer

Status: written from a read of the imported tree and its history. It describes behaviour and
requirements. It contains no code from the third-party ("faycal") commits; identifiers named
without a marker exist in pristine upstream KytyPS5 at `dd9f9901`. Identifiers that the third
party introduced are marked *(3p)* and are named only so the reader can recognise what is being
replaced; do not reuse them as a design. Statements marked **[inferred]** were deduced, not read.

Terms: **host** = the AnyPS5-relinked game process and its reimplemented system libraries;
**renderer library** = Kyty's GPU stack built as `libkyty_gpu.so`; **guest thread** = a game
thread (created and owned by the host); **bridge** = the glue between the two.

Paths are relative to the Kyty source root (`engines/kyty` in our repository).

---

## 1. How embedding works today

### 1.1 What is built, and from what

There is no CMake target for the renderer library. An out-of-tree script
(`tools/hybrid/build_kyty_gpu_linux.py`, ours; the Windows twin `build_kyty_gpu.py` is 3p) reuses
a finished `kyty_emulator` build directory:

| Step | Behaviour |
|---|---|
| Precondition | `kyty_emulator` configured with position-independent code and exported compile commands. |
| Compile | Only the bridge translation unit is compiled, with the exact flags of one emulator source (`src/libs/agc.cpp`), minus output/dependency/PIE flags. |
| Link inputs | Every object of the `kyty_emulator` link **except `main.cpp.o`**, plus the bridge object, plus the emulator's full library list. PIE, map-file and dependency-file linker options are removed. |
| Link mode | Shared object, soname `libkyty_gpu.so`, symbolic binding (`-Bsymbolic`), and a version script. |
| Visible symbols | Only: the C API entry point, the pad-audio entry point, the vibration-gain entry point, one symbol per graphics NID, and the per-NID thunk symbols (the forwarder shims link against the thunk symbols). Everything else is local. |

Why the version script matters: the host loads the library with global symbol visibility next to
its own SDL2, while Kyty links SDL3 and many other libraries statically. Any leaked symbol can
bind across the two. This is a hard requirement for the rewrite.

What is excluded: only `main.cpp`. The loader, network, file system, audio, save-data and every
other HLE library are linked in and simply never initialised. The rewrite should prefer a real
CMake shared-library target over scraping `build.ninja`, but may keep linking the whole emulator
object set if isolating the GPU stack proves expensive (the GPU stack depends on
`Libs::LibKernel::Memory`, `Libs::LibKernel::EventQueue`, `Libs::LibKernel` pthread lifecycle,
`Config`, `Log`, `Profiler`, `Libs::Controller`, `Loader::SymbolDatabase`).

Optional build inputs: `KYTY_NGX_SDK_DIR` *(3p, CMake cache variable, not an environment
variable)* adds the NGX SDK include path, defines a compile switch and links the NGX static
library (see 2.21 and 3.11). `KYTY_STREAMLINE_DIR` *(3p)* is Windows-only (2.23).

### 1.2 How the library gets into the process

Three independent paths load or find it on Linux, all present today:

1. **Forwarder shims.** `tools/hybrid/build_linux_shims.py` (ours) emits three small shared
   objects named exactly like the PS5 libraries the game imports graphics from
   (`libSceAgc.prx`, `libSceAgcDriver.prx`, `libSceVideoOut.prx`). Each exports the NIDs of its
   library; every export is a single jump to the renderer library's thunk for that NID. They are
   linked against `libkyty_gpu.so` (run path `$ORIGIN`, immediate binding), so the dynamic loader
   pulls the renderer library in at process start. The `libSceAgcDriver` shim additionally
   forwards two host-internal HDR entry points to the host's own bridge module; those two never
   reach Kyty.
2. **Host kernel library.** When the host's engine switch selects Kyty, its `libkernel` opens the
   library named by `KYTY_GPU_LIBRARY` (host-side variable, default file name on the loader
   path) with global visibility and looks NID names up in it to forward direct-memory calls.
3. **Host audio.** The host's audio output looks the pad-audio and vibration-gain entry points up
   in the global symbol scope.

`tools/hybrid/gen_kyty_gpu_exports.py` *(3p)* generates the export table. Inputs: the relink
registry JSON files of the game executable and its bundled modules (every imported NID with the
library it is imported from; a `#` suffix on a NID is stripped). Rules it applies:

* Only four libraries are considered: `libSceAgc`, `libSceAgcDriver`, `libSceVideoOut`,
  `libkernel`.
* Kyty's implementation name for a NID is found by scanning the `LIB_FUNC("<nid>", <function>)`
  registrations in `src/libs/libAgcDriver.cpp`, `src/libs/libVideoOut.cpp`, `src/libs/libKernel.cpp`.
* From `libkernel`, only an allowlist of memory functions is taken (list in 1.5); all other
  kernel NIDs stay with the host.
* A graphics NID the game imports that Kyty does not implement is a generation error.
* Two extra NIDs are added because the host's own modules (job manager, system service) import
  them from the graphics libraries although the game does not.
* Three NIDs are marked "implemented by the bridge" (event registration, 1.6).
* Five driver NIDs are marked "argument layout verified against the host's declarations"
  (informational only).
* Output: an ordered table (index, NID, library, function name, binding kind) and, for Windows, a
  module-definition file. Current table: **101 exports** = 66 `libSceAgc`, 7 `libSceAgcDriver`,
  11 `libSceVideoOut`, 17 `libkernel`.

The rewrite needs an equivalent generator (any language) and should keep "fail generation when
the game imports something unimplemented" as a property.

### 1.3 Export mechanics and lazy start

Requirement: game code calls the exports with the System V x86-64 convention and arbitrary
arguments (integer, floating point, variadic). Kyty's HLE functions are already declared
`KYTY_SYSV_ABI`, so once bound a call must reach the HLE function with **no register disturbed
except a scratch register**.

Today:

* Each export is a tiny thunk that records its table index in a caller-clobbered scratch register
  and jumps through a per-export slot.
* Before binding, every slot points at a resolver trampoline. The trampoline preserves all six
  integer argument registers, the variadic count register and the eight vector argument
  registers, calls a resolver with the index, restores everything and tail-jumps to the address
  the resolver returned. The stack must be 16-byte aligned at the resolver call.
* The resolver, under one lock, performs a **one-time lazy start** if nothing has started yet:
  1. locate the host bridge module: the path in `KYTY_GPU_HOST_MODULE` *(3p)* or a default file
     name; first try "already loaded only", then a normal load;
  2. call a well-known function in it that returns the host's callback table;
  3. run *init* in "stage only" mode (targets are computed but slots are not yet published, so
     other guest threads keep funnelling into the lock);
  4. run *start graphics* (1.4);
  5. publish all slots.
* Any failure (no host module, version mismatch, a NID without implementation, renderer start
  failure) prints the NID and function name to stderr and aborts the process. A call can never
  run on half-initialised state.

An eager path also exists (host calls the API entry, then init, then start graphics). It is
exercised by the Windows acceptance tests; whether the shipped host ever uses it was not
verified. The rewrite may choose lazy, eager, or both; lazy is what works today, and it has the
side effect that the window appears only when the game first touches graphics.

*Init* binding: construct a `Loader::SymbolDatabase`, run the upstream registration functions
`Libs::InitAgcDriver_1`, `Libs::LibGen5::InitVideoOut_1` and `Libs::InitLibKernel_1_Mem` on it
(these are the `LIB_DEFINE` bodies in `libAgcDriver.cpp`, `libVideoOut.cpp`, `libKernel.cpp`),
then resolve each table NID with `SymbolDatabase::FindByNid(nid, Loader::SymbolType::Func)` and
take the record's `vaddr`. Note `InitAgcDriver_1` registers both the `libSceAgc` and the
`libSceAgcDriver` functions.

### 1.4 Renderer start and threads

*Start graphics* does, in order:

1. Ask the host for guest address ranges and register them with Kyty's memory manager (1.5).
2. Create the **renderer thread** and wait (bounded: 120 s) for it to report that all subsystems
   are up.
3. Create the event proxy queue and the **event pump thread** (1.6), if the host can receive events.
4. Create the **input relay thread** (1.7), if the host can receive pad state.
5. Install the guest-memory fault handler (1.5).

The renderer thread is Kyty's "main thread". It mirrors `src/emulator.cpp`'s initialisation
order but skips what the host owns. It calls `Common::InitializeThreads()`, builds a
`Config::ConfigOptions` from the environment (1.9), then initialises through
`Common::Subsystems`, in this order: `Config::Lifecycle` (then `Config::Load(options)`),
`Log::Lifecycle`, `Libs::LibKernel::PthreadLifecycle`, `Profiler::Lifecycle`,
`Libs::LibKernel::Memory::Lifecycle`, `Libs::Controller::Lifecycle`,
`Libs::Graphics::Lifecycle`. Upstream's `Libs::Network`, `Libs::LibKernel::FileSystem`,
`Libs::Audio` and `Libs::Wheel` lifecycles are **not** started. It then signals readiness and
runs `Libs::Graphics::WindowRun()`, which owns the SDL window and event loop until quit. The
subsystems are destroyed on the same thread in reverse order when it leaves.

Threads that exist while running:

| Thread | Owner | Role |
|---|---|---|
| Guest threads | host | Call Kyty HLE functions directly through the exports (command buffer building, DCB/ACB submission, video-out, direct memory). Take guest-memory write faults. |
| Renderer / window thread | bridge | Subsystem lifetime, SDL events, keyboard shortcuts, window title. |
| Kyty GPU threads | Kyty (upstream) | Command processor(s), draw-prep workers, Vulkan submission worker, video-out present thread (`VideoOutDriver::Impl::PresentThread`), shader compile workers. Created by `Libs::Graphics::Lifecycle`. |
| Event pump | bridge | Blocks on a Kyty event queue, calls the host for each event. |
| Input relay | bridge | 4 ms loop: publish pad state, fetch and apply pad output. |
| Host audio thread | host | Calls the pad-audio entry point. |
| Session socket worker | ours (`src/streaming/`) | Created by the presenter when a session socket is configured (3.8). |

### 1.5 Guest memory

**Ownership.** The host owns the process's guest address space (one large inaccessible
reservation, its "guest arena"). Kyty's memory manager normally discovers and reserves free
address space itself; here it must not. Instead the bridge asks the host to *donate* ranges, and
Kyty manages only those.

Donations requested at start, each through one host capability "give me N bytes, at this address
if I name one, else wherever you like; tell me the base":

| Range | Why |
|---|---|
| Fixed, base `0x0fe0000000`, size `0x20000000` | Kyty's AGC driver maps its own 2 MiB direct-memory block at the PS5 address `DriverDmemBase` (`src/libs/agc.cpp`). Kyty keeps no free gap there otherwise. |
| Fixed, base `0x200000000` up to `0x7ffff0000` | The PS5 system-managed area. Titles map fixed addresses there (Astro Bot maps 2 GiB of direct memory at `0x300000000`), and it is Kyty's default range for its own mappings. |
| Floating, default 64 GiB (`KYTY_GPU_GUEST_GIB` *(3p)*, not set by our product) | General room for direct and flexible memory. The host chooses and aligns the base. |

A fixed request that the host cannot satisfy at exactly that base is a start failure.

**Kyty-side hook (3p, engine-internal, must be re-implemented in C++).** A new entry point in
`Libs::LibKernel::Memory` (next to `Initialize`) records donated ranges; it must be called before
`Memory::Initialize` and is an error afterwards. The address-space reservation routine in
`src/kernel/memoryAddressSpace.inc` then behaves differently when any donation is recorded:

* Linux: each donated range is re-mapped in place as an inaccessible, private, anonymous,
  no-reserve mapping at its fixed address (the host keeps its own reservation record; Kyty takes
  the pages over), added to the free list, and recorded as owned. **Nothing else is scanned or
  reserved** (the normal `/proc/self/maps` walk is skipped entirely).
* Windows: each range is checked to be a reserved placeholder starting exactly at its base,
  added to the free list, and the normal scan is skipped.
* Teardown must leave donated ranges to the host (the Windows release loop skips them; the Linux
  teardown behaviour for donated ranges was not verified, see section 4).

**Search range (ours, 3.6).** Upstream int22 limits ordinary mappings to addresses below the AMM
window (`HOST_USER_ALLOC_END` = `kAmmWindowBase`). The host's floating donation lies at or above
that base, so with donations present the upper bound of ordinary searches becomes the end of the
user range (`HOST_USER_MAX + 1`). Affected upstream code: `FindGuestFreeRange` in
`src/kernel/memory.cpp` and the address-space class's aligned free-range search in
`memoryAddressSpace.inc`. Without this, allocations fail once the low area is full.

**Which memory calls Kyty answers.** Exactly these upstream functions in `Libs::LibKernel::Memory`
are exported under their NIDs and the host's `libkernel` forwards to them: `KernelAllocateDirectMemory`,
`KernelAllocateMainDirectMemory`, `KernelReleaseDirectMemory`, `KernelGetDirectMemorySize`,
`KernelDirectMemoryQuery`, `KernelMapDirectMemory`, `KernelMapDirectMemory2`,
`KernelMapNamedDirectMemory`, `KernelMapFlexibleMemory`, `KernelMapNamedFlexibleMemory`,
`KernelAvailableFlexibleMemorySize`, `KernelConfiguredFlexibleMemorySize`, `KernelMunmap`,
`KernelMprotect`, `KernelVirtualQuery`, `KernelSetVirtualRangeName`, `KernelClearVirtualRangeName`.
For address-based calls the host decides per call whether the address lies inside a donated
range and forwards only then **[inferred from host code]**. The rationale: Kyty must own every
GPU-visible mapping because its renderer tracks those pages.

Three memory notification functions exist in the 3p API (register / unregister / invalidate a
range). They are unimplemented stubs and unused. Two host callbacks (query a mapping, app0 path)
are declared and never provided or called. None of these are requirements.

**Write tracking and faults.** Kyty tracks CPU writes to GPU-visible guest memory by page
protection (`Libs::Graphics::PageManager`, `src/graphics/host_gpu/pageManager.cpp`). In the
stand-alone emulator the handler is installed by `src/loader/runtimeLinker.cpp`
(`Common::HostException::InstallFirstAccessHandler` with its own callback). Nothing loads through
Kyty's loader here, so the bridge installs a handler that:

* accepts only access violations; maps read/write/execute to `Libs::Graphics::PageFaultAccess`;
* if `Libs::Graphics::FaultCost::MapEnabled()`, records the faulting instruction
  (`FaultCost::SetFaultInstruction`);
* calls `Libs::LibKernel::Memory::HandleGpuFault(access, address)` and reports "handled" with
  its result;
* everything it does not handle must reach the host's own handlers (the host has a guest write
  watch and a crash reporter).

`Common::HostException::InstallFirstAccessHandler` upstream is implemented for Windows only
(first vectored handler) and returns failure elsewhere. **The Linux implementation is ours**
(3.3): a first-chance `SIGSEGV` + `SIGBUS` action (`SA_SIGINFO | SA_RESTART | SA_ONSTACK`) that
builds the upstream `ExceptionInfo` from the signal context (instruction pointer, stack pointer,
fault address, access kind via the file's existing access decoder), asks the registered callback,
and otherwise chains to the previously installed action (siginfo-style or classic), or restores
the default action when there was none. Only one callback can be registered.

Consequence the host must respect: a kernel system call that writes into a protected guest page
fails with `EFAULT` instead of faulting, so host file reads into guest memory go through a
staging buffer (host side, already done).

### 1.6 GPU events back to the host

The game's event queues are host kernel objects; Kyty raises end-of-pipe interrupts and flip
events on Kyty event queues (`Libs::LibKernel::EventQueue`). The three registration NIDs are
therefore implemented by the bridge instead of bound to Kyty:

* **Add EOP event** (`sceAgcDriverAddEqEvent`: queue handle, event id, user data) and
  **add flip event** (`sceVideoOutAddFlipEvent`: queue handle, video-out handle, user data):
  1. ask the host to add the event to the game's queue (the host stores user data and owns
     trigger semantics); a host error is returned to the game unchanged;
  2. the first time a given event id (or video-out handle) is seen, register the same id on one
     Kyty **proxy queue** using the upstream functions
     `Libs::Graphics::Gen5Driver::AgcDriverAddEqEvent(queue, id, udata)` /
     `Libs::VideoOut::VideoOutAddFlipEvent(queue, handle, udata)`.
* **Delete EOP event**: forwarded to the host only; the proxy registration stays.
* Missing host capability: return the SCE error the real function would for an invalid argument
  (`0x8A6C0002` for AGC) or invalid queue (`0x80290012` for video-out).

The proxy queue is created with `EventQueue::KernelCreateEqueue` and carries one user event
(`KernelAddUserEvent`) used only to wake the pump for shutdown (`KernelTriggerUserEvent`). The
pump thread loops on `EventQueue::KernelWaitEqueue` (up to 16 events, no timeout) and, per event,
tells the host: for filter `KERNEL_EVFILT_GRAPHICS` "EOP interrupt, ident, data"; for
`KERNEL_EVFILT_VIDEO_OUT` "flip, ident (video-out handle), data (the flip argument)".

Not covered today: vblank events (a kind is reserved, never raised), deleting flip events, and
any video-out event NID the game does not import. Video-out status calls
(`VideoOutGetVblankStatus`, `VideoOutIsFlipPending`, `VideoOutGetOutputStatus`, ...) are answered
by Kyty directly. Flips themselves travel inside command buffers (`AgcDcbSetFlip`, then
`AgcDriverSubmitDcb`), so Kyty sees them without bridge involvement.

### 1.7 Input, pad output, motion, pad audio

Kyty owns the window and SDL, therefore keyboard and controllers; the game reads pads through
the host's pad library. All of this is the relay thread unless stated.

**Input (Kyty to host), every 4 ms.** `Libs::Controller::PadInit()`, then
`PadOpen(Config::GetUserId(), 0, 0, nullptr)` (falling back to `PadGetHandle`), then per cycle
`PadReadState(handle, &data)` (`Libs::Controller::PadData`, `src/libs/padData.h`). Published for
host port 0 only: the 32-bit SCE button mask, four stick bytes (128 = centre), L2 and R2 analog
bytes, connected flag, two touch points (x, y, id, active = index below the touch count).

**Motion (Kyty to host), optional**, published immediately before the pad state of the same
cycle: acceleration in m/s² including gravity (Kyty's `acceleration_*` are in g; multiply by
9.80665) and angular velocity in rad/s (`angular_velocity_*`). See 2.18 and 2.26.

**Pad output (host to Kyty), same 4 ms cycle.** The host keeps "what the game last asked of the
pad" with a change sequence number; the relay asks "anything newer than the sequence I saw?" and
applies only differences through Kyty's own pad functions (which apply the user's intensity
settings and translate DualSense trigger effects):

* rumble: per motor, the larger of the game-requested and the audio-derived strength;
  `Libs::Controller::PadSetVibration` when either motor changed or on the first update;
* light bar: `PadSetLightBar` with RGB, or `PadResetLightBar` when the game cleared it;
* adaptive triggers: `PadSetTriggerEffect` with a parameter block shaped like the PS5 call
  (a trigger mask byte, padding to 8 bytes, then two 56-byte commands, left then right); only
  triggers whose command is new or changed are flagged in the mask.

**Pad audio (host to Kyty), on the host's audio thread.** One entry point taking sample rate,
"speaker or haptics", a sample pointer, frame count, channel count, sample format (16-bit or
float) and a gain; it returns the queued duration in microseconds, 0 when nothing was queued.
Behaviour: pick the controller (the active one from `Libs::Controller::GetActiveControllerId()`
if SDL still knows it, and for speaker audio only if it is a DualSense; otherwise make sure the
SDL gamepad subsystem is up with HIDAPI and enhanced PS5 reports enabled, enumerate, open, and
prefer the first DualSense, else the first pad); keep one stream for speaker and one for haptics
(`Libs::Controller::DualSenseHaptics::Open(freq, speaker)`, reopened when the rate changes); call
`DualSenseHaptics::Queue` with unity per-channel volume and the caller's gain multiplied by
`Libs::Controller::GetSettingScale` for `Setting::SpeakerVolume` or
`Setting::VibrationIntensity`. Streams are closed at shutdown. The data pointer is only valid
during the call.

**Vibration gain (Kyty to host, ours).** A second entry point returns the live haptic strength
factor (3.9); the host's audio mix polls it and clamps it to 0..4.

### 1.8 Window close and process exit

* The player closes the window: `WindowRun()` returns although no shutdown was requested. The
  renderer thread asks the host to exit the process "its usual way" (exit code 0) and then
  **keeps every subsystem alive**, polling every 100 ms, because the game may keep submitting
  GPU work while it shuts down. Recognise the missing behaviour by a crash or hang on closing
  the window.
* Host-initiated shutdown: set the shutdown flag, close pad audio streams, post an SDL quit event
  (`SDL_EVENT_QUIT`) if the renderer runs, stop the relay, wake and stop the pump, join the
  renderer thread (which destroys the subsystems). Which host path calls this at process exit,
  if any, was not verified (section 4).

### 1.9 Start-up configuration read by the bridge

Read once on the renderer thread into `Config::ConfigOptions` before `Config::Load`:

| Variable | Origin | Meaning | Default | Set by our product |
|---|---|---|---|---|
| `KYTY_GPU_WIDTH`, `KYTY_GPU_HEIGHT` | 3p | Window / screen size in pixels, accepted range 1..7680 | Kyty's option defaults | yes |
| `KYTY_GPU_FULLSCREEN` | 3p | `1`: desktop fullscreen and hidden cursor at start (the upstream toggle keys keep working) | off | yes |
| `KYTY_GPU_PRESENT_MODE` | 3p | `Fifo`, `Mailbox`, otherwise `Immediate` (`Config::PresentMode`) | Immediate | no |
| `KYTY_GPU_SHADER_OPTIMIZATION` | 3p | `None`, `Size`, otherwise `Performance` (`Config::ShaderOptimizationType`) | Performance | no |
| `KYTY_GPU_VBLANK_HZ` | 3p | Guest vblank frequency | 60 | no |
| `KYTY_GPU_GUEST_GIB` | 3p | Size of the floating donation | 64 | no |
| `KYTY_GPU_HOST_MODULE` | 3p | Path of the host bridge module for lazy start | default file name | yes |
| `ASTROHYBRID_RUMBLE_SCALE` | ours on top of 3p | Also seeds `controller_vibration_intensity` (0..100) | unset | yes |

Upstream versus third-party status of the other variables our product sets:

| Variable | Status |
|---|---|
| `KYTY_HANG_WATCHDOG`, `KYTY_CP_PRIORITY`, `KYTY_DRAW_PREP_SPIN_US`, `KYTY_DRAW_PREP_COLD_SPIN_US`, `KYTY_BUFFER_RANGE_FIXES`, `KYTY_AGC_DONE_MODE` | Upstream Kyty. Keep setting them; nothing to implement. |
| `KYTY_DRAW_PREP_PRIORITY` | 3p (2.9). No effect on Linux. |
| `KYTY_WAYLAND_NATIVE` | Ours (3.3). |
| `KYTY_GPU_LIBRARY` | Host-side variable, not read by Kyty. |
| `KYTY_LIB`, `KYTY_SO` | Shell variables of our launch script only. |
| `KYTY_NGX_SDK_DIR` | 3p CMake cache variable. |
| All `ASTROHYBRID_*` | 3p or ours; see sections 2 and 3. |

### 1.10 What must cross the boundary (capability list for the new ABI)

Only plain C data crosses. "Lifetime" says how long passed memory must stay valid.

| # | Capability | Direction | Calling thread | Data | Lifetime / notes |
|---|---|---|---|---|---|
| A1 | Version / size negotiation | both | first caller | interface version; today "same major, struct at least as large as the oldest supported, fields only appended" | once |
| A2 | Find the host's services | Kyty to host | first guest thread to call an export | a way to obtain the host's service table | process lifetime |
| A3 | Graphics calls | host (game) to Kyty | guest threads | the 101 NID exports, PS5 ABI | pointers are guest memory |
| A4 | Donate guest range | Kyty to host | starting thread | size, optional required base; returns base | until process exit |
| A5 | Add GPU event | Kyty to host | guest thread inside the NID call | host queue handle, kind (EOP / flip), ident, user-data pointer; returns 0 or negative SCE error | host stores user data |
| A6 | Delete GPU event | Kyty to host | guest thread | queue handle, kind, ident | |
| A7 | Deliver GPU event | Kyty to host | pump thread | kind, ident, 64-bit data | by value |
| A8 | Publish pad state | Kyty to host | relay thread, about 250 Hz | port, buttons, sticks, triggers, connected, two touches | copied during the call |
| A9 | Publish pad motion | Kyty to host | relay thread, before A8 | port, 3 floats acceleration, 3 floats angular velocity | copied |
| A10 | Fetch pad output | host to Kyty (polled by Kyty) | relay thread | in/out sequence; rumble (game and audio-derived, two motors each), light bar valid + RGB, per trigger "set" flag and 56-byte command | copied into Kyty's buffer |
| A11 | Request process exit | Kyty to host | renderer thread | exit code | must not tear the renderer down synchronously |
| A12 | Shutdown | host to Kyty | any host thread, once | none | blocks until the renderer thread ended |
| A13 | Queue pad audio | host to Kyty | host audio thread | rate, target (speaker / haptics), samples, frames, channels, format, gain; returns microseconds queued | samples valid during the call only |
| A14 | Haptic gain | host to Kyty (polled by host) | host audio thread | returns a float | |
| A15 | Log | Kyty to host | any | level (info / warning / error), text | text valid during the call |
| A16 | HDR status, tone-map peak note | game to host | guest threads | forwarded by a shim, never reaches Kyty | listed for completeness |

Not needed (present today, unused): memory register / unregister / invalidate, memory query,
app0 path, direct DCB/ACB submission through the C API, NID table introspection.

---

## 2. Feature catalogue (third-party commits)

Legend: **MUST** = the game does not run or the product breaks without it; **SHOULD** = product
feature or measurable gain; **DROP** = diagnostic, experiment, Windows-only or unused.
"Used" = whether our product sets or depends on it (launch settings and script).

Summary: 7 MUST, 12 SHOULD, 16 DROP (35 commits).

| # | Commit | Subject | Platform | Used | Verdict |
|---|---|---|---|---|---|
| 2.1 | `00157dbd` | Larger texture binding memo by default | both | no | DROP |
| 2.2 | `c7716fb7` | Skip region host lock for settled spans | both | no | DROP |
| 2.3 | `234c5298` | Export table, thunks, C API, generator, build script | both | yes | MUST |
| 2.4 | `14cbbbcd` | Renderer started inside the host process | both | yes | MUST |
| 2.5 | `a6e784b8` | Lazy start, direct-memory exports, GPU event proxy | both | yes | MUST |
| 2.6 | `8221d49a` | Donated address space, fault handler, staged binding | both | yes | MUST |
| 2.7 | `c5443342` | Input relay | both | yes | MUST |
| 2.8 | `9275450f` | Test expectations | Windows | no | DROP |
| 2.9 | `e4f3e5f5` | Renderer options from the environment, build fixes | both | yes | MUST |
| 2.10 | `4d41d0a4` | Window close asks the host to exit | both | yes | MUST |
| 2.11 | `6ed4997b` | Draw-prep worker priority | Windows | set, no effect | DROP |
| 2.12 | `f10725be` | Lock-free watchdog event updates | both | no | DROP |
| 2.13 | `6eba4178` | Pad output: rumble, light bar, triggers | both | yes | SHOULD |
| 2.14 | `0aa28fed` | Descriptor offset audit by bind point | both | no | DROP |
| 2.15 | `df58c118` | Commit time attributed to render targets | both | no | DROP |
| 2.16 | `d42e4348` | Cache title id for persistent caches | both | yes | SHOULD |
| 2.17 | `520675ae` | Frame-rate limit setting | both | yes | SHOULD |
| 2.17 | `6fdc82ff` | More limits, fixed cadence, fullscreen start | both | yes | SHOULD |
| 2.19 | `1cbf2e8d` | Real-time game step | both | no | DROP |
| 2.17 / 2.19 | `34dda51d` | More limits (keep); game speed percent (drop) | both | partly | SHOULD |
| 2.20 | `614acdba` | Fixed window title (keep); overlay start modes (drop) | both | partly | SHOULD |
| 2.22 | `4cb7fc66` | Guest-load fault context | both | no | DROP |
| 2.18 | `cae890db` | Motion sensor relay | both | yes | SHOULD |
| 2.24 | `9dbd6770` | Generation fast path for written buffer bindings | both | set to off | DROP |
| 2.24 | `dbced347` | Read memo cross-epoch proof | both | no | DROP |
| 2.25 | `e31d441a` | NVIDIA low-latency mode | both (NVIDIA) | yes | SHOULD |
| 2.27 | `9b4e9cda` | Pass trace, shader dump | both | no | DROP |
| 2.21 | `94cbc59a` | DLSS Super Resolution | both (NVIDIA) | yes | SHOULD |
| 2.21 | `a06d0635` | Comment only: address-space requirement of NGX | Windows | no | DROP |
| 2.23 | `377ff170` | DLSS Frame Generation | Windows | no | DROP |
| 2.23 | `6312ad97` | Overlay shows generated frames | Windows | no | DROP |
| 2.17 | `6a9bdfd6` | Any limit from 10 to 240 | both | yes | SHOULD |
| 2.23 | `dc6509ce` | Adaptive frame-generation multiplier | Windows | no | DROP |
| 2.26 | `a1ccda55` | Choose the playing pad; rumble scaling | both | yes | SHOULD |
| 2.18 | `37198216` | Angular velocity derived from orientation | both | yes | SHOULD |

### 2.1 Texture binding memo size (`00157dbd`) - DROP
A switch (`ASTROHYBRID_OPT_2`) raises the default slot count of
`TextureBindingMemo::SlotCount()` (`renderer/pipeline/textureBindingMemo.cpp`) from 4096 to 32768.
Upstream already has `KYTY_TEXTURE_MEMO_SLOTS` (64..4194304) for the same purpose. If the gain
is wanted, set the upstream variable; no code needed. Claimed symptom without it: run-start draws
re-resolving their textures every frame in dense scenes.

### 2.2 Settled-span shortcut in the page manager (`c7716fb7`) - DROP
A switch (`ASTROHYBRID_OPT_1`) lets `PageManager::Impl::ApplySpan` return without taking the
region's host lock when, under the region's count lock, no page is pending and every page's
recorded host protection already equals what its counts ask (never in the defer-verify mode).
Purpose: a write fault whose pages another thread just unprotected need not queue behind
in-flight protection calls. Unused by us; a correctness-sensitive micro-optimisation. Revisit
only with a measurement on Linux.

### 2.3 Export table and binding (`234c5298`) - MUST
Everything in 1.2 and 1.3 except lazy start. Without it the game cannot link against Kyty's
graphics functions at all. Dependencies: none. Also introduced the Windows acceptance test
(a C program built with the host toolchain checking exports, version negotiation, binding and
calls through thunks); write our own tests instead.

### 2.4 Renderer inside the host (`14cbbbcd`) - MUST
Section 1.4. Symptom without it: no window, first submission crashes on a null renderer.
Hooks: `Common::Subsystems`, the lifecycles listed in 1.4, `Libs::Graphics::WindowRun`.

### 2.5 Lazy start, direct-memory exports, event proxy (`a6e784b8`) - MUST
Sections 1.3 (lazy), 1.5 (the 17 kernel exports) and 1.6 (events). Symptoms without the event
proxy: the game waits forever on its end-of-pipe or flip event queue after the first frame.
Symptoms without the memory exports: GPU-visible memory mapped by the host is unknown to Kyty's
tracker and renders as garbage or faults.

### 2.6 Donated address space and fault handling (`8221d49a`) - MUST
Section 1.5: donation requests, the Kyty-side adoption hook, staged slot publication, and the
write-fault callback. Symptoms without it: Kyty aborts at memory initialisation because the host
already holds the address space; or, without the fault callback, the host's crash reporter
treats Kyty's expected write faults as crashes. Hooks: `src/kernel/memory.cpp`, `memory.h`,
`memoryAddressSpace.inc`, `Common::HostException`, `Libs::LibKernel::Memory::HandleGpuFault`.

### 2.7 Input relay (`c5443342`) - MUST
Section 1.7 input. Without it the game receives no input (Kyty's window swallows it).

### 2.8 Test expectations (`9275450f`) - DROP
Windows test maintenance only.

### 2.9 Renderer options from the environment (`e4f3e5f5`) - MUST
Section 1.9, first seven rows: without command-line parsing the renderer needs its presentation
mode, shader optimisation level and vblank frequency from somewhere. The defaults (Immediate,
Performance, 60 Hz) are the ones in use. The rest of the commit is Windows build plumbing.

### 2.10 Window close (`4d41d0a4`) - MUST
Section 1.8.

### 2.11 Draw-prep worker priority (`6ed4997b`) - DROP
`KYTY_DRAW_PREP_PRIORITY` (0..2) raises each draw-prep worker's thread priority at worker start
(`Engine::Workers` thread body in `renderer/drawPrep/drawPrep.cpp`) using the same helper as
upstream's `KYTY_CP_PRIORITY` / `KYTY_SERVICE_PRIORITY` (`src/common/threads.cpp`). The body is
compiled for Windows only, so our product's setting of `2` does nothing on Linux. Remove the
setting, or, if a Linux gain is measured, add a Linux implementation as a separate decision.

### 2.12 Watchdog fast events (`f10725be`) - DROP
`ASTROHYBRID_WATCHDOG_FAST_EVENTS=1` lets `HangWatchdog::NoteEvent` (`src/common/hangWatchdog.cpp`)
update a previously seen event record through a small per-thread cache without the global event
mutex. Only matters when the hang watchdog is on; our product sets `KYTY_HANG_WATCHDOG=0`.

### 2.13 Pad output (`6eba4178`) - SHOULD
Section 1.7 pad output. Without it: no rumble, no light bar colour, no adaptive triggers.
Depends on 2.7. Strongly recommended for this title; it is "SHOULD" only because the game runs
without it.

### 2.14 Descriptor offset audit (`0aa28fed`) - DROP
Extra counters in an upstream diagnostic (`DrawEncode::OffsetAuditEnabled`); splits reuse
statistics by graphics versus compute bind point.

### 2.15 Commit time by render-target signature (`df58c118`) - DROP
`KYTY_CP_PASS_STATS=1` (with upstream `KYTY_CP_GAP_STATS=1`) attributes draw-commit time to the
attachment signature (size, colour count, formats) of the committed draw, in up to 32 buckets
plus overflow. Diagnostic.

### 2.16 Cache title id (`d42e4348`) - SHOULD
Problem: upstream names its persistent pipeline / program cache files after the title id from
loader metadata (`PipelineCacheTitleId()` in `renderer/pipeline/pipelineCache.cpp`). In the
hybrid no title was loaded through Kyty, the id is empty, and persistent caches are silently
disabled: every launch recompiles shaders and pipelines (long stutter after each start).
Behaviour: when the loader-provided id is empty, the cache file stem is taken from
`ASTROHYBRID_CACHE_TITLE_ID`, accepted only if it is exactly nine characters, `PPSA` followed by
five digits; otherwise caches stay off. Only the file naming is affected. Our product sets
`PPSA21567`. A cleaner design is to pass the title id through start-up configuration.

### 2.17 Frame-rate limiter (`520675ae`, `6fdc82ff`, `34dda51d` in part, `6a9bdfd6`) - SHOULD
Purpose: cap presented frames per second in Kyty's presenter. Our product sets
`ASTROHYBRID_FPS_LIMIT` (30 or 60) and changes it live (3.9).

Hook point: `VideoOutDriver::Impl::PresentThread` in `src/graphics/presentation/videoOut.cpp`,
at the two places where it calls `FlipQueue::Flip()` (the vblank present and the
"present ready flips between vblanks" loop used with `Config::PresentMode::Immediate` or
`KYTY_PRESENT_READY_FLIPS`).

Configuration: `ASTROHYBRID_FPS_LIMIT` = one of the presets 30, 35, 40, 45, 50, 55, 60, 90, 120,
the word `unlimited`, or any whole number 10..240. Unset or invalid: the limiter is absent and
upstream behaviour is untouched, including the shortcut below.

Algorithm (per present thread, monotonic high-resolution ticks, `f` = ticks per second):

* State: the next due time (0 = none). Reset to 0 whenever the selected limit changes.
* Interval = ceil(f / limit).
* A flip may be presented only when the current time has reached the due time **and** the flip
  queue actually presents something. Otherwise nothing changes.
* After presenting: if cadence pacing is on, a due time existed, and the present was at most
  1 ms late, the next due time is the previous due time plus the interval (small timer
  overshoots do not accumulate; measured motivation: a 30 FPS cap delivering 29.76 FPS with
  uneven gaps). Otherwise (first frame, or late by more than 1 ms) the next due time is "now"
  plus the interval, so a late frame is never followed by a catch-up burst.
* Limit `unlimited`: present whenever a flip is ready; no due time.
* While a limit (including `unlimited`) is selected and the present mode is Immediate, the thread
  uses its absolute-vblank timeline (the same mode as upstream `KYTY_PRESENT_ABS_VBLANK`), so
  guest vblank events keep a steady clock independent of presents.
* In the between-vblank loop, when a numeric limit is active and the due time is in the future,
  the thread sleeps until the earlier of the vblank deadline and the due time instead of
  polling.
* `ASTROHYBRID_FPS_PACING=0` turns the cadence rule off (always restart from "now"); default on.
  Not set by us; keep the rule, drop the switch.

User interface: Ctrl+F2 (in `WindowContext::ProcessEvent`, `window/window.cpp`, next to the
upstream F2 handlers) advances to the next preset, wrapping, and shows the performance overlay if
hidden; it only works when the variable was set. The overlay shows the current limit and the
window title gets a suffix with it. Optional for the rewrite.

Interaction: with the low-latency interval (2.25) the driver paces the *start* of a frame while
this limiter paces its *presentation*; our product sets both to the same rate.

### 2.18 Motion relay (`cae890db`, `37198216`) - SHOULD
`ASTROHYBRID_PAD_MOTION=1` (default off; our product sets 1) and a host able to receive motion.
Data and units as in 1.7. Without it, shake and tilt gameplay does not work.

Fallback for pads without a gyroscope (Kyty emulates tilt for them and then reports an
orientation and accelerometer but zero angular velocity): when all three angular velocity
components are exactly zero and a previous sample exists, derive the body-frame rate from the
change of the orientation quaternion (`orientation_*` in `PadData`) between this and the previous
read: the vector part of (conjugate of previous) times (current), times two, divided by the
elapsed time, with the sign flipped when the scalar part is negative. Apply only when the elapsed
time is between 0.5 ms and 100 ms. The host integrates angular velocity into orientation itself,
which is why a rate, not an orientation, is sent.

### 2.19 Real-time game step (`1cbf2e8d`, `34dda51d` in part) - DROP
`ASTROHYBRID_GAME_SPEED=1` writes the measured frame rate (smoothed, clamped 20..120, snapped to
the limit when within 6 %) into a fixed address in the game image that holds the game's frame
rate, its reciprocal and microseconds per frame, so the game does not run in slow motion below
60 FPS; `..._PERCENT` scales it, `..._ADDR` overrides the address. Not used: our product solves
this with executable patches selected by the adapter ("precise frame cap" patches, 3.9). The
approach is fragile (hard-coded address, validated only by plausibility).

### 2.20 Window title and overlay start modes (`614acdba`) - title SHOULD, overlay DROP
`ASTROHYBRID_WINDOW_TITLE`: when set and non-empty, replaces the diagnostic window title built in
`WindowContext::UpdateTitle` (`window/window.cpp`). Our product sets "Astro Bot". Without it the
title bar shows emulator diagnostics (device, frame count, FPS).
`ASTROHYBRID_OVERLAY=fps|compact|full` shows the performance overlay from the start (an added
"FPS only" presentation in `performanceOverlay.cpp`). Unused.

### 2.21 DLSS Super Resolution (`94cbc59a`, `a06d0635`) - SHOULD
Our product exposes it as a setting (`ASTROHYBRID_DLSS=1`) and ships the runtime (3.11). Requires
an NVIDIA GPU and a build with the NGX SDK.

Idea: replace the game's own temporal upscale pass by a DLSS evaluation that writes into the
same output target, so all later passes are unaffected.

Recognition of the pass: a draw whose pixel shader hash equals a known constant
(`0x2936e5eca99d75e7` for this title; `ASTROHYBRID_DLSS_PASS` overrides). Title facts the 3p
author established and the implementation relies on:

* The pass is a full-screen draw. Its first bound image is the G-buffer target whose first two
  channels are motion vectors; its second is the anti-aliased colour; a history image follows.
  Its first colour target is the upscaled output.
* Its first constant buffer (at least 56 bytes) holds, as 32-bit words: current jitter in UV
  (words 0, 1), output width and height (4, 5), input width and height (6, 7), a frame counter
  (11), and a "motion vectors in use" flag (13).
* Motion vectors are in UV units, point from the previous to the current position and include
  jitter. Depth is not bound to the pass.

Depth discovery: for every draw, remember which depth attachment was used together with each
colour target (a small ring of the 16 most recent colour-to-depth pairs). The depth for DLSS is
the one last used while rendering into the motion-vector target.

Hook point: `RenderExecutor::ExecutePreparedDraw` (`renderer/renderDraw.cpp`), after the late
checks and before the pipeline-variant selection. If the replacement succeeds the draw records
nothing else, and, exactly like a skipped draw, any draw-run continuation state must be
invalidated so no run is continued across it.

Evaluation requirements:

* Extensions: ask NGX for its required Vulkan instance and device extensions before instance and
  device creation (`VulkanGetExtensions` and the device-extension assembly in
  `window/vulkanWindow.cpp`) and enable those that are available. The EXT form of buffer device
  address must not be enabled because Kyty already enables the core feature.
* Initialise NGX lazily on the first eligible draw, with the instance, physical device, device and
  the two loader entry points from `VULKAN_HPP_DEFAULT_DISPATCHER`, a writable data directory and
  a search path for the DLSS runtime library. Use our own application identity.
* Create (or recreate on size change) one feature for the pass's own input and output size, with
  flags: HDR colour, low-resolution motion vectors, jittered motion vectors, auto exposure, and
  optionally inverted depth. Quality family from `ASTROHYBRID_DLSS_QUALITY`
  (`quality` default, `balanced`, `performance`, `dlaa`). Since sizes are dictated by the game,
  this selects a preset family, not a resolution.
* Record natively into the current command buffer: end the current rendering scope, take the
  native handle, transition motion, colour and depth to "sampled, read only" and the output to
  "general, storage read/write", create temporary views, evaluate, then destroy the views when
  the command buffer has completed (`CommandScheduler::DeferOperation`). After any foreign
  recording the renderer's shadow of bound state must be forgotten (a small added method on the
  command-buffer wrapper in `renderer/render.h` that settles pending encodes and restarts the
  state shadow).
* Parameters: jitter in input pixels = UV jitter times input size (sign switch
  `ASTROHYBRID_DLSS_JITTER_SIGN`, default +1); motion-vector scale = (minus input width, minus
  input height); render sub-rectangle = input size; history reset when the game's frame counter
  is not the previous one plus one.
* Afterwards mark the output image as written by the GPU (`Image::NoteContentWrite`,
  `TextureCache::CommitGpuWrite`).
* Any precondition failure leaves the game's own pass to run: not built in, initialisation
  failed, feature unavailable, unexpected bindings, size or sample-count mismatch or output not
  usable as storage, no depth known, motion vectors unused this frame, creation failed,
  evaluation failed. A ten-second console summary counts evaluations and fallbacks by reason.

Other switches (`..._DEPTH_INVERTED`, `..._PATH`, `..._SKIP_EXT`, `..._PROBE`) are tuning or
diagnostics; only `..._PATH` (extra runtime search folder) is worth keeping.

Windows-only fact from `a06d0635`: NGX on Vulkan needs a device extension whose CUDA backing
reserves a large address window below 1 TiB at device creation; the host must leave it free. On
Linux this has not been observed to be a problem **[inferred from the feature working after 3.11]**.

Alternative worth evaluating before re-implementing: upstream int22 ships its own DLSS path
(`renderer/dlss.h`, namespace `Libs::Graphics::Dlss`, driven by PSSR requests, loading the
driver's NGX library itself, with `AppendExtensions`, `Init`, `CreateFeature`, `Evaluate`). The
3p file was renamed `hybridDlss.*` at the int22 merge to coexist with it; both sit in the same
namespace. If the game's upscale can be routed through upstream's mechanism, most of this entry
becomes configuration.

### 2.22 Guest-load fault context (`4cb7fc66`) - DROP
`ASTROHYBRID_GUEST_LOAD_FAULTS=1`: for read faults whose instruction lies inside the game
executable image, publish the native context (`GuestLoad::FaultScope`, after registering the
upstream decode/apply pair with `GuestLoad::Register`) so upstream's `KYTY_READ_FAULT_PUBLISHED`
can complete the load in the handler. Off by default, unused.

### 2.23 DLSS Frame Generation (`377ff170`, `6312ad97`, `dc6509ce`) - DROP
Streamline-based frame generation: Streamline's interposer becomes the Vulkan loader, the window
surface is created through it from Win32 handles, low-latency markers are routed through
Streamline's Reflex, the camera is captured from a 576-byte per-view constant buffer, depth and
motion vectors are tagged at the upscale pass, and an adaptive multiplier targets an on-screen
rate. Windows-only by construction and unused. Its hooks in `vulkanWindow.cpp`,
`renderDraw.cpp`, `lowLatency.cpp` and `performanceOverlay.cpp` should not be reproduced. Note
for 2.25: the low-latency module has a "frame generation active" branch on every entry point;
omit it.

### 2.24 Buffer binding fast paths (`9dbd6770`, `dbced347`) - DROP
`ASTROHYBRID_COMMIT_EPOCH_FASTPATH` (off / on / verify / exit; our product sets `0`): in
`BufferCache::ObtainBuffer` (written, non-texel) and `BufferCache::ObtainWrittenBuffer`, a
4096-slot memo accepts a repeated writable binding without re-running its synchronisation when
the tracker's `MemoryTracker::RangeSignature` for the range, the buffer-structure epoch and a new
"a range left the GPU-modified set" counter are all unchanged since the record was taken in a
proven-clean state. A verify mode runs both paths and compares. The second commit's read-side
counterpart is dead in the imported tree (its flag and counters are declared, nothing sets or
reads them after the int22 merge). High risk, disabled by us: do not re-implement.

### 2.25 Low-latency mode (`e31d441a`) - SHOULD
`ASTROHYBRID_LOW_LATENCY=1` (our product default on; `measure` = markers and reports only, drop
that mode). NVIDIA Reflex through `VK_NV_low_latency2`. Enabled only if the device offers both
that extension and `VK_KHR_present_id`; otherwise logged as unavailable and ignored. The present
id device feature is enabled whenever the present id extension is.

Lifecycle hooks (all upstream locations):

* Device extension assembly and after device creation (`WindowContext::CreateVulkan`,
  `VulkanCreateDevice` in `window/vulkanWindow.cpp`): add the two extensions; after creation
  resolve the four extension entry points and create one timeline semaphore.
* `Swapchain::Create` (`window/swapchain.cpp`): chain the latency create-info with the mode
  enabled; after creation set the sleep mode on that swapchain: low latency on, boost on, minimum
  interval from 3.4. `Swapchain::Destroy`: forget the swapchain first.
* All calls naming the swapchain are serialised by one mutex, which is also held across
  `vkQueuePresentKHR`.

Frame numbering: frames are counted by guest flips, in three independent counters that advance
in the same order (guest side, command-processor side, presenter side).

| Where | Thread | Action |
|---|---|---|
| `AgcDcbSetFlip` (`src/libs/agc.cpp`) | guest | Count "a flip packet was written". |
| After the renderer accepted a DCB submission (the internal submit helper used by `AgcDriverSubmitDcb`) | guest | If flip packets were written since the last accounted submission, advance the guest frame counter by that many and end guest frame N: mark *simulation end N*; call the driver's latency sleep with the timeline semaphore at the next value and wait for it, **bounded to 100 ms**; mark *simulation start N+1*. This wait is where the driver delays the start of the next frame. |
| `VideoOutSubmitFlip` (`videoOut.cpp`) | guest | CPU flips: number the frame before the reservation, end the frame the same way after the flip preparation was submitted. |
| `FlipQueue::Reserve` (`videoOut.cpp`) | command processor (GPU flips) or guest (CPU flips) | GPU flips: advance the command-processor counter, mark *render submit end N* and *render submit start N+1*. Store the frame number in the flip request. |
| `FlipQueue::Flip`, just before `Presenter::Present` | present | For the main bus request being presented, hand its frame number to the presenter. |
| `Swapchain::Present`, around `vkQueuePresentKHR` | present | If a frame number newer than the last presented one is pending, attach it as the present id, mark *present start* before and *present end* after. A repeat present without a new frame carries no id. |

Diagnostics (optional): every 10 s read the driver's per-frame timing reports and print averages
plus sleep counts and time-outs; filled timestamps are the evidence the driver acts on the
markers.

Risks: the guest thread blocks up to 100 ms per frame inside a graphics call; markers assume one
flip per submission.

### 2.26 Controller selection and rumble strength (`a1ccda55`) - SHOULD
Hooks: `src/libs/controller.cpp` (pad-to-player routing near `RefreshPlayers`, `Connect`,
`GameController::ApplyVibration`) and the controller-button handler in `window/window.cpp`.

* `ASTROHYBRID_PAD_PREFER` (unset: upstream behaviour, first connected pad is player 1; our
  product sets `auto`). `auto`: the pad most recently connected or pressed becomes player 1 (it
  swaps player slots with the current player 1). `ps` / `xbox` / `switch` / `generic`: a pad of
  that family takes player 1 whenever one is connected; other pads only while none of that family
  is connected. Never when the keyboard holds player 1, never for the host-input pseudo
  controller. Families from the SDL gamepad type. When a non-PlayStation pad takes over it gives a
  short confirmation buzz (moderate strength on both motors, 150 ms). Each connection is logged
  (name, family, type, vendor:product, rumble and gyro capability). Requires a new notification
  "a pad button went down" from the window's controller event handler. Why it matters: only
  player 1 is relayed to the game (2.7), so with two pads connected the wrong one would play.
* `ASTROHYBRID_RUMBLE_SCALE` (percent 0..200, default 100; ours sets 100): scales motor strength
  for pads driven through SDL rumble (not the DualSense path).
* `ASTROHYBRID_RUMBLE_SWITCH_FLOOR` (0..255, default 0; ours sets 96): on Nintendo Switch type
  pads a motor that is on runs at least at this strength, because many such pads ignore weak
  rumble. A motor value of 0 always stays 0. Result is clamped to 8 bits and expanded to SDL's
  16-bit range.

### 2.27 Pass trace and shader dump (`9b4e9cda`) - DROP
Diagnostics used to find the upscale pass, depth and motion vectors for 2.21:
`ASTROHYBRID_PASS_TRACE=<seconds>` prints shader hashes, bound images and render targets for
three flips; `..._MATRIX` scans bound buffers for projection matrices; `..._CB` dumps a shader's
constants; `ASTROHYBRID_SHADER_DUMP` writes guest code and disassembly for listed shader hashes.
Useful as a development tool only; rebuild on demand.

---

## 3. Our own changes on top (kept; must be re-homed)

For each: what it does and which third-party code it touches. "Re-home" = it must be re-attached
to the new implementation because its current anchor disappears.

### 3.1 `feb4bf5f` (Ali) - DualSense speaker / haptics bridge, rumble scale, save transfer mount
* Pad-audio entry point and controller resolution (1.7, A13) live **inside the 3p bridge file**
  and its export list; shutdown closes the streams from the 3p shutdown routine. Re-home in the
  new bridge.
* `ASTROHYBRID_RUMBLE_SCALE` seeding `controller_vibration_intensity`: inside the 3p renderer
  thread option block. Re-home in the new start-up configuration.
* Upstream files edited (independent of 3p code, keep as patches on upstream):
  `src/libs/dualSenseBluetooth.cpp` (treat "not wired" as wireless; recognise Bluetooth HID paths
  by service UUID or enumerator name; fall back to a single unambiguous Bluetooth device when the
  serial does not match), `src/libs/dualSenseHaptics.cpp` (speaker routing bits, mono mix for
  speaker, `ASTROHYBRID_DS_WIRELESS_SPEAKER` / `..._HAPTICS` opt-outs, wireless detection limited
  to DualSense), `src/libs/libSaveData.cpp` (`SaveDataTransferringMount` creates a missing save
  directory with default metadata and mounts it as new; irrelevant while Kyty's save library is
  not used in the hybrid).
* Windows build script: recompiles the edited sources. Not needed on Linux.

### 3.2 `ab9de9fc` (Jetsku) - intrinsics header per compiler
`renderer/cache/bufferCache.cpp` and `src/common/hleWaitStats.h`: include the GCC/Clang
intrinsics header instead of the MSVC one outside MSVC. Pure upstream build fix; no 3p
dependency except that the hunk sits near 3p fast-path code in `bufferCache.cpp`. Keep.

### 3.3 `2423aeee` (Claude) - Linux port of the bridge
* Inside the 3p bridge file: host module lookup with the dynamic loader, "is the faulting
  instruction in the game image" via loader queries, System V argument passing to the resolver,
  hidden slot table, NID names exported as aliases of the thunks. All re-homed by writing the new
  bridge Linux-first.
* `tools/hybrid/build_kyty_gpu_linux.py`, `build_linux_shims.py`: ours; they read the 3p NID
  table format and the 3p module-definition file. Re-home on the new table format.
* `src/common/hostException.cpp`: the Linux first-chance handler (1.5). Edits upstream only.
  Keep as is.
* `src/kernel/memoryAddressSpace.inc`: the Linux adoption branch. It depends on the **3p adoption
  hook and its recorded-range list** in `memory.cpp`. Must be rewritten together with 2.6.
* `videoOut.cpp`: a Linux writability check inside the 3p game-step code (2.19, dropped; goes
  away). `hybridDlss.cpp` / `frameGen.cpp`: platform guards around 3p code.
* `window/window.cpp`: `KYTY_WAYLAND_NATIVE=1` selects the Wayland video driver first and asks SDL
  to scale the window to the display, so a 4K screen under fractional desktop scaling gets a true
  4K surface. Edits upstream `WindowCreate` only. Keep.
* `src/common/CMakeLists.txt`: exceptions enabled for three sources outside MSVC. Upstream build
  fix. Keep. `ProgramCodec.cpp`: explicit construction of an optional's nested type for the
  compiler. Keep.

### 3.4 `f14f408a` (Claude) - low-latency minimum interval
`ASTROHYBRID_LOW_LATENCY_INTERVAL_US` (1..999999 microseconds, default 0 = none; our product sets
33333 or 16666 to match the frame cap): passed as the minimum frame interval in the sleep mode
set on each new swapchain, only when the low-latency mode is on. Lives **inside the 3p
low-latency file**. Re-home in the new low-latency module (2.25). Known shortcut in our live
settings: the interval keeps its launch value when the cap changes at run time.

### 3.5 `f7c280a7` (Ali) - DualSense rumble switch
`ASTROHYBRID_DUALSENSE_RUMBLE=0` never sends motor-emulation rumble to a DualSense, because a
rumble report switches the pad out of audio-haptics mode. Edits upstream
`src/libs/dualSenseHaptics.cpp` (the rumble application routine) only. Keep.

### 3.6 `07606667` (Ali) - ordinary mappings search the whole user range
Section 1.5 "search range". It keys on "any donated range recorded", i.e. the **3p
recorded-range list**. Re-home on the new donation mechanism.

### 3.7 `457b1fa8` (Ali) - launcher removal
Deletes `src/launcher` and its tests and CMake wiring. No 3p dependency. Keep.

### 3.8 `100b3c35` (Ali) - AGS session socket client, protocol 2 (`src/streaming/`)
Protocol, transport, metrics and environment selection (`AGS_SESSION_SOCKET`,
`AGS_SESSION_PROTOCOL=2`). Hooks: CMake source glob and tests; `window/window.cpp` gains a
"presented frames" accessor and a window-lifetime mutex. Touches upstream code only. Keep.

### 3.9 `540abb40` (Ali) - live settings (`liveSettings*.cpp`)
Applies frame cap, output size and haptic strength while running.
* Frame cap: adds a "set frame limit" function to the **3p frame limiter** in `videoOut.cpp`,
  rewrites its label function, and makes the present thread reset its cadence when the numeric
  limit changes, not only the selection. Re-home on the new limiter (2.17): it needs "set limit
  to N FPS at run time, thread-safe, cadence restarts".
* Initial live state reads `ASTROHYBRID_FPS_LIMIT` and `KYTY_GPU_WIDTH` / `HEIGHT` and
  `APS5_PAD_VIBRATION_GAIN`. Re-home on the new configuration names.
* Cap changes that cross a 30/60 group also toggle executable patches through the patcher
  library found via `LD_AUDIT`; independent of 3p code.
* Output size: a pending-size request consumed by the window loop (`WindowContext::Run`),
  fullscreen state tracking. Upstream edits only.
* Haptic strength: the vibration-gain entry point (A14) added to the **3p bridge file and export
  list**. Re-home in the new bridge.

### 3.10 `76b39815` (Ali) - native frames on the session socket
Exporter (`exporter.*`, `aspectMode.h`, `captureOptions.h`), a capture class in
`window/swapchain.cpp` and two external-memory device extensions requested in
`vulkanWindow.cpp`, one flag in `graphicContext.h`. Upstream edits only; it shares
`Swapchain::Present` and device creation with the 3p low-latency and DLSS hooks, so expect merge
adjacency, not dependency. Keep.

### 3.11 `b8c695b6` (Ali) - NGX on Linux
CMake: link the Linux NGX static library. In the **3p DLSS file**: on Linux the NGX data
directory is a per-user cache folder, the runtime search path is the optional override plus the
folder the renderer library itself was loaded from. Re-home in the new DLSS module (2.21). The
release staging script checks that the renderer was built with NGX.

### 3.12 `a9743238` (Ali) - clang 18 build
One explicit construction in `shader/recompiler/ir/passes/ResourceTracking.cpp`. Upstream edit.
Keep.

### 3.13 `888eea21` - documentation only.

Re-homing checklist (our features anchored in code that will be deleted):

| Our feature | Current anchor (3p) | Needs from the rewrite |
|---|---|---|
| Pad audio entry + stream lifetime | bridge file, shutdown routine | A13, shutdown hook |
| Vibration gain entry | bridge file, export list | A14 |
| Rumble scale seeding | renderer option block | start-up configuration |
| Linux adoption of donated ranges | adoption hook + range list | 2.6 rewritten, Linux-first |
| Whole-range search | range list "non-empty" test | same |
| Low-latency interval | low-latency module | 2.25 |
| Live frame cap | frame limiter | 2.17 with a run-time setter |
| NGX paths on Linux | DLSS module | 2.21 |
| Shim and library build scripts | NID table format, export list | new generator output |

---

## 4. Open questions and risks

1. **Teardown of donated ranges on Linux.** The Windows release loop skips adopted ranges; no
   equivalent guard was seen for Linux. Whether `Memory::Shutdown` unmaps host-owned ranges at
   exit was not verified. Decide explicitly: donated ranges are never unmapped by Kyty.
2. **Who calls shutdown.** The window-close path asks the host to exit and waits forever for a
   shutdown call. No caller of the shutdown entry was confirmed on the host side. If the host
   simply exits the process, Kyty's teardown (cache saving included) never runs. Check whether
   persistent caches are flushed during play or only at teardown.
3. **Guest threads are not Kyty threads.** Kyty HLE functions are called on threads the host
   created; `Libs::LibKernel::PthreadLifecycle` is initialised but guest threads were never
   registered with it. It works today; which Kyty thread-local facilities silently degrade
   (profiler, watchdog, guest thread identity in the tracker) is unknown.
4. **Signal handler ordering.** The Linux fault handler chains to whatever was installed before
   it. If the host (or a library) installs its own `SIGSEGV` action later, Kyty's write tracking
   stops working without an error. The new design should make ordering explicit (the host
   installs first, or the host dispatches to Kyty).
5. **Lazy start blocks a guest thread** for the whole renderer bring-up (up to 120 s allowed)
   inside an arbitrary graphics call, while all other guest threads calling graphics queue on a
   lock. Eager start from the host before the game's entry point would be cleaner; whether the
   donation requests can be served that early is a host question.
6. **Event semantics are split.** The host implements trigger semantics for the game's queues
   (counting, clear-on-read); Kyty's own queue semantics for the same events are bypassed. The
   flip event's data and count fields were verified only as far as "forwarded"; flip-event
   deletion and vblank events are unimplemented.
7. **Only port 0.** Input, motion and pad output are relayed for one player. Multi-player would
   need per-port relays and per-port pad handles.
8. **Low-latency sleep on the guest thread** assumes one flip per DCB submission and holds a
   mutex shared with the present thread around driver calls. Its interaction with the frame
   limiter was tuned empirically (both set to the same rate).
9. **DLSS depends on title-specific facts** (shader hash, constant-buffer layout, binding order).
   A game update can change the hash and silently disable it (the game's own pass then runs).
   Decide whether to use upstream's DLSS/PSSR path instead (2.21).
10. **`KYTY_DRAW_PREP_PRIORITY` is set by our product but does nothing on Linux.** Remove it from
    the settings or implement it for Linux after measuring.
11. **Whole-emulator linkage.** The library contains the entire emulator; static initialisers of
    unused subsystems still run at load. Not known to cause harm; it inflates the library and
    the symbol-isolation burden.
12. **Version skew of the NID table.** The table is generated from one game build's registry. A
    different game version importing an extra graphics NID fails at load in the shim, not at
    generation time, unless the generator is rerun.
13. **Dead read-side fast path** (2.24): declared, unused. Confirms it can be dropped, but also
    shows the int22 merge altered 3p behaviour; other 3p features may have been affected the same
    way without notice.
