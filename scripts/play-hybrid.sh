#!/bin/bash
# Astro Bot, native Linux PC port: AnyPS5 runs the relinked game, Kyty's renderer (libkyty_gpu.so,
# AstroHybrid's bridge built for Linux) draws it. Same settings the AstroHybrid launcher passes.
#   RUN_DIR=<path>        the run folder (required; built from your own dump)
#   FPS_LIMIT=30|60|...   game frame cap (default 30, for frame generation 30 -> 60)
#   LEVEL=<name>          start in a level (e.g. underwater_aerial_garden), for tests
#   VERSION=1019          game version (patches/PPSA21567-<VERSION>.json)
#   CHEATS=0              without the cheat patches (astro-patcher)
#   EXE=AstroBotTrace     the build that counts system-library calls (APS5_TRACE_CALLS, APS5_TRACE_CALLS_FILE)
#   HAPTICS=2.5           controller haptic strength (1.0 = as the game sends it, max 4)
#   ULTRAWIDE=1           21:9 render aspect (cheat "Ultrawide"); resolution defaults to 3440x1440
#   RAY_TRACING=1         the game's original lighting: leaves out the faster-lighting cheats
#   KYTY_SO=<path>        any other build of libkyty_gpu.so
#   TITLE_PRESET=0        without int22's Astro Bot preset; PRESET=0 without any speed preset
N="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"   # repository root
VERSION=${VERSION:-1019}
R="${RUN_DIR:?RUN_DIR must name the run folder (run-hybrid-<version>)}"
cd "$R" || exit 1

# libkyty_gpu.so has ~580 KB of initial-exec thread-local storage (Kyty's thread_local counters);
# glibc must reserve room for it, since AnyPS5 opens the library after startup.
export GLIBC_TUNABLES="${GLIBC_TUNABLES:+$GLIBC_TUNABLES:}glibc.rtld.optional_static_tls=0x200000"

# AnyPS5 + Kyty bridge (Run-AstroHybrid.bat)
export APS5_GPU_ENGINE=kyty
KYTY_LIB="$R/libs/libkyty_gpu.so"; PRESETS="$N/engines/kyty/tools/u59-preset.json"
[ "${TITLE_PRESET:-1}" = 1 ] && PRESETS="$PRESETS $N/engines/kyty/tools/u59-preset.PPSA21567.json"
[ -n "${KYTY_SO:-}" ] && KYTY_LIB="$KYTY_SO"   # any other renderer build
# The AGC/VideoOut modules link libkyty_gpu.so by name (RUNPATH libs/); LD_LIBRARY_PATH is searched first, so
# they load the chosen build too. Not LD_PRELOAD: its symbols would then override AnyPS5's own in every module.
[ "$KYTY_LIB" != "$R/libs/libkyty_gpu.so" ] && export LD_LIBRARY_PATH="$(dirname "$KYTY_LIB")${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
[ "${PRESET:-1}" = 0 ] && PRESETS=

export KYTY_GPU_LIBRARY="$KYTY_LIB" KYTY_GPU_HOST_MODULE="$R/libs/libAstroHybridHost.prx"
export KYTY_HANG_WATCHDOG=${KYTY_HANG_WATCHDOG:-0} ASTROHYBRID_CACHE_TITLE_ID=PPSA21567
export APS5_PAD_TRIGGER_STATE=1 ASTROHYBRID_PAD_MOTION=1 KYTY_CP_PRIORITY=2
export KYTY_DRAW_PREP_PRIORITY=2 KYTY_DRAW_PREP_SPIN_US=10 KYTY_DRAW_PREP_COLD_SPIN_US=5

# Kyty's speed preset (u59-preset.json), as u59-preset.cmd sets it on Windows
eval "$(python3 -I -c '
import json, shlex, sys
for path in sys.argv[1:]:
    for k, v in json.load(open(path)).items():
        if not k.startswith("_") and isinstance(v, str): print("export %s=%s" % (k, shlex.quote(v)))
' $PRESETS)"

# Launcher settings (LauncherService.BuildEnvironmentVariables)
export ASTROHYBRID_FPS_LIMIT=${FPS_LIMIT:-30}
if [ "${ULTRAWIDE:-0}" = 1 ]; then WIDTH=${WIDTH:-3440} HEIGHT=${HEIGHT:-1440}; fi
export KYTY_GPU_WIDTH=${WIDTH:-3840} KYTY_GPU_HEIGHT=${HEIGHT:-2160} KYTY_GPU_FULLSCREEN=${FULLSCREEN:-1}
export ASTROHYBRID_LOW_LATENCY=${LOW_LATENCY:-1}          # NVIDIA Reflex (VK_NV_low_latency2)
# Lowest latency (Sky Garden, measured: 152 -> ~80 ms from frame start to GPU end, 30 FPS kept):
# Reflex paces the frames at the cap, before the game reads input, and the game runs at most one
# frame ahead of the renderer.
export ASTROHYBRID_LOW_LATENCY_INTERVAL_US=${LATENCY_INTERVAL_US:-$(( 1000000 / ${FPS_LIMIT:-30} ))}
export KYTY_AGC_DONE_MODE=${AGC_DONE_MODE:-bounded}  # "idle" deadlocked in GPU readbacks (freeze-stacks-20261115.txt)
export ASTROHYBRID_PAD_PREFER=auto ASTROHYBRID_RUMBLE_SCALE=${RUMBLE:-100} ASTROHYBRID_RUMBLE_SWITCH_FLOOR=96
export APS5_PAD_VIBRATION_GAIN=${HAPTICS:-2.5}                 # haptic strength (1.0 = as the game sends it, up to 4)
# DualSense rumble reports (DUALSENSE_RUMBLE=0: none; they switch the pad to rumble emulation,
# which silences audio haptics).
export ASTROHYBRID_DUALSENSE_RUMBLE=${DUALSENSE_RUMBLE:-1}
export ASTROHYBRID_WINDOW_TITLE="Astro Bot" ANYPS5_LANGUAGE=${LANGUAGE:-1}  # 1 = English (United States), as Kyty
# AstroHybrid 1.0.9 (#30, #35): with the commit-epoch fast path on and the buffer-range fixes off,
# cloth, blankets and underwater bubbles turn into stray polygons.
export ASTROHYBRID_COMMIT_EPOCH_FASTPATH=${COMMIT_EPOCH_FASTPATH:-0} KYTY_BUFFER_RANGE_FIXES=${BUFFER_RANGE_FIXES:-1}
# AstroHybrid 1.0.9 (#27): builds that mount their first save through sceSaveDataTransferringMount
# get a regular save instead of "not found".
export APS5_SAVEDATA_TRANSFER_FALLBACK=1
export KYTY_WAYLAND_NATIVE=1                                # true 4K surface under KDE scaling

# The user's KytyPS5 cheat file, applied in memory before the game starts (frame timing fixes,
# 30/60 FPS cap, 1080p render, lower detail options). The precise frame cap there and
# ASTROHYBRID_FPS_LIMIT agree. The file is KytyPS5's 1.018 cheats ported with tools/port_patches.py
# (patches/PPSA21567-<VERSION>.json); without one the game runs unpatched.
CHEAT_FILE="$N/patches/PPSA21567-$VERSION.json"
if [ "${CHEATS:-1}" = 1 ] && [ -f "${ASTRO_PATCHES:-$CHEAT_FILE}" ]; then
  export LD_AUDIT="$N/patcher/target/release/libastro_patcher.so"
  export ASTRO_PATCHES="${ASTRO_PATCHES:-$CHEAT_FILE}"
  [ "${ULTRAWIDE:-0}" = 1 ] && export ASTRO_PATCH_ENABLE="${ASTRO_PATCH_ENABLE:+$ASTRO_PATCH_ENABLE,}ultrawide"
  [ "${RAY_TRACING:-0}" = 1 ] && export ASTRO_PATCH_SKIP="${ASTRO_PATCH_SKIP:+$ASTRO_PATCH_SKIP,}non-tiled deferred-lighting,disable gi probes"
fi

# Start in a level (tests): a private args file, the dump's own stays untouched.
if [ -n "$LEVEL" ]; then
  printf '%s -lvl %s\n' "$(tr -d '\r\n' < "$R/app0/args.txt.dump")" "$LEVEL" > "$R/app0/args.txt"
else
  cp "$R/app0/args.txt.dump" "$R/app0/args.txt"
fi

export ASTRO_PATCH_PROCESS=${EXE:-AstroBot}
exec ./${EXE:-AstroBot} "$@" > "$R/last-hybrid.log" 2>&1
