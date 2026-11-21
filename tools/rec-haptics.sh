#!/bin/bash
# rec-haptics.sh <name> [seconds]: record every audio stream that plays on the DualSense (raw float,
# its own channel layout) for a few seconds, for comparing the haptic signal of two runs.
OUT="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")/.." && pwd)/haptics-rec"; mkdir -p "$OUT"; S=${2:-30}
SINK=$(pactl list short sinks | awk '/DualSense/{print $1; exit}')
pactl list short sink-inputs | awk -v s="$SINK" '$2==s{print $1, $6}' | while read -r id ch; do
  n=${ch%ch}
  case $n in 4) map=front-left,front-right,rear-left,rear-right ;;
             6) map=front-left,front-right,front-center,lfe,rear-left,rear-right ;;
             *) map=front-left,front-right ;; esac
  timeout "$S" parec --monitor-stream="$id" --format=float32le --channels="$n" --channel-map="$map" \
    --rate=48000 --raw > "$OUT/$1-stream$id-${n}ch.raw" &
done
timeout "$S" parec -d "$(pactl list short sinks | awk '/DualSense/{print $2; exit}').monitor" --format=float32le \
  --channels=4 --channel-map=front-left,front-right,rear-left,rear-right --rate=48000 --raw > "$OUT/$1-device-4ch.raw" &
wait; ls -la "$OUT" | grep "$1"
