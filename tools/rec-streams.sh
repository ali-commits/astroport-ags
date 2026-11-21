#!/bin/bash
# rec-streams.sh <dir> [seconds]: for <seconds>, record every audio stream that plays on the DualSense
# USB sound card, each to its own file <dir>/<time>-<id>-<app>-<N>ch.f32 (raw float, own layout).
DIR=$1; END=$(( $(date +%s) + ${2:-1200} )); mkdir -p "$DIR"; declare -A seen
while [ "$(date +%s)" -lt "$END" ]; do
  SINK=$(pactl list short sinks | awk '/DualSense/{print $1; exit}')
  if [ -n "$SINK" ]; then
    while read -r id ch; do
      [ -n "${seen[$id]}" ] && continue; seen[$id]=1; n=${ch%ch}
      app=$(pactl list sink-inputs | awk -v id="Sink Input #$id" '$0==id{f=1} f&&/application.name/{gsub(/[" ]/,"",$3); print $3; exit}')
      case $n in 4) map=front-left,front-right,rear-left,rear-right ;;
                 6) map=front-left,front-right,front-center,lfe,rear-left,rear-right ;;
                 *) map=front-left,front-right ;; esac
      echo "$(date +%T) stream $id $app ${n}ch"
      timeout $(( END - $(date +%s) )) parec --monitor-stream="$id" --format=float32le --channels="$n" \
        --channel-map="$map" --rate=48000 --raw > "$DIR/$(date +%H%M%S)-$id-${app:-x}-${n}ch.f32" &
    done < <(pactl list short sink-inputs | awk -v s="$SINK" '$2==s{print $1, $6}')
  fi
  sleep 2
done
wait
