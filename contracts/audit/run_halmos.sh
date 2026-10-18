#!/usr/bin/env bash
# Sequential Halmos runs (ONE job at a time, one solver thread). Logs in audit/logs/halmos_<name>.log
# Usage: audit/run_halmos.sh <Contract> [--function prefix] [extra halmos args...]
set -uo pipefail
cd "$(dirname "$0")/.."
export FOUNDRY_OUT=out-halmos FOUNDRY_CACHE_PATH=cache-halmos
c=$1; shift
tag=$c
for a in "$@"; do case $a in check_*) tag=${c}_$a;; esac; done
./audit/gen_harnesses.sh >/dev/null
{ time .venv/bin/halmos --contract "$c" --forge-build-out out-halmos --solver-threads 1 "$@" ; } > "audit/logs/halmos_${tag}.log" 2>&1
grep -E "\[(PASS|FAIL|TIMEOUT|ERROR)\]|Symbolic test result|real|total" "audit/logs/halmos_${tag}.log"
