#!/bin/bash
# rec-device.sh <out.f32> [max seconds]: wait for the DualSense USB sound card, then record everything
# it plays (4 channels: speaker L/R, haptics L/R as rear) until stopped or max seconds.
OUT=$1; MAX=${2:-900}
until pactl list short sinks | grep -q DualSense; do sleep 1; done
SINK=$(pactl list short sinks | awk '/DualSense/{print $2; exit}')
echo "recording $SINK -> $OUT ($(date +%T))"
exec timeout "$MAX" parec -d "$SINK.monitor" --format=float32le --channels=4 \
  --channel-map=front-left,front-right,rear-left,rear-right --rate=48000 --raw > "$OUT"
