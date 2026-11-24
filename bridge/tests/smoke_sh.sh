#!/bin/bash
# Checks scripts/smoke.sh with dummy commands: argument handling, pattern result, time limit.
# usage: bridge/tests/smoke_sh.sh
set -u
smoke=$(cd "$(dirname "$0")/../../scripts" && pwd)/smoke.sh
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failed=0

# expect <exit code> <text in output> <smoke.sh arguments...>
expect() {
    local want=$1 text=$2 out code
    shift 2
    out=$("$smoke" -l "$work/log" "$@" 2>&1)
    code=$?
    if [ "$code" != "$want" ] || ! grep -Fq -- "$text" <<< "$out"; then
        echo "FAIL (exit $code, want $want; want text '$text'): smoke.sh $*"
        echo "$out"
        failed=1
    fi
}

expect 2 "at least one -p" -- true
expect 2 "needs a number" -t soon -p x -- true
expect 2 "smoke.sh [-t seconds]" -x
expect 0 "found    ready in [0-9]+ ms" -p 'ready in [0-9]+ ms' -p donated -- sh -c 'echo donated; echo "ready in 12 ms" >&2'
expect 1 "MISSING  adopted" -p donated -p adopted -- echo donated
expect 0 "exited with code 3" -p ok -- sh -c 'echo ok; exit 3'
expect 1 "cannot run" -p ok -- "$work/no-such-program"
expect 0 'found    ^1 0$' -p '^1 0$' -- sh -c 'echo "$DISABLE_LSFGVK $ASTROPORT_FULLSCREEN"'
expect 0 "found    ^$work" -d "$work" -p "^$work\$" -- pwd

start=$SECONDS
expect 0 "stopped at the time limit of 1 s" -t 1 -p started -- sh -c 'echo started; sleep 30'
expect 1 "stopped at the time limit of 1 s" -t 1 -p never -- sh -c 'trap "" TERM; while :; do sleep 1; done'
[ $((SECONDS - start)) -le 12 ] || { echo "FAIL: time limit took $((SECONDS - start)) s"; failed=1; }

[ $failed = 0 ] && echo "smoke.sh: all checks passed"
exit $failed
