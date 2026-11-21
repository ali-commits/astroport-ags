# AstroPort step 5: session socket v2 and live settings, implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The port's renderer speaks AGS session socket protocol 2 (session environment, metrics,
native frames, `settings.apply`). It applies output size and haptic strength live and refuses a
30/60 switch with "Applies at the next start". The patcher gets a live write entry that checks
the expected bytes. AnyPS5's pad audio opens `AGS_CONTROLLER_AUDIO_SINK` when AGS names one.

**Architecture:** The protocol 1 client in KytyPS5-AGS `src/streaming` is copied into
`engines/kyty/src/streaming/`, cut to what AstroPort needs (no remote input, motion,
controller-audio or legacy socket) and moved to version 2. Tests drive it over a socketpair.
`settings.apply` goes through one pure decision function (`DecideSettingsApply`). It returns the
`settings.applied` reply and a short list of actions, and the transport thread runs those
actions on atomics: the frame limiter, a pending window size, the export size, the haptic gain.
Haptic strength crosses from Kyty to AnyPS5 through a new export, `KytyGpuPadVibrationGain`,
which AnyPS5 finds with `dlsym` the way it already finds `KytyGpuQueuePadAudio`. The patcher's
live entry, `astro_patch_live`, is called by the renderer only for a frame-rate change inside one
physics group. Today the groups are `{30}` and `{60}`, so every real 30/60 switch is refused and
that path stays dormant until 90/120 are added (design 6).

**Tech Stack:** C++20 (Kyty: clang, Vulkan, SDL3, nlohmann_json; AnyPS5: gcc, SDL2), Rust 2021
(patcher: serde, serde_json, libc), CMake/Ninja, Cargo.

**Spec:** `~/Games/astrobot-pc/native/docs/superpowers/specs/2026-11-21-astroport-design.md`
(sections 6, 7, 8 and 9 step 5). Protocol: `~/projects/AGS/.claude/worktrees/extension-protocol-2/docs/superpowers/specs/2026-11-21-extension-protocol-2-design.md`
(sections 6.2, 8.2, 8.3, 9.3). Reference client (read-only):
`~/projects/KytyPS5-AGS-p2`, branch `feat/ags-protocol-2` at 064e7b9c6, `src/streaming/*`,
`src/graphics/presentation/window/swapchain.cpp:330-605`, `vulkanWindow.cpp:2455-2475`,
`tests/StreamingTests.cpp`, `docs/streaming/PROTOCOL.md`. Host end: AGS
`docs/superpowers/plans/2026-11-21-m2-m5-interfaces.md` (the "m2-m5 sheet", worktree
`m2-m5-plans`; sections 2.1, 2.2, 2.5, 6.1 to 6.6, 9) and `2026-11-21-m1-interfaces.md` (the
"m1 sheet", sections 1.2.2, 1.3, 2.8). Where this plan and the sheet differ, the sheet wins.

## Global Constraints

- Never start the game, AnyPS5, AGS or any service in any step. Tests use fixtures, a socketpair
  and fake memory. Nothing here reads the owner's dump.
- Every test or build command has a hard `timeout` (values are given in each step). After a
  timeout, run `pkill -f AstroBot; pkill -f kyty_emulator` (they should never be running) and
  report.
- Never modify game dumps. Never add game files, firmware `.prx`, keys or `ngx-sdk` to the repo.
- No push and no release without the owner's OK.
- Implementers (Codex agents in a worktree) cannot commit. The controller runs each commit step
  exactly as written. Every commit message ends with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Edit only `engines/kyty/**`, `engines/anyps5/**` and `patcher/**`. Never edit `.gitignore`,
  `scripts/check-repo.sh`, `README.md`, `NOTICE.md` or `engines/*/UPSTREAM.md`; what this step
  needs there is listed under "Prelude and wrap-up needs".
- No `$HOME` path in any tracked file (`scripts/check-repo.sh` fails on it). Tests make temp files
  with `mkstemp`/`tempfile`.
- The patcher stays standalone (its own `Cargo.toml`/`Cargo.lock`, edition 2021, dependencies
  `serde`, `serde_json`, `libc` only). Add no crate.
- The session socket keeps protocol 1's transport rules: `AF_UNIX SOCK_SEQPACKET`, one JSON object
  per datagram, at most 8192 bytes, `SO_PEERCRED` UID check. Version 2 in `welcome`, `hello` and
  `accepted`. `hello` width and height bounds are 2 to 8192.
- Refusal text is fixed copy: `"Applies at the next start"` (frame-rate switch across physics
  groups, or a failed live patch), `"Not supported"` (unknown key or bad value),
  `"Leave fullscreen first"` (output size on a fullscreen local window with no frame export).
  Each is one line under 80 characters, as the m2-m5 sheet 6.1 requires of a `refused` reason.
- `settings.apply` `id` is an integer from 1, increasing per connection, one outstanding at a
  time; the reply echoes it, and the host waits 5 s (m2-m5 sheet 6.1).
- Haptic strength range is 1.0 to 4.0 (design 6), sent live as step 3's `controller.haptics`, an integer percent 100 to 400 (gain = value / 100). Frame-rate values are 30 and 60 (design 6).
- Never `LD_PRELOAD` (design 5.3). The patcher is found through the `LD_AUDIT` entry ending in
  `libastro_patcher.so`.
- Build check is `scripts/build-engines.sh` plus the unit tests named here. Never run the game to
  verify.

## Review Focus

1. **The renderer's `dlopen` of the patcher runs its constructor a second time.** The audit copy
   lives in its own link namespace, so the renderer gets a fresh copy whose `.init_array` would
   re-apply startup mods. That could flip the R1+X-toggled Decouple bytes back mid-game. Expected:
   the second copy does nothing at load. Task 1 adds `startup_wanted` and a test for it.
2. **A `settings.apply` with mixed good and bad values** (`"display.fps": "60"`,
   `"display.resolution": "abcx"`, `"0x0"`, `"9000x9000"`, a `controller.haptics` of 450, `"2"` or NaN, an unknown key).
   Expected: each bad key is refused with `"Not supported"`, the good keys still apply, and the
   connection stays open. Test in Task 4.
3. **`settings.apply` before `accepted`, or on a frame-less (metrics-only) connection.** Expected:
   before `accepted` the packet is ignored and logged once. On a frame-less connection it is
   answered, and an applied size change (or an applied frame rate that differs from the old one)
   is followed by one `output` message, which the host waits 5 s for (m2-m5 sheet 6.3 step 3).
   Test in Task 3.
6. **Local play: the host answers the frame `hello` with
   `{"type":"refused","version":2,"reason":"local"}`** (m2-m5 sheet 6.2). Expected: the worker
   sends `{"type":"hello","version":2,"frames":false}` on the same connection and carries on as a
   frame-less producer; the export pool is released and not recreated. Test in Task 3.
4. **`AGS_CONTROLLER_AUDIO_SINK` set to a name no device has, or set but empty.** Expected: a
   name no device has opens no pad device (no fallback to some other DualSense). Empty means
   unset, so today's match applies. Test in Task 2.
5. **Output size while fullscreen.** Expected: on a fullscreen local window with no frame export,
   the change is refused with `"Leave fullscreen first"`. With a frame export, the export size
   changes and a new pool `hello` follows (the window is left alone). Test in Task 4.

---

## File structure

| File | Responsibility |
| --- | --- |
| `patcher/src/lib.rs` (modify) | startup guard; `live_plan` (pure) and `astro_patch_live` (C entry); unit tests |
| `engines/anyps5/core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.{hpp,cpp}` (modify) | `AudioOut2PadDeviceWanted(name, sink)`, `AudioOut2HostVibrationGain()` |
| `engines/anyps5/core/libs/prx/libSceAudioOut/src/AudioOut.cpp:170`, `AudioOut2Context.cpp:163,235-242` (modify) | use the two helpers |
| `engines/anyps5/core/libs/prx/libSceAudioOut/tests/AudioOut2PadMix.cpp` (modify) | tests for both helpers |
| `engines/kyty/src/streaming/protocol.{h,cpp}` (create, from reference) | packet limit, pool descriptor, lease ring, known types (v2) |
| `engines/kyty/src/streaming/transport.{h,cpp}` (create, from reference) | socket worker: welcome/hello/accepted v2, frames, metrics, `settings.apply` dispatch, `output` |
| `engines/kyty/src/streaming/metrics.h` (create, from reference) | subscription parse, message build, pacer (catalog cut to two keys) |
| `engines/kyty/src/streaming/metricsSource.{h,cpp}` (create) | `game_fps`, `frame_time_ms` from the presented-frame counter |
| `engines/kyty/src/streaming/sessionEnv.h` (create, from reference) | `AGS_SESSION_SOCKET` + `AGS_SESSION_PROTOCOL=2` selection, `KYTY_STREAM_FRAMES` |
| `engines/kyty/src/streaming/liveSettings.{h,cpp}` (create) | `DecideSettingsApply` (pure), live atomics, `RunLiveActions`, `LivePatch` binding |
| `engines/kyty/src/streaming/exporter.{h,cpp}`, `aspectMode.h`, `captureOptions.h` (create, from reference) | Vulkan frame pool (native frames) |
| `engines/kyty/src/graphics/presentation/window/swapchain.cpp` (modify, `Presenter::Impl` ~314) | `NativeStreamExport`, metrics-only fallback |
| `engines/kyty/src/graphics/presentation/window/vulkanWindow.cpp` (modify, device extensions) | external memory/semaphore fd extensions when a session socket is set |
| `engines/kyty/src/graphics/presentation/window/window.cpp`, `windowInternal.h` (modify) | pending window size applied on the main loop; `fullscreen` atomic |
| `engines/kyty/src/graphics/presentation/videoOut.{h,cpp}` (modify, ~847-880) | `VideoOut::SetFrameLimit(uint32_t fps)` storing into `FrameLimitChoice()` |
| `engines/kyty/src/hybrid/kytyGpuBridge.cpp`, `kyty_gpu.def` (modify) | export `KytyGpuPadVibrationGain` |
| `engines/kyty/tests/SessionSocketTests.cpp`, `engines/kyty/tests/LiveSettingsTests.cpp` (create) | unit tests |
| `engines/kyty/CMakeLists.txt` (modify) | `streaming/*` in the emulator glob; two test targets with `add_test` and `TIMEOUT 30`; Linux NGX link (Task 7) |
| `engines/kyty/src/graphics/host_gpu/renderer/hybridDlss.cpp` (modify, ~465-475) | Linux ngx-data folder and the `libkyty_gpu.so` folder in the NGX path list (Task 7) |

Every copied file keeps its header and gets the line
`// Modified by AstroPort contributors: ported to AGS session protocol 2.` (design 3).

---

### Task 0: Build baseline (no code)

- [ ] **Step 1: Build both engines once so `build/kyty` and `build/anyps5` exist**

Run: `timeout 5400 scripts/build-engines.sh`
Expected: last line `engines built: libkyty_gpu.so, <n> system libraries, relinker, 3 forwarders`.

- [ ] **Step 2: Configure the AnyPS5 test build**

Run: `timeout 900 cmake -S engines/anyps5 -B build/anyps5-tests -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++ -DBUILD_TESTING=ON > build/anyps5-tests.log 2>&1; echo $?`
Expected: `0`.

---

### Task 1: Patcher live write entry and startup guard

**Files:**
- Modify: `patcher/src/lib.rs`

**Interfaces:**
- Produces (C ABI, exported from `libastro_patcher.so`):
  `#[no_mangle] pub extern "C" fn astro_patch_live(name: *const c_char, on: i32) -> i32`.
  It looks up the one mod in `ASTRO_PATCHES` whose lowercased name contains the lowercased
  `name`, at base `ASTRO_PATCH_BASE` (default `0x400000`), and writes that mod's `on`
  (`on != 0`) or `off` bytes. Returns: `0` written; `1` already in that state, nothing written;
  `-1` no mod or more than one matches; `-2` some entry's bytes are neither the expected ones nor
  the target ones, nothing written; `-3` an entry is unreadable or unmapped (a live write never
  maps a cave); `-4` file, environment or argument error.
- Produces (Rust, for tests): `fn live_plan(m: &Mod, on: bool, base: usize, read: &dyn Fn(usize, usize) -> Option<Vec<u8>>) -> Result<Vec<(usize, Vec<u8>)>, i32>`.
  It returns an empty Vec when every entry already holds the target bytes (the caller maps that
  to `1`), the full write list when every entry holds the expected bytes, and `Err(-2)` /
  `Err(-3)` otherwise. It is all or nothing: one entry in a mixed state refuses the mod.
- Produces: `fn startup_wanted(renderer_loaded: bool) -> bool` (returns `!renderer_loaded`).
  `run()` returns early when
  `startup_wanted(!libc::dlsym(libc::RTLD_DEFAULT, c"KytyGpuQueuePadAudio".as_ptr()).is_null())`
  is false. The audit-namespace copy never sees libkyty_gpu.so, while the copy the renderer
  `dlopen`s does.

- [ ] **Step 1: Write the failing tests** (in `#[cfg(test)] mod tests` at the end of `lib.rs`)

```rust
fn m(on: &str, off: &str) -> Mod { Mod { name: "Precise frame cap 60 FPS (x)".into(), enabled: false,
    memory: vec![Mem { offset: "0x10".into(), on: on.into(), off: off.into() }] } }
#[test] fn writes_when_expected_bytes_match() {
    let r = live_plan(&m("aabb", "0011"), true, 0x1000, &|a, n| { assert_eq!((a, n), (0x1010, 2)); Some(vec![0x00, 0x11]) });
    assert_eq!(r, Ok(vec![(0x1010, vec![0xaa, 0xbb])]));
}
#[test] fn turning_off_expects_on_bytes() {
    assert_eq!(live_plan(&m("aabb", "0011"), false, 0, &|_, _| Some(vec![0xaa, 0xbb])), Ok(vec![(0x10, vec![0x00, 0x11])]));
}
#[test] fn already_in_state_writes_nothing() {
    assert_eq!(live_plan(&m("aabb", "0011"), true, 0, &|_, _| Some(vec![0xaa, 0xbb])), Ok(vec![]));
}
#[test] fn foreign_bytes_refuse() {
    assert_eq!(live_plan(&m("aabb", "0011"), true, 0, &|_, _| Some(vec![0x12, 0x34])), Err(-2));
}
#[test] fn unreadable_refuses() {
    assert_eq!(live_plan(&m("aabb", "0011"), true, 0, &|_, _| None), Err(-3));
}
#[test] fn mixed_entries_refuse_whole_mod() {
    let mut x = m("aabb", "0011");
    x.memory.push(Mem { offset: "0x20".into(), on: "cc".into(), off: "dd".into() });
    assert_eq!(live_plan(&x, true, 0, &|a, _| Some(if a == 0x10 { vec![0x00, 0x11] } else { vec![0xcc] })), Err(-2));
}
#[test] fn second_copy_does_not_patch_at_load() {
    assert!(startup_wanted(false));
    assert!(!startup_wanted(true));
}
```

`Mod` and `Mem` need `#[derive(Debug, PartialEq)]` only if a test compares them; the tests above
compare only the results.

- [ ] **Step 2: Run the tests and see them fail**

Run: `cd patcher && timeout 600 cargo test --offline 2>&1 | tail -5`
Expected: compile error `cannot find function 'live_plan'` (and `startup_wanted`).

- [ ] **Step 3: Implement `live_plan`, `startup_wanted`, `astro_patch_live` and the guard in `run()`**

`astro_patch_live` reads the file each call (the call is rare), refuses a null or non-UTF-8 `name`
with `-4`, checks readability with `readable()`, reads the current bytes with
`std::slice::from_raw_parts`, and writes with the existing `write_mem` (which keeps pages RWX, as
the startup path does). It logs one `[astro-patcher] live on|off <mod>` or
`live SKIP <mod>: <reason>` line. Update the module doc comment with the new entry and the guard.

- [ ] **Step 4: Run the tests and the release build**

Run: `cd patcher && timeout 600 cargo test --offline 2>&1 | tail -3 && timeout 600 cargo build --release --offline 2>&1 | tail -1 && nm -D --defined-only target/release/libastro_patcher.so | grep -c -E ' (astro_patch_live|la_version)$'`
Expected: `test result: ok. 7 passed`, `Finished`, then `2`.

- [ ] **Step 5: Commit**

```bash
git add patcher/src/lib.rs
git commit -F - <<'EOF'
Patcher: live write entry that checks the expected bytes

astro_patch_live(name, on) writes one mod's on or off bytes only when every
entry holds the other state; a copy loaded after the renderer does not patch
at load.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 2: AnyPS5 controller sink name and live haptic gain

**Files:**
- Modify: `engines/anyps5/core/libs/prx/libSceAudioOut/src/AudioOut2PadMix.hpp`, `AudioOut2PadMix.cpp`
- Modify: `engines/anyps5/core/libs/prx/libSceAudioOut/src/AudioOut.cpp:170`, `AudioOut2Context.cpp:163` and `:235-242`
- Test: `engines/anyps5/core/libs/prx/libSceAudioOut/tests/AudioOut2PadMix.cpp`

**Interfaces:**
- Consumes: `float KytyGpuPadVibrationGain(void)` exported by libkyty_gpu.so (Task 4); it may be
  absent (older renderer, tests).
- Produces: `bool AudioOut2PadDeviceWanted(const char* name, const char* sink)`. A `sink` that is
  non-null and non-empty matches only a device whose name equals it exactly. Otherwise it falls
  back to today's `AudioOut2IsPadAudioDevice(name)`.
- Produces: `float AudioOut2HostVibrationGain(float fallback)`. It returns
  `KytyGpuPadVibrationGain()` clamped to [0, 4] when `dlsym(RTLD_DEFAULT, ...)` finds the symbol
  (the pointer is cached; `GetProcAddress` on `kyty_gpu.dll` on Windows, like
  `AudioOut2QueueHostPadAudio`), and `fallback` otherwise or when the value is not finite.

- [ ] **Step 1: Write the failing tests** (append to `main()` in `tests/AudioOut2PadMix.cpp`)

```cpp
Require(AudioOut2PadDeviceWanted("AGS Controller 1", "AGS Controller 1"), "the named sink is used");
Require(!AudioOut2PadDeviceWanted("DualSense Wireless Controller", "AGS Controller 1"), "a named sink excludes other pads");
Require(!AudioOut2PadDeviceWanted(nullptr, "AGS Controller 1"), "null name with a sink");
Require(AudioOut2PadDeviceWanted("DualSense Wireless Controller", ""), "empty sink is today's match");
Require(AudioOut2PadDeviceWanted("DualSense Wireless Controller", nullptr), "no sink is today's match");
Require(!AudioOut2PadDeviceWanted("HDMI Digital Stereo", nullptr), "no sink still skips other cards");
Require(AudioOut2HostVibrationGain(2.5f) == 2.5f, "without the renderer the launch value stays");
```

- [ ] **Step 2: Run the test and see it fail**

Run: `timeout 900 ninja -C build/anyps5-tests audio_out2_pad_mix_tests 2>&1 | tail -3`
Expected: error `'AudioOut2PadDeviceWanted' was not declared`.

- [ ] **Step 3: Implement both helpers and use them**

In `AudioOut.cpp:170` and `AudioOut2Context.cpp:163`, replace `AudioOut2IsPadAudioDevice(x)` with
`AudioOut2PadDeviceWanted(x, std::getenv("AGS_CONTROLLER_AUDIO_SINK"))`. In
`AudioOut2Context.cpp`, split `VibrationGain()` into the cached env value (`LaunchVibrationGain()`,
the existing lambda) and `VibrationGain()` returning
`AudioOut2HostVibrationGain(LaunchVibrationGain())`, which is read once per grain.

- [ ] **Step 4: Run the tests**

Run: `timeout 900 ninja -C build/anyps5-tests audio_out2_pad_mix_tests audio_out_vibration_device_tests 2>&1 | tail -1 && timeout 60 ctest --test-dir build/anyps5-tests -R 'audio_out2_pad_mix|audio_out_vibration_device' --timeout 30 --output-on-failure 2>&1 | tail -2`
Expected: `100% tests passed`.

- [ ] **Step 5: Commit**

```bash
git add engines/anyps5/core/libs/prx/libSceAudioOut
git commit -F - <<'EOF'
AnyPS5: open AGS_CONTROLLER_AUDIO_SINK when set; live haptic gain from the renderer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 3: Session socket v2 client (no frames yet)

**Files:**
- Create: `engines/kyty/src/streaming/{protocol.h,protocol.cpp,transport.h,transport.cpp,metrics.h,metricsSource.h,metricsSource.cpp,sessionEnv.h}`
- Modify: `engines/kyty/CMakeLists.txt` (glob ~285: add `"${KYTY_SOURCE_DIR}/streaming/*.cpp"`, `"${KYTY_SOURCE_DIR}/streaming/*.h"`; the test target below)
- Test: `engines/kyty/tests/SessionSocketTests.cpp`

**Interfaces:**
- Consumes: `WindowContext::loop.presented_frames` (`windowInternal.h:64`) through a getter
  `uint64_t Libs::Graphics::PresentedFrames()` added to `window.cpp`.
- Produces (`sessionEnv.h`): `SessionSocketChoice SelectSessionSocket(const char* ags_socket, const char* ags_protocol)`.
  It returns a path only when the socket is set and the protocol is exactly `"2"`; a protocol
  mismatch is logged once. Also `const char* SessionSocket()` and
  `bool FramesRequested(const char* kyty_stream_frames)` (false only for `"0"`).
  `KYTY_STREAM_SOCKET`, `MetricsOnlyAllowed`, `OverlayToggleAllowed` and `stream_only` are dropped.
- Produces (`transport.h`), the constructor:
  `Transport(std::string path, Completion complete, MetricsHooks metrics, SettingsHook settings, bool metrics_only)`,
  where `using SettingsHook = std::function<Json(const Json& apply)>`. The hook returns the
  complete `settings.applied` packet, which the worker sends. On a frame-less connection, after a
  reply whose `applied` holds `display.resolution`, or a `display.fps` that differs from the fps
  before the call, the worker also sends `{"type":"output","width":W,"height":H,"fps":F}` read
  from `Streaming::Live()` (Task 4); W and H 2 to 8192, F 1 to 360 (m2-m5 sheet 6.1). Nothing else
  sends `output` (the sheet defines it only "after an apply").
- On a frame `hello` answered `{"type":"refused","version":2,"reason":"local"}` (local play,
  m2-m5 sheet 6.2), the worker calls `SetMetricsOnly(true)` and sends the frame-less `hello` on the
  same connection; any other `reason` does the same and is logged once.
- A `metrics.subscribe` replaces the previous subscription: the host resends it on every change
  (m2-m5 sheet 6.1).
  `SetPool`, `Publish`, `Reconnect`, `SetMetricsOnly`, `Generation`, `Attached` and `Dropped` keep
  the reference signatures.
- Produces (`protocol.h`): `IsKnownPacketType` is `{"welcome","accepted","refused","error","release",
  "metrics.subscribe","settings.apply","session.state"}`. The input, motion, reset and
  controller-audio parsers, `ParseInput`, `ParseMotion`, `MotionOrder` and
  `ParseControllerAudioState` are not ported (streamed play uses AGS's virtual DualSense, design 8).
  `session.state` (`{"type":"session.state","state":"paused"|"resumed"}`, streamed sessions only,
  m2-m5 sheet 6.1) is accepted and logged, nothing more.
- Produces (`metrics.h`): `MetricCatalog` = `game_fps` (`fps`, number) and `frame_time_ms` (`ms`,
  number), `MetricsMinIntervalMs = 1000`. `metricsSource.cpp` computes both from the
  `PresentedFrames()` delta between samples (`frame_time_ms = 1000 / game_fps`; no value before
  two samples).
- The `hello` packets, both version 2: pool form as the reference with `"version":2,"frames":true`;
  metrics-only form `{"type":"hello","version":2,"frames":false}`. The host answers
  `{"type":"accepted","version":2,"epoch":E,"transport":"vulkan-buffer-fd"}` or
  `{"type":"accepted","version":2,"frames":false}` (m2-m5 sheet 6.1). The worker rejects a
  `welcome` or `accepted` whose `version` is not 2.

- [ ] **Step 1: Copy the reference files and add the test target**

Copy from `~/projects/KytyPS5-AGS-p2/src/streaming/` at 064e7b9c6 (`git -C ~/projects/KytyPS5-AGS-p2 show 064e7b9c6:src/streaming/<file>`),
then cut as the Interfaces block says. In `CMakeLists.txt`, next to `hang_watchdog_tests`:

```cmake
add_executable(session_socket_tests EXCLUDE_FROM_ALL "${KYTY_TESTS_DIR}/SessionSocketTests.cpp"
	"${KYTY_SOURCE_DIR}/streaming/protocol.cpp" "${KYTY_SOURCE_DIR}/streaming/transport.cpp")
target_include_directories(session_socket_tests PRIVATE "${KYTY_SOURCE_DIR}")
target_link_libraries(session_socket_tests PRIVATE nlohmann_json::nlohmann_json Threads::Threads)
add_test(NAME session_socket COMMAND session_socket_tests)
set_tests_properties(session_socket PROPERTIES TIMEOUT 30)
```

The test fakes the host: `socketpair(AF_UNIX, SOCK_SEQPACKET)` (bind a listening socket in a
`mkdtemp` folder if `Transport` needs a path), with `Check()` and `std::abort()` as in
`HangWatchdogTests.cpp`.

- [ ] **Step 2: Write the failing tests** (`tests/SessionSocketTests.cpp`)

```cpp
static void TestEnvironment() {
  Check(SelectSessionSocket("/s", "2").path != nullptr, "protocol 2 selects the socket");
  Check(SelectSessionSocket("/s", "1").path == nullptr, "protocol 1 is ignored");
  Check(SelectSessionSocket("/s", nullptr).path == nullptr, "missing protocol is ignored");
  Check(SelectSessionSocket("", "2").path == nullptr, "empty socket is unset");
  Check(!FramesRequested("0") && FramesRequested(nullptr) && FramesRequested("1"), "KYTY_STREAM_FRAMES");
}
static void TestMetricsOnlyHandshake() {      // host sends welcome v2; producer answers hello frames:false
  // expect {"type":"hello","version":2,"frames":false}; reply accepted v2; Attached() becomes true
}
static void TestWelcomeVersion1Rejected() {   // welcome {"version":1}: no hello is sent, connection closes
}
static void TestSettingsApplyRoundTrip() {    // after accepted: host sends settings.apply {"id":7,"values":{...}}
  // the SettingsHook receives the packet; the host receives exactly the Json the hook returned (id 7)
}
static void TestSettingsApplyBeforeAccepted() { // sent before accepted: hook not called, no reply, connection open
}
static void TestOutputAfterResolution() {      // frame-less, hook returns applied {"display.resolution":"1280x720"}
  // the next packet after settings.applied is {"type":"output","width":1280,"height":720,"fps":<Live fps>}
  // a hook reply whose applied holds only an unchanged display.fps is followed by no output
}
static void TestLocalRefusedFallsBackToFrameless() { // frame hello answered {"type":"refused","version":2,"reason":"local"}
  // the next packet is {"type":"hello","version":2,"frames":false}; reply accepted {"version":2,"frames":false}; Attached()
}
static void TestUnknownAndSessionState() {     // {"type":"session.state","state":"paused"} and {"type":"x.y"}: connection stays
}
static void TestMetrics() {                    // metrics.subscribe {"keys":["game_fps","nope"],"interval_ms":10}
  // interval clamps to 1000; BuildMetricsMessage only reports game_fps;
  // a second subscribe {"keys":["frame_time_ms"],"interval_ms":1000} replaces it: only frame_time_ms
}
int main() { TestEnvironment(); TestMetricsOnlyHandshake(); TestWelcomeVersion1Rejected();
  TestSettingsApplyRoundTrip(); TestSettingsApplyBeforeAccepted(); TestOutputAfterResolution();
  TestLocalRefusedFallsBackToFrameless(); TestUnknownAndSessionState(); TestMetrics();
  std::puts("session socket tests: ok"); }
```

Each test body uses `SendPacket`/`ReceivePacket` from `transport.h` with a 2 s `poll` bound per
receive. `TestOutputAfterResolution` sets the live fps through `Streaming::Live()` (Task 4). Until
Task 4 lands, give it a local stub in the test file. Task 4 deletes the stub.

- [ ] **Step 3: Run the tests and see them fail**

Run: `timeout 1800 ninja -C build/kyty session_socket_tests 2>&1 | tail -3`
Expected: compile errors for the version 2 names (`SelectSessionSocket` with two arguments, `SettingsHook`).

- [ ] **Step 4: Implement the cuts and version 2 in the copied files**

- [ ] **Step 5: Run the tests**

Run: `timeout 1800 ninja -C build/kyty session_socket_tests 2>&1 | tail -1 && timeout 60 ctest --test-dir build/kyty -R '^session_socket$' --output-on-failure 2>&1 | tail -2`
Expected: `100% tests passed, 0 tests failed out of 1`.

- [ ] **Step 6: Commit**

```bash
git add engines/kyty/src/streaming engines/kyty/tests/SessionSocketTests.cpp engines/kyty/CMakeLists.txt engines/kyty/src/graphics/presentation/window/window.cpp
git commit -F - <<'EOF'
Kyty: AGS session socket client, protocol 2

Ported from KytyPS5-AGS src/streaming (064e7b9c6): version 2 handshake,
metrics, settings.apply dispatch and output; no remote input or legacy socket.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 4: `settings.apply` decisions and live actions

**Files:**
- Create: `engines/kyty/src/streaming/liveSettings.h`, `liveSettings.cpp`
- Modify: `engines/kyty/src/graphics/presentation/videoOut.h`, `videoOut.cpp` (~847-880)
- Modify: `engines/kyty/src/graphics/presentation/window/window.cpp`, `windowInternal.h`
- Modify: `engines/kyty/src/hybrid/kytyGpuBridge.cpp`, `engines/kyty/src/hybrid/kyty_gpu.def`
- Modify: `engines/kyty/CMakeLists.txt` (test target)
- Test: `engines/kyty/tests/LiveSettingsTests.cpp`

**Interfaces:**
- Consumes: `astro_patch_live` (Task 1); `SettingsHook` (Task 3).
- Produces (`liveSettings.h`, namespace `Streaming`):

```cpp
struct LiveState { uint32_t fps; uint32_t width, height; float haptic; bool fullscreen; bool exporting; };
struct FrameCapRule { uint32_t fps; uint8_t group; };
inline constexpr std::array<FrameCapRule, 2> kFrameCapGroups {{{30, 0}, {60, 1}}}; // 90/120 join later (design 6)
inline constexpr std::string_view kHapticKey = "controller.haptics";               // step 3's item id, integer percent; a bare item id in settings.apply values (m2-m5 sheet 6.1, 1.4)
struct LiveActions { std::optional<uint32_t> fps; std::optional<std::pair<uint32_t, uint32_t>> size; std::optional<float> haptic; };
struct Decision { Json reply; LiveActions actions; std::optional<std::pair<uint32_t, uint32_t>> cap_swap; /* old, new */ };
Decision DecideSettingsApply(const Json& apply, const LiveState& now,
                             std::span<const FrameCapRule> groups = kFrameCapGroups);
std::atomic<LiveState>& Live();          // initial values: ASTROHYBRID_FPS_LIMIT, KYTY_GPU_WIDTH/HEIGHT, APS5_PAD_VIBRATION_GAIN (clamped 0..4, default 1)
float PadVibrationGain();                // Live().load().haptic
int   LivePatch(const char* name, bool on); // dlopen of the LD_AUDIT entry ending "libastro_patcher.so", RTLD_NOW|RTLD_LOCAL, cached; -10 when absent
Json  ApplySettings(const Json& apply);  // the SettingsHook: Decide, then the cap swap, then the actions; returns the reply
```

- `std::atomic<LiveState>` needs a trivially copyable `LiveState`. If the platform makes it not
  lock-free, keep a mutex-guarded struct instead; the readers are not hot.
- `DecideSettingsApply` rules, one per key in `values`:
  - `display.fps` takes an integer listed in `groups`. The same value as now is applied. Another
    value in the same group is applied, with `cap_swap = {now.fps, new}`. A value in another group
    is refused `"Applies at the next start"`. Anything else is refused `"Not supported"`.
  - `display.resolution` takes `"WxH"` with each side from 2 to 8192. When `exporting`, it is
    applied, `size` is set, and the export follows (Task 5). When `fullscreen` and not exporting,
    it is refused `"Leave fullscreen first"`. Otherwise it is applied and `size` is set. Anything
    else is refused `"Not supported"`.
  - `kHapticKey` takes an integer from 100 to 400 (percent, step 3's `controller.haptics`). It is
    applied and `haptic` is set to value / 100 (the gain, 1.0 to 4.0). Anything else is refused
    `"Not supported"`.
  - Any other key is refused `"Not supported"`.
  - The reply (m2-m5 sheet 6.1):
    `{"type":"settings.applied","id":<the request's id>,"applied":{standard key: value},"values":{item id: value},"refused":{key: reason}}`.
    `applied` holds only standard keys in the sheet's typed `Applied` form (sheet 1.2):
    `display.fps` an integer, `display.resolution` `"WxH"`. `values` holds the source item ids
    taken, here `controller.haptics` as the integer sent. Every key of the request lands in exactly
    one of the three maps. `id` is copied from the request as is (the host always sends an integer,
    sheet 6.1). A packet without an object `values` has no keys, so the reply has three empty maps.
- `ApplySettings` runs `cap_swap` first. It calls `LivePatch("precise frame cap <old> fps", false)`
  then `LivePatch("precise frame cap <new> fps", true)`; on any error it re-enables the old cap
  and moves `display.fps` from `applied` to `refused` with `"Applies at the next start"`. (The host
  sends `display.fps` only while step 3 declares it `"live":"live"`, m2-m5 sheet 6.1; today it is
  `next_launch`, so this path and the refusal are defensive.) Then it
  runs the actions:
  - `fps` goes to `Libs::Graphics::VideoOut::SetFrameLimit(fps)`. That stores the index of `fps`
    in `FRAME_LIMITS`, or `FRAME_LIMIT_CUSTOM` with `g_custom_frame_limit` for a value from 10 to
    240 that is not listed, into `FrameLimitChoice()`.
  - `size` goes to `Libs::Graphics::WindowRequestSize(w, h)`, which stores a packed pending size.
    `WindowContext::Run()` applies it on the main thread with `SDL_SetWindowSize` before
    `HostInputWaitEvent`; the existing `PIXEL_SIZE_CHANGED` path then recreates the swapchain.
  - `haptic` is stored into `Live()`, and `Live()` is updated with every applied value.
  - Comment the swap with: `shortcut: ASTROHYBRID_LOW_LATENCY_INTERVAL_US keeps its launch value; update it when a group gets a second value`.
- `WindowContext` gains `std::atomic<bool> fullscreen`. `WindowCreate` sets it, and so does
  `ToggleDesktopFullscreen`. `ProcessWindowEvent` updates it on
  `SDL_EVENT_WINDOW_ENTER_FULLSCREEN`/`LEAVE_FULLSCREEN` and mirrors it into `Live()`.
- `kytyGpuBridge.cpp` adds
  `extern "C" KYTY_GPU_EXPORT float KytyGpuPadVibrationGain() { return Streaming::PadVibrationGain(); }`,
  and `kyty_gpu.def` lists `KytyGpuPadVibrationGain` after `KytyGpuQueuePadAudio`.

- [ ] **Step 1: Add the test target** (same shape as `session_socket_tests`: name `live_settings_tests`,
  sources `tests/LiveSettingsTests.cpp` and `streaming/liveSettings.cpp`, test `live_settings`,
  `TIMEOUT 30`). The test links only `liveSettings.cpp`. `ApplySettings` and its calls into
  `VideoOut`/`Window` live in a second file, `liveSettingsApply.cpp`, so the pure part builds
  alone.

- [ ] **Step 2: Write the failing tests** (`tests/LiveSettingsTests.cpp`)

```cpp
static const LiveState kNow {30, 3840, 2160, 2.5f, false, false};
static Json Apply(Json values) { return {{"type", "settings.apply"}, {"id", 7}, {"values", values}}; }
static void TestFrameRate() {
  auto d = DecideSettingsApply(Apply({{"display.fps", 60}}), kNow);
  Check(d.reply["refused"]["display.fps"] == "Applies at the next start" && !d.actions.fps && !d.cap_swap, "30 to 60 refused");
  d = DecideSettingsApply(Apply({{"display.fps", 30}}), kNow);
  Check(d.reply["applied"]["display.fps"] == 30 && !d.cap_swap, "same value applied");
  const std::array<FrameCapRule, 2> one_group {{{30, 0}, {60, 0}}};
  d = DecideSettingsApply(Apply({{"display.fps", 60}}), kNow, one_group);
  Check(d.actions.fps == 60u && d.cap_swap == std::pair<uint32_t, uint32_t>{30, 60}, "same group swaps the cap");
  for (Json bad : {Json(45), Json("60"), Json(60.5)})
    Check(DecideSettingsApply(Apply({{"display.fps", bad}}), kNow).reply["refused"]["display.fps"] == "Not supported", "bad fps");
}
static void TestResolution() {
  auto d = DecideSettingsApply(Apply({{"display.resolution", "2560x1440"}}), kNow);
  Check(d.actions.size == std::pair<uint32_t, uint32_t>{2560, 1440} && d.reply["applied"]["display.resolution"] == "2560x1440", "windowed resize");
  for (const char* bad : {"abcx", "0x0", "9000x9000", "1920x", "1920x1080x2"})
    Check(DecideSettingsApply(Apply({{"display.resolution", bad}}), kNow).reply["refused"]["display.resolution"] == "Not supported", bad);
  LiveState full = kNow; full.fullscreen = true;
  Check(DecideSettingsApply(Apply({{"display.resolution", "1920x1080"}}), full).reply["refused"]["display.resolution"] == "Leave fullscreen first", "fullscreen local");
  full.exporting = true;
  Check(DecideSettingsApply(Apply({{"display.resolution", "1920x1080"}}), full).actions.size.has_value(), "fullscreen export follows");
}
static void TestHapticAndMixed() {
  auto d = DecideSettingsApply(Apply({{"controller.haptics", 300}, {"display.fps", "x"}, {"nope", 1}}), kNow);
  Check(d.actions.haptic == 3.0f && d.reply["values"]["controller.haptics"] == 300 && d.reply["applied"].empty()
        && d.reply["refused"].size() == 2 && d.reply["id"] == 7 && d.reply["type"] == "settings.applied", "mixed packet");
  for (Json bad : {Json(50), Json(450), Json("2"), Json(std::nan(""))})
    Check(DecideSettingsApply(Apply({{"controller.haptics", bad}}), kNow).reply["refused"].contains("controller.haptics"), "bad haptic");
  d = DecideSettingsApply(Json{{"type", "settings.apply"}, {"id", 8}}, kNow);
  Check(d.reply["id"] == 8 && d.reply["applied"].empty() && d.reply["values"].empty() && d.reply["refused"].empty(), "no values");
}
int main() { TestFrameRate(); TestResolution(); TestHapticAndMixed(); std::puts("live settings tests: ok"); }
```

- [ ] **Step 3: Run the tests and see them fail**

Run: `timeout 1800 ninja -C build/kyty live_settings_tests 2>&1 | tail -3`
Expected: `fatal error: 'streaming/liveSettings.h' file not found`.

- [ ] **Step 4: Implement `liveSettings.{h,cpp}` and `liveSettingsApply.cpp`, the `VideoOut`, window and bridge changes; pass `ApplySettings` as the `SettingsHook`; delete the Task 3 test stub**

- [ ] **Step 5: Run both test suites and check the export**

Run: `timeout 1800 ninja -C build/kyty live_settings_tests session_socket_tests kyty_emulator 2>&1 | tail -1 && timeout 60 ctest --test-dir build/kyty -R '^(live_settings|session_socket)$' --output-on-failure 2>&1 | tail -2 && timeout 1800 python3 -I engines/kyty/tools/hybrid/build_kyty_gpu_linux.py build/kyty build/kyty-gpu > /dev/null && nm -D --defined-only build/kyty-gpu/libkyty_gpu.so | grep -c ' KytyGpuPadVibrationGain$'`
Expected: `100% tests passed, 0 tests failed out of 2`, then `1`.

- [ ] **Step 6: Commit**

```bash
git add engines/kyty
git commit -F - <<'EOF'
Kyty: settings.apply for frame cap, output size and haptic strength

A 30/60 switch is refused with "Applies at the next start"; output size
resizes the window; haptic strength is a live atomic AnyPS5 reads through
KytyGpuPadVibrationGain.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 5: Native frames on the session socket

**Files:**
- Create: `engines/kyty/src/streaming/{exporter.h,exporter.cpp,aspectMode.h,captureOptions.h}` (from the reference, unchanged apart from the header line)
- Modify: `engines/kyty/src/graphics/presentation/window/swapchain.cpp` (`Presenter::Impl` ~314, and the present path that calls `Capture`)
- Modify: `engines/kyty/src/graphics/presentation/window/vulkanWindow.cpp` (device extension list), `engines/kyty/src/graphics/host_gpu/graphicContext.h` (`bool streaming_external_enabled = false;`)

**Interfaces:**
- Consumes: `Transport`, `SessionSocket()` and `FramesRequested()` (Task 3); `ApplySettings` and
  `Live()` (Task 4).
- Produces: `NativeStreamExport`, ported from the reference `swapchain.cpp:330-470` without
  `stream_only`, remote input or the system overlay. Its export size is `Live().load().width/height`
  when a resolution was applied while exporting, else `KYTY_STREAM_WIDTH`/`KYTY_STREAM_HEIGHT`,
  else the source image. Its rate is `Live().load().fps` when a frame rate was applied while
  exporting, else `KYTY_STREAM_FPS`. A size or rate change takes the existing `Reconnect()` path,
  which sends a new pool `hello` within the host's 5 s (the size and rate signal of a frame
  producer, m2-m5 sheet 6.1 and 6.3 step 3). `Impl` sets `Live().exporting` while a pool is
  attached.
- AGS sets only `AGS_SESSION_SOCKET` and `AGS_SESSION_PROTOCOL=2` (m2-m5 sheet 6.1).
  `KYTY_STREAM_FRAMES`, `KYTY_STREAM_WIDTH`/`HEIGHT`/`FPS` reach the renderer only from step 3's
  `launch.plan` `env` (see "Interfaces with other steps"); each one unset keeps its default.
- Metrics-only fallback, as the reference `swapchain.cpp:540-565`: a connection with
  `"frames": false` when `FramesRequested(getenv("KYTY_STREAM_FRAMES"))` is false, when the
  external fd extensions are missing, when `GpuPool::Create` fails, or when the host answers the
  frame `hello` with `refused` (Task 3).
- `vulkanWindow.cpp`: when `SessionSocket() != nullptr` and both
  `VK_KHR_external_memory_fd` and `VK_KHR_external_semaphore_fd` are available, enable them and
  set `streaming_external_enabled`, as the reference `vulkanWindow.cpp:2461-2475`.

- [ ] **Step 1: Port the files and the integration**

The command-scheduler completion check the reference calls `scheduler.IsFree(tick)` is whatever
`CommandScheduler` here offers for "this tick has completed"
(`graphics/host_gpu/renderer/commandScheduler.h`). If it offers none, add `bool IsFree(uint64_t tick) const`
there.

- [ ] **Step 2: Build and rerun the unit tests**

Run: `timeout 1800 ninja -C build/kyty kyty_emulator session_socket_tests live_settings_tests 2>&1 | tail -1 && timeout 60 ctest --test-dir build/kyty -R '^(live_settings|session_socket)$' --output-on-failure 2>&1 | tail -1`
Expected: `ninja: no work to do.` or a `Linking` line, then `100% tests passed`.

- [ ] **Step 3: Check that no socket code runs without the environment**

Run: `grep -n 'SessionSocket()' engines/kyty/src/graphics/presentation/window/*.cpp`
Expected: each hit sits inside an `if (... SessionSocket() ...)` guard, so a launch without
`AGS_SESSION_SOCKET` behaves as today. Read each hit; the guard is the check.

- [ ] **Step 4: Commit**

```bash
git add engines/kyty
git commit -F - <<'EOF'
Kyty: native frames on the AGS session socket, metrics-only fallback

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

### Task 6: Full build check

- [ ] **Step 1: Clean engine build**

Run: `timeout 5400 scripts/build-engines.sh`
Expected: `engines built: libkyty_gpu.so, <n> system libraries, relinker, 3 forwarders`.

- [ ] **Step 2: All tests of this step**

Run: `timeout 1800 ninja -C build/kyty session_socket_tests live_settings_tests > /dev/null && timeout 60 ctest --test-dir build/kyty -R '^(live_settings|session_socket)$' 2>&1 | tail -1; timeout 900 ninja -C build/anyps5-tests audio_out2_pad_mix_tests audio_out_vibration_device_tests > /dev/null && timeout 60 ctest --test-dir build/anyps5-tests -R 'audio_out2_pad_mix|audio_out_vibration_device' --timeout 30 2>&1 | tail -1; (cd patcher && timeout 600 cargo test --offline 2>&1 | grep 'test result')`
Expected: two `100% tests passed` lines and `test result: ok. 7 passed`.

- [ ] **Step 3: Repository rules**

Run: `timeout 120 scripts/check-repo.sh`
Expected: `repo OK: ...`. If the plan document itself trips the `$HOME` rule, report it; do not edit the script.

- [ ] **Step 4: Nothing ran**

Run: `pgrep -l -f 'AstroBot|kyty_emulator|ags-host' || echo none`
Expected: `none`.

No commit (no files change).

---

### Task 7: Linux DLSS (NGX) link and runtime paths (owner-run check)

Step 2's release (`inside.sh`, `ASTROPORT_REQUIRE_DLSS=1`) fails with `the renderer was built
without DLSS` until this lands. This step is the only plan that edits `engines/kyty/CMakeLists.txt`.

**Files:**
- Modify: `engines/kyty/CMakeLists.txt` (the `if(KYTY_NGX_SDK_DIR)` block, ~3181)
- Modify: `engines/kyty/src/graphics/host_gpu/renderer/hybridDlss.cpp` (NGX init, ~465-475)

**Interfaces:**
- Consumes: the SDK layout step 2 fetches (`include/`, `lib/Linux_x86_64/libnvsdk_ngx.a`,
  `lib/Linux_x86_64/rel/libnvidia-ngx-dlss.so.<n>`); step 4 copies the runtime into `<run>/libs/`
  next to `libkyty_gpu.so`.
- Produces: with `-DKYTY_NGX_SDK_DIR=<sdk>` on Linux, `libkyty_gpu.so` defines the
  `NVSDK_NGX_VULKAN_*` symbols (what step 2's `stage.sh` checks with `nm`). No change without the
  SDK. NGX is still enabled only by `ASTROHYBRID_DLSS=1` (step 3's `graphics.dlss`).

- [ ] **Step 1: Link the Linux NGX library**

In the `if(KYTY_NGX_SDK_DIR)` block keep the Windows line under `if(WIN32)`, and add an `else()`
branch: `target_link_libraries(kyty_emulator "${KYTY_NGX_SDK_DIR}/lib/Linux_x86_64/libnvsdk_ngx.a" ${CMAKE_DL_LIBS})`.
Update the comment above the cache variable to name both library paths.

- [ ] **Step 2: Linux paths in `hybridDlss.cpp`**

Off Windows (`#ifndef _WIN32`), replace the hard-coded `E:\AstroHybrid-Work\ngx-data` (~474) with a
writable folder: `$XDG_CACHE_HOME/astroport/ngx`, else `$HOME/.cache/astroport/ngx`, created on
first use (`std::filesystem::create_directories`, errors ignored; NGX then logs its own failure).
Replace the Windows default of the path list (~467) with two entries: `ASTROHYBRID_DLSS_PATH` when
set, and the folder holding `libkyty_gpu.so` (`dladdr` on a function of this file, the folder of
`dli_fname`), so NGX finds `libnvidia-ngx-dlss.so.*` next to it. Paths are widened to `wchar_t`
as the existing code does; `PathListInfo.Length` is the number of entries. Windows keeps today's
values.

- [ ] **Step 3: Build without the SDK (implementer)**

Run: `timeout 1800 ninja -C build/kyty kyty_emulator 2>&1 | tail -1`
Expected: a `Linking` line or `ninja: no work to do.` (the NGX code is not built in without the SDK).

- [ ] **Step 4: Owner-run check with the SDK** (configuring needs the real SDK, which no fixture
  replaces; implementers skip this step, as step 2 Task 5)

Run: `timeout 600 scripts/release/fetch-dlss.sh build/release/dlss-sdk && timeout 900 cmake -S engines/kyty -B build/kyty-ngx -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_C_COMPILER=clang -DCMAKE_CXX_COMPILER=clang++ -DCMAKE_CXX_FLAGS=-fPIC -DKYTY_NGX_SDK_DIR=$PWD/build/release/dlss-sdk > /dev/null && timeout 5400 ninja -C build/kyty-ngx kyty_emulator > /dev/null && timeout 1800 python3 -I engines/kyty/tools/hybrid/build_kyty_gpu_linux.py build/kyty-ngx build/kyty-ngx-gpu > /dev/null && nm build/kyty-ngx-gpu/libkyty_gpu.so | grep -c ' T NVSDK_NGX_VULKAN_Init_with_ProjectID$'`
Expected: `1`. Nothing is run; the game is never started.

- [ ] **Step 5: Commit**

```bash
git add engines/kyty/CMakeLists.txt engines/kyty/src/graphics/host_gpu/renderer/hybridDlss.cpp
git commit -F - <<'EOF'
Kyty: link NGX on Linux and find the DLSS runtime next to libkyty_gpu.so

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
EOF
```

---

## Interfaces with other steps

**Session socket (step 5 is the game end; AGS milestone 5 is the host end, m2-m5 sheet 6).**
- Environment set by AGS: `AGS_SESSION_SOCKET` (path) and `AGS_SESSION_PROTOCOL=2`, nothing else
  (m2-m5 sheet 6.1). The renderer ignores the socket unless the protocol is exactly `2`.
  `KYTY_STREAM_FRAMES=0` (frame-less), `KYTY_STREAM_WIDTH`, `KYTY_STREAM_HEIGHT` and
  `KYTY_STREAM_FPS` (export size and rate) are read as the KytyPS5 source reads them, but only
  step 3's `launch.plan` `env` sets them.
- Where the socket is (m2-m5 sheet 6.2): streamed with `capture: native`, a frame socket; streamed
  under Gamescope, a frame-less socket; local play with `live` in `hello.features`, a frame-less
  socket that answers a frame `hello` with `{"type":"refused","version":2,"reason":"local"}`;
  local play without `live`, no socket.
- Packets, game to host: `hello` v2 (pool form with `"frames": true` and 9 fds, or
  `{"type":"hello","version":2,"frames":false}`), `frame`, `metrics`
  `{"type":"metrics","t_ms":n,"values":{"game_fps":x,"frame_time_ms":y}}`,
  `{"type":"settings.applied","id":7,"applied":{"display.resolution":"2560x1440"},"values":{"controller.haptics":300},"refused":{"display.fps":"Applies at the next start"}}`,
  and `{"type":"output","width":W,"height":H,"fps":F}` (frame-less connections, after an applied
  size or a changed rate). A frame producer signals a new size or rate with a new pool `hello`.
- Packets, host to game: `{"type":"welcome","version":2}`, `accepted` v2 (frame form with
  `epoch` and `transport`, or `"frames": false`), `refused` (local play), `error`, `release`,
  `metrics.subscribe` (resent on every change; each replaces the last),
  `{"type":"settings.apply","id":<integer from 1>,"values":{"display.resolution":"WxH","display.fps":int,"controller.haptics":int 100..400}}`
  (only keys whose live class is `live`), and `{"type":"session.state","state":"paused"|"resumed"}`
  (accepted, ignored).
- What the host does with the reply (m2-m5 sheet 6.3, 9): a key in `refused` becomes
  `live-refused` with `layer: "game"` and this plan's reason text; no reply within 5 s becomes
  `live-timeout`; a key in none of the three maps reads as refused with `""`.

**Step 3 (adapter) needs to declare, for this renderer:**
- `hello.features` includes `native_frames`, `metrics` and `live` (m2-m5 sheet 6.2: without
  `live` local play gets no socket, so the overlay cannot reach the renderer); the metrics catalog
  is
  `[{"key":"game_fps","label":"Game FPS","unit":"fps","type":"number","min_interval_ms":1000},
  {"key":"frame_time_ms","label":"Frame time","unit":"ms","type":"number","min_interval_ms":1000}]`
  (labels are step 3's to word).
- `launch.plan` `env`: `KYTY_STREAM_FRAMES=0` unless params have `"play":"stream"` (m2-m5 sheet
  2.5) and `"capture":"native"` (m1 sheet 2.8). The renderer also falls back on `refused`, so a
  local launch without it still works.
- `standard` in `page.describe` (m2-m5 sheet 1.2 `Support`, 2.2): `display.resolution`
  `{"level":"honoured","live":"live"}`; `display.fps`
  `{"level":"honoured","values":[30,60],"live":"next_launch"}` (design 6; the renderer refuses a
  live switch anyway). `applier` stays the default `source`. Under Gamescope the host itself
  forces `display.resolution` to `next_launch` (sheet 2.4 step 3).
- Live haptic strength is step 3's own `controller.haptics` item (integer percent 100 to 400) with
  `"restart":"live"`; at launch step 3 sends value / 100 as `APS5_PAD_VIBRATION_GAIN`. The host
  sends it in `settings.apply` `values` under its bare id, beside the standard keys (m2-m5 sheet
  6.1, 1.4). The overlay draws Resolution and Frame rate from `standard` and draws only the page's
  `overlay` sections otherwise (m2-m5 sheet 6.6), and `page.set` is refused while the game runs.
  Ids are unique across a description (m1 sheet 1.3), so for haptics to change during play,
  step 3 puts `controller.haptics` in an `overlay` section (m1 sheet 1.2.2: no `tab`, a `title`)
  and not on the panel. Where it sits is step 3's call; this step needs only the id and the range.
- Launch env: `LD_AUDIT` holds exactly one entry ending in `libastro_patcher.so` (the install
  folder's `lib/libastro_patcher.so`), as `play-hybrid.sh` does. The renderer finds the patcher
  there.

**Exported C symbols (both inside the release package, no layout change):**
- `lib/libkyty_gpu.so`: `float KytyGpuPadVibrationGain(void)`.
- `lib/libastro_patcher.so`: `int32_t astro_patch_live(const char* name, int32_t on)`, with return
  codes as in Task 1. Mod names are matched as a lowercase substring; the renderer uses
  `"precise frame cap <N> fps"`.

**AnyPS5:** reads `AGS_CONTROLLER_AUDIO_SINK` (exact SDL audio device name). If it is empty or
unset, the old DualSense name match applies.

No change to the install-folder layout, the data folder or the version list. Task 7 makes the
renderer link NGX on Linux (step 2's DLSS release needs it) and look for `libnvidia-ngx-dlss.so.*`
in the folder of `libkyty_gpu.so` (`<run>/libs/`, where step 4 copies it); NGX writes its data
to `$XDG_CACHE_HOME/astroport/ngx` (else `~/.cache/astroport/ngx`).

## SHEET items

Reconciled with the m2-m5 sheet. Resolved (each now in the task text above):

- `settings.applied` shape: `{type, id, applied, values, refused}`; `applied` is typed standard
  keys only, `values` the source item ids taken, `refused` key to one line of at most 80
  characters (sheet 6.1, 1.2 `Applied`).
- A source's live item reaches the game under its bare item id in `settings.apply.values`,
  sharing one namespace with the standard keys (sheet 6.1, 1.4); `controller.haptics` as planned.
- `settings.apply` `id` is an integer from 1, one outstanding, 5 s timeout (sheet 6.1).
- `display.resolution` is `"WxH"` and `display.fps` an integer, in `settings.apply` and in
  `applied` (sheet 2.1 value forms, 1.2 `Applied`).
- `session.state` is `{"type":"session.state","state":"paused"|"resumed"}`, streamed only (sheet
  6.1); accept-and-ignore stands.
- `output` is sent only after an apply: after an applied size or a changed rate, on frame-less
  connections; width and height 2 to 8192, fps 1 to 360. A frame producer sends a new pool
  `hello` instead, for size and rate alike (sheet 6.1, 6.3 step 3).
- AGS passes only `AGS_SESSION_SOCKET` and `AGS_SESSION_PROTOCOL=2`; frame or frame-less follows
  from `launch.plan` `play` and `capture`, so `KYTY_STREAM_*` are step 3's to set (sheet 6.1,
  6.2, 2.5; m1 sheet 2.8).
- Local play: frame `hello` answered `refused` with `reason: "local"`, frame-less `hello` on the
  same connection after (sheet 6.2); `live` feature needed for the local socket.
- Overlay: `standard[key].live` for Resolution and Frame rate, `overlay` page sections for
  source items (sheet 6.6).

Still open:

- SHEET: a `settings.apply` without an integer `id`. The sheet says the host always sends one;
  this plan echoes whatever `id` came (absent stays absent) and still answers. (`DecideSettingsApply`)
- SHEET: a frame `hello` answered `refused` with a `reason` other than `local`. The sheet defines
  only `local`; this plan falls back to frame-less for any reason and logs it once. (`transport.cpp`)
- SHEET: `AGS_CONTROLLER_AUDIO_SINK` value. The sheet leaves it to m6 (sheet 10, design 9.3).
  This plan assumes the device name SDL2 lists (on PulseAudio that is the sink description, not
  the sink name). (`AudioOut2PadDeviceWanted`)

## Prelude and wrap-up needs

- `engines/kyty/UPSTREAM.md`: under the port's changes, add "AGS session socket client
  (`src/streaming/`) ported from KytyPS5-AGS `src/streaming` at 064e7b9c6 (Ali, GPL-2.0), updated
  to session protocol 2; live settings (`liveSettings*.cpp`), `KytyGpuPadVibrationGain` export."
- `engines/anyps5/UPSTREAM.md`: add "`AGS_CONTROLLER_AUDIO_SINK` pad audio device; live haptic
  gain read from the renderer."
- `.gitignore`: nothing new (`/docs/` is already whitelisted; `build/` is already ignored,
  `build/anyps5-tests` included).
- `scripts/check-repo.sh`: nothing required. Optional rule-7 entries for files this step
  creates: `engines/kyty/src/streaming/transport.cpp`, `patcher/src/lib.rs` (already listed).
- `README.md` / `NOTICE.md`: nothing beyond the UPSTREAM lines (no new third-party code; the
  ported files are the owner's).
