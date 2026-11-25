# Pending verification

Everything below was written and compiled or unit-tested, but not run in the game, because game runs
and full builds were paused. Work through it once they may resume. Each item is a command or an
observation and what to expect.

## Results of the first verification round (2026-11-25)

Machine: RTX 3090, NVIDIA 620.73, KDE Wayland, 60 Hz. Build: `BUILD_DIR=build-rewrite JOBS=6
scripts/build-engines.sh` (frame generation off), level runs with `LEVEL=underwater_aerial_garden`
through scripts/smoke.sh, logs in build-rewrite/verify/. From the second round on every run went
through the shared test slot; earlier runs shared the GPU with another agent's stream tests.

Verified:

- Full build: libkyty_renderer.so exports only kyty_embed_api; the forwarder import check and the
  export-table check pass; kyty_emulator links. Kyty ctest dlss, gyro_bias, image_memory_pressure,
  frame_gen pass; bridge, adapter, patcher and runner cargo tests pass.
- AnyPS5 ctest: 2757 tests, 6 fail, all repository tooling that reads AnyPS5's .github/ folder or
  vendored mbedtls test scripts, which this repository does not carry (pr_overlap_tools,
  conventions_ci_gate, test_script_registration, build_eligibility, sanitizer_report,
  conventions_pr_template). No engine test fails.
- Start on the bridge: renderer ready in 515-878 ms, first frame after 1.3-1.9 s, the title and the
  level load, clean exit (exit line, write faults handled, 0 passed on) in every run.
- Exit reason: with the new 'Window: close requested' / 'Window: quit event' lines a run ended by the
  time limit shows only the quit event. The early exit at 106 s in run 2 (before those lines) is not
  explained; no later run repeated it.
- Limiter: 30 cap holds 30.0 fps (median 33.33 ms); the warning without KYTY_PRESENT_ABS_VBLANK=1
  appears. play.sh at FPS_LIMIT=60 left the game's 30 FPS cap patch on (fixed: it now selects the
  cap patch of the rate). At a 60 cap this level reaches about 33-36 fps on this machine.
- Low latency: 'LowLatency: on (VK_NV_low_latency2)', 'sleep mode on (boost)', 64 of 64 reports
  filled, sleeps grow by one per frame, 0 time-outs, at 30 and 60; FIFO present mode runs without a
  stall. Cost: at a 60 cap it lowers this level from 36.0 to 31.4 fps and raises the 99th percentile
  frame time from 37 to 52 ms (owner decision: play.sh defaults LOW_LATENCY=1).
- Video override (A5): during the title intro the low-latency interval switches to 16666 us and back
  to 33333 us at a 30 cap.
- DLSS 8a facts: the pass line, buffer 0 = jitter 0/1, output 4/5 (2432x1368), input 6/7
  (1920x1080), frame 11, motion flag 13, which equals ASTROPORT_DLSS_LAYOUT in patches/settings.json
  (play.sh now sets it too); image 0 is the motion target, drawn with D32S8 depth, compare
  LessOrEqual (depth not reversed: ASTROPORT_DLSS_REVERSE_DEPTH stays 0); target 0 has storage 1, an
  equal view format and mapping 0xe4, so DLSS can engage. NGX starts its extensions without a device
  error but says super resolution is unavailable (0xbad00004): the DLSS runtime
  (libnvidia-ngx-dlss.so) is not on this machine (scripts/release/fetch-dlss.sh, not run).
- K8b: no 'Draw dropped:' line in any run. K4: no pipeline list file in the run folder, skipped
  silently as designed.
- Release: scripts/release/test.sh (38) passes; BUILD_DIR=build-rewrite scripts/release/stage.sh
  stages 369 system libraries and strips; package.sh makes an 81 MB tarball; astroport-run build
  --install <stage> makes a run folder, and the game starts from it (stripped libraries).
- Frame generation (rewrite/fsr-framegen): configure ON fetches and checks the SDK, links with
  --no-undefined, exports only kyty_embed_api, 11 frame_gen tests pass. In game after three fixes
  (A2R10G10B10 copy-through, scratch alignment patch, separateDepthStencilLayouts): 294 of 294
  frames generated at a 30 cap, 3840x2160, contexts in 449 ms, 199 MiB; under the Khronos layer no
  validation error (one warning: a descriptor set allocated from a pool without storage-buffer
  entries, source not identified).

Not verifiable here without the owner (hands, eyes or decisions):

- Pads: two-pad takeover, DualSense over Bluetooth and USB (speaker, haptics, triggers, light bar,
  sleep and wake, plug during play), Switch Pro gyro offset (K1), Xbox rumble scale, unknown pad type.
- Picture: DLSS (needs the runtime), frame generation smoothness, halos and HUD smear, jitter sign.
- AGS: session frames, reconnects, settings.apply over the socket, the installed product. The AGS on
  this PC is the other agent's streaming test build and must not be replaced.
- Native Wayland 4K needs fullscreen (windowed, KDE shrinks the window to 2508x1412); window icon
  under X11 (K3); K7 prepare run from the adapter.
- K5/K8a: a 2500 MiB KYTY_VRAM_BUDGET_MB never ran this level out of video memory; the full-memory
  path stays unproven.

## M7 presentation features

- Full build first: scripts/build-engines.sh (links libkyty_renderer.so; build check 1 must still show only kyty_embed_api exported) and ninja -C <build>/kyty kyty_emulator to confirm the stand-alone emulator still links.
- Limiter: scripts/smoke.sh with ASTROPORT_FPS_LIMIT=30 and then 60, KYTY_PACE_LOG=<file> (or the session metrics); expect 30.0 and 60.0 FPS.
- Limiter warning: one start with ASTROPORT_FPS_LIMIT=30 and KYTY_PRESENT_ABS_VBLANK unset; expect 'astroport: warning: ASTROPORT_FPS_LIMIT is set without KYTY_PRESENT_ABS_VBLANK=1'. No warning with it set to 1.
- Low latency on (ASTROPORT_LOW_LATENCY=1, NVIDIA): log shows 'LowLatency: on (VK_NV_low_latency2)' and 'LowLatency: sleep mode on (boost), minimum interval 33333 us -> Success' at a 30 cap. Every 10 s 'LowLatency: N of M driver reports filled ... sleeps S, time-outs T' must have N > 0, S growing by about one per frame (S = 0 means the guest wrappers were not installed) and T small. Frame rate stays at the cap, no hang, no validation error on window resize.
- Low latency with ASTROPORT_PRESENT_MODE=fifo: no stutter from the guest thread waiting behind a present (the low-latency mutex is held across vkQueuePresentKHR).
- Low latency unavailable: on a device without VK_NV_low_latency2 the log shows 'LowLatency: unavailable (the device lacks VK_NV_low_latency2 or VK_KHR_present_id)' and the game runs normally.
- Low latency with the video override: during the studio intro the interval line changes to 16666 us and back afterwards; intro in sync at caps 30, 120 and off.
- Low latency with live cap change: settings.apply display.fps 30 -> 60 over the session socket prints a new 'sleep mode ... 16666 us' line.
- Pad selection (ASTROPORT_PAD_PREFER=auto, two pads): pressing a button on the second pad logs 'Controller N takes player 1' and it controls the game; a non-PlayStation pad buzzes about 150 ms; each connection logs 'Controller N connected: ...'. Repeat with ASTROPORT_PAD_PREFER=ps and one Xbox pad plus one DualSense: the Xbox pad must not take over.
- Pad of unknown SDL type (a generic pad without a mapping type, if one is at hand) with ASTROPORT_PAD_PREFER=auto: connects without a crash and logs 'type unknown'.
- Rumble on an Xbox pad at ASTROPORT_RUMBLE_SCALE=150 is stronger than at 100; on a Switch Pro pad with ASTROPORT_RUMBLE_SWITCH_FLOOR=96 weak rumble is felt.
- ASTROPORT_DS_RUMBLE=0 on a DualSense: audio haptics (footsteps) keep working after a strong effect; no motor rumble is sent.
- Bluetooth DualSense: pad speaker and haptics play (console line 'DualSense speaker audio ...: Bluetooth HID audio'); no deadlock after ten minutes. Also check a USB DualSense speaker still plays, since the speaker report now sets bytes 1 = 0x82 and 9 = 0x10 on both paths; listen for clicks in speaker sounds (per-sample mono mix).
- Live output size: settings.apply display.resolution "2560x1440" resizes the window within a moment while windowed; in fullscreen nothing happens.
- Live haptic gain: settings.apply controller.haptics 300 changes haptic strength during play.
- Wayland native: ASTROPORT_WAYLAND_NATIVE=1 on a 4K display under fractional scaling gives a true 3840x2160 surface (swapchain extent in the log).
- Window title fixed at 'Astro Bot' with ASTROPORT_WINDOW_TITLE set.

Open points:

- Nothing was linked or run. The low-latency Vulkan sequence is checked by reading only; its first real test is the game run.
- Low latency: a GPU flip rejected because the flip queue is full advances the guest counter but not the reservation counter, so simulation markers and present ids drift by one from then on. No cheap, clearly correct resync; left as is.
- Low latency: the guest thread can block up to 100 ms per frame (design risk 17), and in FIFO a guest thread in FlipQueue::Reserve waits behind vkQueuePresentKHR for up to a vblank while holding the flip-queue lock.
- Speaker mono mix decides average or sum per sample; stereo content with differing channels can click. Kept because it is our hardware-tuned code; a per-buffer decision would be the fix if clicks are heard.
- DualSense speaker report byte 9 = 0x10 mutes the pad microphone and is not undone when routing returns to headphones; now applies to USB too.
- When the pad's serial is unknown or unmatched, audio goes to the only Bluetooth DualSense present, which could be a different pad than the active one (for example an active USB pad whose connection state SDL reports as unknown).
- The haptic-audio approximation for non-DualSense pads is not scaled by rumble_scale or the Switch floor, and competes with the bridge's audio-derived rumble.
- After a live resize the session socket still reports the launch width and height, and the bridge's fullscreen flag is the launch value only.
- With ASTROPORT_PAD_PREFER=auto any newly connected pad or any button press on a second pad takes player 1, even while the first pad is in use; with a family preference a pad stranded on player 2 only moves up on its next button press. Spec behaviour, worth confirming as wanted.
- No Kyty unit test for PadFamily / PreferPad or the low-latency counters: tests/ is outside my files.
- The build directory re-runs the CMake glob check on every ninja call; harmless but it touched generated files in build-rewrite/kyty.
- The working tree holds other agents' uncommitted changes (bridge/src/abi.rs, AnyPS5 files, scripts, patches); I did not touch them.

## M8 DLSS

- Full build first: scripts/build-engines.sh (render.h changed, so every renderer object recompiles); then ctest --test-dir build/kyty -R '^dlss$'.
- 8a, does NGX start: run with ASTROPORT_DLSS=1 (scripts/smoke.sh) and look for 'DLSS: upscaling PSSR with the model in <folder>' and 'DLSS: NGX started for the upscale pass 2936e5eca99d75e7; data folder <dir>'. Failure lines now end with '; the title's own upscale pass runs.': 'WARNING: DLSS: the NVIDIA driver has no NGX library', 'WARNING: DLSS: NGX did not start (0x...)', 'WARNING: DLSS: super resolution is unavailable (0x...); check the model in <folder>'.
- 8a, device creation with the arena reserved (risk 13): look for 'DLSS: NGX asks for N device extensions, added: ...' followed by no 'Could not create device', then the NGX start line. If NGX fails here, the fallback is the late third donation of the design.
- 8a, is the hash the right kind: the line 'DLSS pass: shader 2936e5eca99d75e7 drawn; NGX on|off; layout given|not given; reverse depth configured 0|1' must appear once. If it never appears, CompiledShaderInfo::shader_hash is not the hash the spec's constant refers to, and ASTROPORT_DLSS_PASS needs the value Kyty computes.
- 8a, which render sizes Choose accepts: for each of the four render settings (1080p, 1368p, 1872p, 2160p) look for 'DLSS pass: WxH -> WxH: quality mode M, render sizes a..b; modes at this output: ...' or '...: no quality mode takes this render size; modes at this output: ...'.
- 8a, images: the once-only lines 'DLSS pass: image <i> 0x<addr> <vk format> WxH samples <n> storage <0|1>' and 'DLSS pass: target <i> ... view <vk format> export mapping 0x<hex>'. Check that image 0 is the motion target and image 1 the anti-aliased colour, all have 'samples 1', and target 0 has 'storage 1', a view format equal to its image format, and 'export mapping 0xe4' (identity). If any of the three target conditions does not hold, the summary shows 'images' and DLSS cannot engage without a code change (a storage-capable intermediate or a swizzled copy).
- 8a, constant layout: the line 'DLSS pass: buffer <i> 0x<base> <size> bytes: <16 hex words>'. Check that buffer 0 holds jitter in words 0 and 1, output size in 4 and 5, input size in 6 and 7, frame counter in 11 and the motion flag in 13; correct ASTROPORT_DLSS_LAYOUT in patches/settings.json if not. A jitter that is not in UV units (more than one input pixel after scaling) now shows as the fallback reason 'sizes'.
- 8a, depth convention: on the motion image's line, '; drawn with depth 0x<addr> <format> WxH ..., compare <op>, clear <value>'. Compare Greater or GreaterOrEqual (or clear 0) means reversed depth: then add ASTROPORT_DLSS_REVERSE_DEPTH=1 to fixed_env in patches/settings.json. If the line has no 'drawn with depth' part, the ring had no entry yet and the summary shows 'no depth'.
- 8b: during play the ten-second line should read 'DLSS pass: N evaluated, no fallbacks'. Other reasons appear as 'fallbacks: <reason> <count>' with reasons NGX off, continued draw, bindings, constants, sizes, motion unused, no depth, images, refused. A steady 'continued draw' count would mean the pass is always a draw-run continuation and the hook needs to leave the run first.
- 8b: __NGX_SHOW_INDICATOR=1024 shows the driver's DLSS indicator.
- 8b: with libnvidia-ngx-dlss.so.* removed from the run folder's libs/, the game's own pass runs: expect a 'WARNING: DLSS: ...' line at start and 'DLSS pass: 0 evaluated, fallbacks: NGX off N'.
- Picture check: jitter sign and direction (no sign switch was built; shimmer or blur on a still camera means the sign of one axis is wrong), ghosting (motion-vector scale), and whether the colour input is HDR as the creation flag assumes.
- Later passes after a DLSS frame: the pass may have further colour targets (a history copy) that DLSS does not write; the 'target <i>' lines show how many there are. Watch for a one-frame ghost when a fallback frame follows DLSS frames.
- Validation layers once (the Khronos layer) on the DLSS frame: inputs are handed to NGX in GENERAL layout, as upstream's Pro-mode path does; also confirms the storage view of the output and the sampled depth view.
- Rare path, by forcing it once (for example a wrong preset so creation fails): after 'WARNING: DLSS: creating a ... feature failed' the title's own pass must draw correctly in the same frame, and the summary must show 'refused' from then on.

Open points:

- Nothing was linked or run. The Vulkan sequence in UpscaleResolved and the hook's interaction with the draw-run state are checked by reading and compiling only.
- The new output preconditions (view format equals image format, identity export mapping, storage usage) may turn out to be false for this title's upscale target. Then DLSS never engages ('images') until a copy through a storage-capable image is added; the 8a 'target 0' line settles it.
- When the cache image is larger than the output, NGX is told the output resource has the pass's size rather than the image's. That matches the equal-size case exactly and is my best reading of NGX for the larger case, but it is unverified.
- scripts/play.sh sets ASTROPORT_DLSS_PASS but not ASTROPORT_DLSS_LAYOUT, so DLSS cannot engage through play.sh until one line is added (not my file). The adapter path gets it from patches/settings.json.
- design.md (3.1 struct and size, 7.1 variable table, 6.4 table) and engines/kyty/UPSTREAM.md do not yet describe the new field, variable, ForgetBoundState, DescribeModes, the extension filter or the output_size parameter; those files are outside my list.
- Whether the layout's image and buffer indices (order of the recompiled shader's images and buffers) equal the 'first bound image / second' order of spec 2.21 is unverified; the once-only log lines settle it.
- Whether the texture binding's guest address equals the colour target's guest address for the motion image (the key of the depth ring) is unverified; a steady 'no depth' count would show a mismatch.
- Rare path: if creation or evaluation fails after rendering was ended, the title's draw continues with bindings prepared before. I traced that the run, seed and feedback records are invalid by then, so targets are re-acquired in full, but a buffer-cache change inside the two cache calls would not be re-resolved. At most once per size pair.
- While DLSS runs, any further colour target of the pass (for example a history copy) is not written.
- Pssr::kJitteredMotion is bit 31 of the same flags word a guest PSSR request carries in Pro mode; a guest that set bit 31 would get the jittered-motion creation flag. Not reachable in the embedded product (Pro mode is off).
- With ASTROPORT_DLSS=1 and a pass hash set, every draw feeds the 16-entry ring even when NGX did not start (kept so the pass facts are still logged).
- scripts/check-repo.sh fails on files outside this step, per the implementer: '$HOME' in docs/rewrite/spec-anyps5.md and the removed bridge's name in docs/superpowers/plans/* and patcher/src/lib.rs. I did not re-run it.
- Redistribution of the DLSS runtime (design decision 2) is still open.

## K ideas (K1 K5 K7 K8, verify K3 K4 K6)

- Full build: scripts/build-engines.sh, so libkyty_renderer.so links with the five changed objects.
- ctest -R 'gyro_bias|image_memory_pressure' in build-rewrite/kyty (the executables pass when run directly; the ctest registration is untested).
- K1 on real hardware: Switch Pro Controller lying on a table. Expect one log line 'Controller <id>: gyro offset at rest ... deg/s, subtracted' after about 1.3 s and no more drift in gyro aiming. Also check that a DualSense in slow deliberate aiming does not learn a wrong offset.
- K5/K8a under a capped VRAM budget (the existing budget-limit switch) in a heavy scene. Expect 'Vulkan: video memory full, an image is in system memory' at most a few times, then 'TextureCache: video memory full, N MiB wanted, M MiB of unused images freed' with M above 0. Compare hitch length and frame rate with and without the change; watch N for over-asking (finding 2).
- K5 sparse path: BindSparseImageLevels with VRAM full now releases retained images, retries and, with the fallback on, allocates from any allowed memory type instead of ending the process. Needs a run with sparse images in use and VRAM full.
- K8b: no 'Draw dropped:' line may appear in a normal play session.
- K7: run the prepare step (KYTY_PIPELINE_LIST_PREPARE=1) from the adapter on X11 and on native Wayland under `timeout`: no window appears, the run finishes, progress events still arrive; then a normal start shows the window as before.
- K7: start with KYTY_PIPELINE_LIST_PREPARE=1 and no readable param.json: confirm whether the game then runs with a hidden window (finding 3).
- K3: the window or task-bar icon shows the game's icon0.png under X11/XWayland.
- K4: the log shows 'Pipeline list' lines in the embedded run and the prepare run starts (title id from param.json is not empty).

Open points:

- K5 over-asks in a burst of ever-smaller failing images (each adds at least 64 MiB to the request); untuned until measured.
- K5 eviction age of 4 frames is a guess: with VRAM truly full, images used less often than every 4 frames can be freed and re-uploaded repeatedly.
- K8a: during the cooldown, large images allocated relaxed may land in system memory with no log line and no eviction request.
- K7: hidden window with no prepare run when the title id is empty (missing param.json).
- K7 on native Wayland depends on the adapter running the prepare step with present mode Immediate; not verified here.
- K1: the offset log line repeats after every device change, because the estimator is reset in SetDevices.
- K3, K4, K6 were not re-verified line by line in this review.
- engines/kyty/UPSTREAM.md and design.md section 6.3 (rows K1, K5, K8) do not yet describe these changes; neither file is mine.
- scripts/check-repo.sh was not run.

## M6 review (host features)

- Real DualSense over Bluetooth, scripts/play.sh: pad speaker sounds come from the controller and haptics are felt; changing the haptics strength live changes them at once; no deadlock or audio stall after ten minutes.
- Real DualSense over USB, scripts/play.sh: haptics play through the renderer and are not doubled with the sound card, speaker stays on the sound card; APS5_PAD_VIBRATION_VIA_HOST=0 puts haptics back on the sound card; plug and unplug during play causes no deadlock or stall (spec R8).
- Start the game with no pad, then connect a DualSense (and: let a Bluetooth pad sleep and wake): check whether light bar colour and adaptive trigger effects come back without the game changing them. Expected from the code: they do not (finding 1).
- Adaptive triggers in game with APS5_PAD_TRIGGER_EFFECT_STATE=1: resistance is felt and the game reacts to the trigger state.
- Rumble and light bar colour on a DualSense follow the game (F9).
- Motion-controlled section responds to tilting the pad (F11), also on a pad without gyroscope data (quaternion fallback).
- Non-DualSense pad (for example Xbox): haptic audio is approximated by Kyty's rumble and the pad speaker stays in the main mix.
- APS5_PAD_VIBRATION_DUMP=/tmp/haptics.raw scripts/play.sh: the file grows with raw float pairs while haptics play.
- AGS_CONTROLLER_AUDIO_SINK=<exact SDL device name> scripts/play.sh: that device is opened as the controller sound card; with a wrong name none is opened and the renderer path is used.
- Fresh save directory with APS5_SAVEDATA_TRANSFER_AS_MOUNT=1 on game versions 1.018 and 1.019: the first save is created; APS5_TRACE_CALLS shows whether sceSaveDataTransferringMount is called at all.
- scripts/build-engines.sh: full AnyPS5 build from a clean CMake configure, to confirm the commit compiles outside the existing build-rewrite tree.
- Full AnyPS5 ctest run in build-rewrite/anyps5 (only the five M6 tests were run here).

Open points:

- Finding 1: after Kyty switches its active controller (pad plugged in or reconnected after start), light bar colour and trigger effects are not sent again until the game changes them. Needs a change in engines/kyty (controller.cpp SetDevices re-sends, or the seam exposes the active controller id), which this role does not own.
- The classic sceAudioOutOutput path ignores APS5_PAD_VIBRATION_VIA_HOST: when the renderer accepts, a classic vibration port always plays through the renderer, also with a USB sound card open. Matches spec F13, differs from the AudioOut2 switch.
- Classic ports and AudioOut2 share the renderer's one stream per kind; a classic pad port at a rate other than 48000 Hz playing together with AudioOut2 pad audio makes Kyty reopen its stream on every rate change. Unlikely for this game; not tested.
- While the title has pad ports and no sound card, AudioOut2 sends a speaker grain to the renderer on every grain, silent while the renderer refuses. Cheap, and the only way to learn that the renderer started accepting.
- The pad-audio worker thread is never stopped; after shutdown it only gets 0 back from the bridge. Recorded in the debt note.
- The F10 trigger-state model (travel threshold 8, ten equal zones) is not measured on a console (spec R12); behind a switch and in the debt note.

## A5 AvPlayer video notify

- Full build (scripts/build-engines.sh) so the patched libs/libSceAvPlayer.prx and libSceAvPlayer.native.prx are relinked; the patched copies in build-rewrite/anyps5/core/libs/libs are older than this change.
- scripts/play.sh with a frame limit of 30, 120 and off: during the studio intro video the limiter must switch to 60 and video and sound must stay in step; afterwards the limit must return to the selected value.
- Skip a video part-way and confirm the limit returns to the selected value (flag not stuck on).
- Confirm Astro Bot's intro actually goes through libSceAvPlayer frame fetches (videoActive(1) is seen once when the intro starts and videoActive(0) once when it ends), for example by a temporary log in the bridge's video_active.

Open points:

- A player that stays in Play but stops fetching frames (or has its video stream disabled) keeps the 60 FPS lock until stop, pause, end of stream or close. Matches the spec; in the debt note.
- A looping in-game video (for example a menu background) would hold the 60 FPS limit for as long as it plays. This is the specified behaviour, but worth watching in the game.
- If the renderer table is installed while a player is already shown, the renderer is not told until the next edge. Not reachable today: the bridge installs the table before the game starts.
- The error path has no unit test.

## M11 product wiring

- Run scripts/build-engines.sh (full build), then check build/install: bin/astroport-adapter, bin/astroport-run, lib/libastro_patcher.so, presets/u59-preset.json, presets/u59-preset.PPSA21567.json, assets as links (they resolve once the patcher, adapter and runner crates are built).
- After scripts/release/fetch-dlss.sh build/release/dlss-sdk, run scripts/build-engines.sh and confirm build/install/lib/libnvidia-ngx-dlss.so.310.9.16 is a link to the SDK file; delete the SDK folder, run again and confirm the link is gone; then astroport-run build --install build/install copies the runtime into the run folder's libs/.
- After a full build and the three cargo builds: scripts/release/stage.sh <empty dir>, then scripts/release/package.sh <stage> <out>; confirm the summary line and that astroport-run build --install <stage> accepts the tree.
- Confirm that strip --strip-unneeded on libastroport_bridge.so and libkyty_renderer.so (stage.sh) leaves a game that starts: run the staged tree through AGS or scripts/smoke.sh.
- scripts/build-release.sh on a tagged commit: the container build must work without the removed Kyty pre-configure in scripts/release/inside.sh, and with ASTROPORT_REQUIRE_DLSS=1 the tarball must contain lib/libnvidia-ngx-dlss.so.310.9.16 and licenses/NVIDIA-DLSS-SDK-LICENSE.txt.
- AGS installs the tarball and launches the game; every setting in patches/settings.json has a visible effect (design section 8, M11).
- With Ultrawide on in the AGS page, confirm the renamed mod 'Ultrawide: 21:9-class render aspect (2.389; set a 21:9 resolution)' is applied by the patcher.
- With DLSS on in the installed product, check the ten-second summary for evaluations and zero fallbacks and for correct depth handling; if the image shows depth-related artefacts, ASTROPORT_DLSS_REVERSE_DEPTH=1 is missing from patches/settings.json fixed_env.

Open points:

- check-repo.sh fails (M11 exit criterion) on the removed project's names in files outside my ownership: the four docs/superpowers/plans/2026-11-21-astroport-step{2,3,4,5}-*.md, engines/anyps5/UPSTREAM.md, engines/kyty/UPSTREAM.md, patcher/src/lib.rs. Owner decision needed for the old plans (delete, rewrite, or exclude docs/superpowers/); design 7.4 already assigns both UPSTREAM.md files a rewrite.
- check-repo.sh also fails on 'files contain $HOME' for docs/rewrite/spec-anyps5.md (not mine).
- README.md and NOTICE.md still credit and describe the removed project ('hybrid bridge', NOTICE section 3) and do not mention bridge/; they are excluded from the grep, so the check does not show this. Design 7.4 assigns them a rewrite; whether the credit stays is the owner's call.
- patcher/src/lib.rs line 277: dead gate on the old symbol KytyGpuQueuePadAudio. It is inverted, so start-up patches always apply on the new bridge (harmless), but the two lines should go to clear the name grep. Patcher owner.
- patches/settings.json has no ASTROPORT_DLSS_REVERSE_DEPTH although design 7.1 places that title fact there; needs the M8 spike answer on whether the depth is reversed.
- NVIDIA Smooth Motion setting not added: it needs a page item, a default and a test entry in adapter/ before a settings.json entry can exist (adapter owner).
- ags-extension.toml depends does not list cc, which build-engines.sh calls for the forwarder link. I did not add it because I could not check how AGS evaluates depends; cargo needs cc as its linker anyway, so a machine without it fails either way.
- scripts/release/fetch-dlss.sh still requires the SDK header and static library (design 7.4 wants only the runtime and LICENSE.txt); when it shrinks, the 'fetch: fresh' case in scripts/release/test.sh must follow.
- The DLSS redistribution checkbox in docs/release.md is unticked while scripts/release/inside.sh sets ASTROPORT_REQUIRE_DLSS=1 and so ships the runtime (design decision 2 open).
- Containerfile is unverified against the new build; only a container build can confirm it.

## README, NOTICE, UPSTREAM

- README status 'the game starts on the bridge and reaches the title level' rests on the design document and commit log: confirm with RUN_DIR=<data>/generated/run-1019 scripts/play.sh before release.
- README 'Running' commands were not executed: runner/target/release/astroport-run build --dump <dump> --data <data> --install build/install, then RUN_DIR=<data>/generated/run-1019 scripts/play.sh.
- README 'Building': scripts/build-engines.sh was not run against the current working tree (heavy build).

Open points:

- Regenerate section 4 of both UPSTREAM.md files right before committing (command is printed in each file); other steps are still editing engine files, as the AvPlayer addition showed. Add a section 3 line when the DLSS hook lands.
- scripts/check-repo.sh fails on the four docs/superpowers/plans files, which name the removed project about 70 times. They are obsolete plans for the old layout; I did not rewrite or delete them (not created by this step). The owner should delete them or exclude docs/superpowers in check-repo.sh.
- scripts/check-repo.sh also fails on '$HOME' in docs/rewrite/spec-anyps5.md and on the removed names in patcher/src/lib.rs, both outside this step's ownership.
- engines/kyty/src/embed/kytyEmbed.cpp line 171 comment says no upstream export is edited, but VideoOutSubmitFlip in videoOut.cpp is edited for low latency; the comment should be corrected by that file's owner.
- engines/anyps5 libSceAvPlayer Player.cpp calls the renderer's videoActive while holding shownMutex and the player's lock; I did not review whether the bridge side can block or re-enter. Worth a look by the AnyPS5 reviewer.
- NOTICE.md says release packages 'may' carry the DLSS runtime; the redistribution checkbox in docs/release.md is still unticked while scripts/release/inside.sh requires the runtime. The owner's licence decision is open.
- Submodule pins in UPSTREAM.md section 5 were not re-verified against upstream.

## Pad output re-apply, low-latency counters, native session frames

- Full build first: libkyty_renderer.so and kyty_emulator must link with the new streaming/exporter.cpp object and the changed objects (nothing was linked here).
- Pad re-apply: start with no pad, then connect a DualSense; also let a Bluetooth pad sleep and wake. Light bar colour, adaptive trigger effects and any running rumble must come back without the game changing them.
- Pad re-apply with two pads (ASTROPORT_PAD_PREFER=auto): when the second pad takes player 1 it gets the current light bar and trigger effects, and the old pad is stopped; alternating button presses on both pads must not flood either pad.
- Light bar reset: after the game resets the light bar, a device change must not bring the old colour back.
- Low latency with the flip queue full ('FlipsRejected' counter above 0): the ten-second 'LowLatency: N of M driver reports filled' line keeps N > 0 afterwards and frame pacing stays at the cap.
- Session frames under AGS (AGS_SESSION_SOCKET set, AGS_SESSION_PROTOCOL=2): the log shows 'Streaming GPU pool: WxH pitch=... epoch=1 UUID=...', the host accepts the frame hello and shows the picture, including the overlay layer.
- Stream frame rate (new pacing): with the game at 60 fps and the default KYTY_STREAM_FPS, the host receives close to 60 frames per second, evenly; with the game above 60 fps the stream stays at about 60.
- Session frames with the Khronos validation layer once, including KYTY_STREAM_ASPECT=fit with a stream aspect different from the game's: the capture commands (queue-family transfer to and from VK_QUEUE_FAMILY_EXTERNAL, READY/DONE semaphores, clear then blit, composite pass) raise no error.
- Session frames with the window minimized: the stream keeps running.
- Session reconnect (changed behaviour): detach and re-attach the host five or more times. The picture must return each time, 'retired GPU pool limit reached' should not appear, and neither the host nor the driver may report an error when the old pool is freed.
- Host stops or crashes while frames are streaming: the game must keep presenting (a slot's DONE semaphore that is never signalled would stall the graphics queue).
- Exit while a host is attached: clean shutdown, no hang in the presenter's teardown.
- Metrics-only fallback: with KYTY_STREAM_FRAMES=0, and with a host that refuses frames ('frame hello refused, falling back to frame-less'), metrics and settings.apply still work and no pool is created.
- Session settings and metrics before the first game frame is shown: check whether AGS needs them during loading (in frames mode the hello is sent only once the pool exists).
- Device creation with AGS_SESSION_SOCKET set: no 'Could not create device' after the two FD extensions are added (relevant together with the DLSS/NGX extension list).
- KYTY_STREAM_WIDTH=1280 KYTY_STREAM_HEIGHT=720 and KYTY_STREAM_ASPECT=fit|fill|stretch give the expected stream size and bars.
- HDR under an AGS session: the game reports SDR (IsHdrSupported is false while frames are offered) and no 'HDR frame cannot be exported' line appears.

Open points:

- docs/rewrite/pending-verification.md, design.md and engines/kyty/UPSTREAM.md are not my files: the three points are still listed as open there, and the KYTY_STREAM_* variables, Transport::Session/SetCompletion, the graphicContext field and the PadResetLightBar change are not described.
- Retired frame pools are now freed at the next connection once their last capture has completed. This assumes the host's imported memory and semaphores stay valid after our side destroys its handles; verified by reading only, needs the reconnect test.
- A recycled slot's capture waits on the host's DONE semaphore on the game's graphics queue. A host that sends 'release' without signalling DONE, or dies in between, would stall presentation. Inherent in the protocol; not changed.
- The frame pool does not follow a live display.resolution or display.fps setting (marked 'shortcut:' in swapchain.cpp); its size is fixed per connection.
- In frames mode the hello is sent only once the pool exists, which needs the first presented game frame; until then the session has no metrics and no settings.apply.
- Presenter::IsHdrSupported is false whenever a session socket offers frames, and also before the first present when the FD extensions turn out to be missing, so a game that asks once at start may stay SDR in that case.
- In the stream the overlay layer is drawn over the whole picture, not inside the fitted game rectangle as in the window; they differ only when the stream's aspect differs from the game's. A 16-bit float overlay frame is not flagged to the overlay shader in the stream.
- At process teardown the transport worker could still send a hello with pool descriptors that are being closed (Reconnect is only a flag). Exit path only; not changed.
- The low-latency fix spends a frame number for every failed GPU flip reservation. A flip packet that is written but never submitted, or a flip on a port other than the main one, can still shift the counters; not checked for this title.
- SetDevices keeps m_state_effect (what scePadGetTriggerEffectState reports) across a device change; upstream cleared it.
- A rumble request is re-applied with its remaining time only (about 65 s limit); existing behaviour.
- No unit test for the pad re-apply, the counter fix or the new capture pacing: tests/ is outside my files and all three need SDL or Vulkan state.
- The ninja calls re-ran the CMake glob check in build-rewrite/kyty and regenerated kytyGitVersion.h.
