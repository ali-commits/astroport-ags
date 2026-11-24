#!/bin/bash
# Smoke run: starts a command (the game, windowed) under `timeout` with DISABLE_LSFGVK=1, writes its
# output to one log and checks that every pattern occurs in it (design.md 1.2, section 8).
#
#   scripts/smoke.sh [-t seconds] [-l log] [-d dir] -p pattern [-p pattern ...] [-- command ...]
#
#   -t  time limit in seconds (default 120); the command is killed when it is reached
#   -l  log file (default: smoke.log in the working directory of the run)
#   -d  directory to run in, normally the run folder (default: current directory)
#   -p  extended regular expression that must occur in the log; repeatable
#   command  default: ./AstroBot
#
# Exit code: 0 all patterns found, 1 a pattern is missing, 2 usage error.
# The command's own exit code is printed, not judged: a run that is killed by the time limit after
# it printed every pattern passes.
set -u

usage() {
    sed -n '2,13s/^# \{0,1\}//p' "$0" >&2
    exit 2
}

limit=120
log=smoke.log
dir=.
patterns=()
while getopts t:l:d:p: option; do
    case $option in
        t) limit=$OPTARG ;;
        l) log=$OPTARG ;;
        d) dir=$OPTARG ;;
        p) patterns+=("$OPTARG") ;;
        *) usage ;;
    esac
done
shift $((OPTIND - 1))
[[ $limit =~ ^[1-9][0-9]*$ ]] || { echo "smoke: -t needs a number of seconds, got '$limit'" >&2; usage; }
[ ${#patterns[@]} -gt 0 ] || { echo "smoke: at least one -p pattern is required" >&2; usage; }
[ $# -gt 0 ] || set -- ./AstroBot

cd "$dir" || exit 2
# ASTROPORT_FULLSCREEN=0: windowed. DISABLE_LSFGVK=1: lsfg-vk would double frame counts (risk 23).
# SIGTERM at the limit, SIGKILL five seconds later.
ASTROPORT_FULLSCREEN=0 DISABLE_LSFGVK=1 timeout -k 5 "$limit" "$@" > "$log" 2>&1
code=$?
case $code in
    124 | 137) echo "smoke: stopped at the time limit of $limit s" ;;
    126 | 127) echo "smoke: cannot run '$1'" ;;
    *) echo "smoke: command exited with code $code" ;;
esac

missing=0
for pattern in "${patterns[@]}"; do
    if grep -Eq -- "$pattern" "$log"; then
        echo "smoke: found    $pattern"
    else
        echo "smoke: MISSING  $pattern"
        missing=1
    fi
done
[ $missing = 0 ] || echo "smoke: log is $(realpath "$log")" >&2
exit $missing
