#!/bin/bash
# Astro Bot, native Linux PC port: AnyPS5 runs the relinked game, Kyty's renderer draws it, our
# bridge (libastroport_bridge.so) joins the two. Developer launch script; the product launches
# through the adapter with the same variables (patches/settings.json).
#   RUN_DIR=<path>        the run folder (required; built by astroport-run from your own dump)
#   FPS_LIMIT=30|60|...   game frame cap (default 30, for frame generation 30 -> 60)
#   LEVEL=<name>          start in a level (e.g. underwater_aerial_garden), for tests
#   VERSION=1019          game version (patches/PPSA21567-<VERSION>.json)
#   CHEATS=0              without the game patches (patcher)
#   EXE=AstroBotTrace     the build that counts system-library calls (APS5_TRACE_CALLS, APS5_TRACE_CALLS_FILE)
#   HAPTICS=2.5           controller haptic strength (1.0 = as the game sends it, max 4)
#   ULTRAWIDE=1           21:9 render aspect (patch "Ultrawide"); resolution defaults to 3440x1440
#   RAY_TRACING=1         the game's original lighting: leaves out the faster-lighting patches
#   TITLE_PRESET=0        without int22's Astro Bot preset; PRESET=0 without any speed preset
#   STATIC_TLS=0          without the glibc static TLS tunable (starts only with CHEATS=0)
#   LOG=<file>|-          log file (default <run folder>/last.log); "-" leaves the output alone
# ASTROPORT_* variables that are already set win over the defaults here (scripts/smoke.sh sets
# ASTROPORT_FULLSCREEN=0).
N="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)"   # repository root
VERSION=${VERSION:-1019}
R="$(realpath "${RUN_DIR:?RUN_DIR must name the run folder (run-<version>)}")"
cd "$R" || exit 1

# Room for the renderer's static thread-local block (about 600 KB). Needed with LD_AUDIT (the
# patcher): glibc then sizes the static TLS area before it loads the program's libraries.
[ "${STATIC_TLS:-1}" = 1 ] && export GLIBC_TUNABLES="${GLIBC_TUNABLES:+$GLIBC_TUNABLES:}glibc.rtld.optional_static_tls=0x200000"

PRESETS="$N/engines/kyty/tools/u59-preset.json"
[ "${TITLE_PRESET:-1}" = 1 ] && PRESETS="$PRESETS $N/engines/kyty/tools/u59-preset.PPSA21567.json"
[ "${PRESET:-1}" = 0 ] && PRESETS=
# Kyty's speed preset (u59-preset.json), as u59-preset.cmd sets it on Windows
preset=$(python3 -I -c '
import json, shlex, sys
for path in sys.argv[1:]:
    for k, v in json.load(open(path)).items():
        if not k.startswith("_") and isinstance(v, str): print("export %s=%s" % (k, shlex.quote(v)))
' $PRESETS) || { echo "play: cannot read the presets ($PRESETS)" >&2; exit 1; }
eval "$preset"

# Kyty and AnyPS5 switches (upstream names)
export KYTY_HANG_WATCHDOG=${KYTY_HANG_WATCHDOG:-0} KYTY_CP_PRIORITY=2
export KYTY_DRAW_PREP_SPIN_US=10 KYTY_DRAW_PREP_COLD_SPIN_US=5
export KYTY_AGC_DONE_MODE=${AGC_DONE_MODE:-bounded}  # "idle" deadlocked in GPU readbacks
export KYTY_BUFFER_RANGE_FIXES=${BUFFER_RANGE_FIXES:-1} KYTY_PRESENT_ABS_VBLANK=1
export APS5_PAD_TRIGGER_EFFECT_STATE=1 APS5_SAVEDATA_TRANSFER_AS_MOUNT=1
export APS5_PAD_VIBRATION_GAIN=${HAPTICS:-2.5}       # haptic strength (1.0 = as the game sends it, up to 4)
export ANYPS5_LANGUAGE=${LANGUAGE:-1}                # 1 = English (United States), as Kyty

# The bridge's settings (docs/rewrite/design.md 7.1)
if [ "${ULTRAWIDE:-0}" = 1 ]; then WIDTH=${WIDTH:-3440} HEIGHT=${HEIGHT:-1440}; fi
export ASTROPORT_APP0="$R/app0" ASTROPORT_WINDOW_TITLE="${ASTROPORT_WINDOW_TITLE:-Astro Bot}"
export ASTROPORT_FPS_LIMIT=${ASTROPORT_FPS_LIMIT:-${FPS_LIMIT:-30}}
export ASTROPORT_WIDTH=${ASTROPORT_WIDTH:-${WIDTH:-3840}} ASTROPORT_HEIGHT=${ASTROPORT_HEIGHT:-${HEIGHT:-2160}}
export ASTROPORT_FULLSCREEN=${ASTROPORT_FULLSCREEN:-${FULLSCREEN:-1}}
export ASTROPORT_LOW_LATENCY=${ASTROPORT_LOW_LATENCY:-${LOW_LATENCY:-1}}   # NVIDIA low-latency mode
export ASTROPORT_PAD_PREFER=auto ASTROPORT_RUMBLE_SCALE=${RUMBLE:-100} ASTROPORT_RUMBLE_SWITCH_FLOOR=96
# DualSense rumble reports (DUALSENSE_RUMBLE=0: none; they switch the pad to rumble emulation,
# which silences audio haptics).
export ASTROPORT_DS_RUMBLE=${DUALSENSE_RUMBLE:-1}
export ASTROPORT_WAYLAND_NATIVE=${ASTROPORT_WAYLAND_NATIVE:-1}             # true 4K surface under KDE scaling
export ASTROPORT_DLSS_PASS=2936e5eca99d75e7                                # the game's upscale pass
# Where the pass keeps its inputs (patches/settings.json; confirmed by the 'DLSS pass: buffer 0' line)
export ASTROPORT_DLSS_LAYOUT=${ASTROPORT_DLSS_LAYOUT:-0,1,4,5,6,7,11,13,0,1,0}

# The game patches, applied in memory before the game starts (frame timing fixes, 30/60 FPS cap,
# 1080p render, lower detail options). The precise frame cap there and ASTROPORT_FPS_LIMIT agree.
# Without a patch file the game runs unpatched.
PATCH_FILE="$N/patches/PPSA21567-$VERSION.json"
if [ "${CHEATS:-1}" = 1 ] && [ -f "${ASTRO_PATCHES:-$PATCH_FILE}" ]; then
  export LD_AUDIT="$N/patcher/target/release/libastro_patcher.so"
  export ASTRO_PATCHES="${ASTRO_PATCHES:-$PATCH_FILE}"
  [ "${ULTRAWIDE:-0}" = 1 ] && export ASTRO_PATCH_ENABLE="${ASTRO_PATCH_ENABLE:+$ASTRO_PATCH_ENABLE,}ultrawide"
  # The game's own cap follows the limit, as display.fps does in patches/settings.json: the 30 FPS cap
  # and the 30 fps physics fixes ("... at 30 fps") give way to the cap patch of that rate.
  if [ "$ASTROPORT_FPS_LIMIT" != 30 ]; then
    export ASTRO_PATCH_ENABLE="${ASTRO_PATCH_ENABLE:+$ASTRO_PATCH_ENABLE,}Precise frame cap $ASTROPORT_FPS_LIMIT FPS"
    export ASTRO_PATCH_SKIP="${ASTRO_PATCH_SKIP:+$ASTRO_PATCH_SKIP,}Precise frame cap 30 FPS,at 30 fps"
  fi
  [ "${RAY_TRACING:-0}" = 1 ] && export ASTRO_PATCH_SKIP="${ASTRO_PATCH_SKIP:+$ASTRO_PATCH_SKIP,}non-tiled deferred-lighting,disable gi probes"
fi

# Start in a level (tests): a private args file, the dump's own stays untouched.
# A dump without args.txt gets one only for LEVEL=.
if [ -f "$R/app0/args.txt" ] || [ -f "$R/app0/args.txt.dump" ]; then
  [ -f "$R/app0/args.txt.dump" ] || cp "$R/app0/args.txt" "$R/app0/args.txt.dump"
  if [ -n "$LEVEL" ]; then
    printf '%s -lvl %s\n' "$(tr -d '\r\n' < "$R/app0/args.txt.dump")" "$LEVEL" > "$R/app0/args.txt"
  else
    cp "$R/app0/args.txt.dump" "$R/app0/args.txt"
  fi
elif [ -n "$LEVEL" ]; then
  echo "play: LEVEL needs $R/app0/args.txt, which this dump does not have" >&2; exit 1
fi

export ASTRO_PATCH_PROCESS=${EXE:-AstroBot}
[ "${LOG:=$R/last.log}" = - ] || exec > "$LOG" 2>&1
exec ./${EXE:-AstroBot} "$@"
