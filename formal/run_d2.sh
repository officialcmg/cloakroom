#!/usr/bin/env bash
# Runs the D=F=2 TLC configs ONE AT A TIME (2 workers, 2 GB heap). Logs in logs/d2_*.log
cd "$(dirname "$0")"
J=$(brew --prefix openjdk)/bin/java
for c in "$@"; do
  { time $J -Xmx2g -XX:+UseParallelGC -cp tla2tools.jar tlc2.TLC -deadlock -config "$c.cfg" -workers 2 -metadir "states/$c" Cloakroom.tla ; } > "logs/$c.log" 2>&1
  echo "== $c"; grep -E 'Invariant .* is violated|Model checking completed|distinct states found|depth of the complete|real' "logs/$c.log"
  rm -rf "states/$c"
done
