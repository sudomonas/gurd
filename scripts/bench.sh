#!/bin/sh
# Times lookups against a real database: scripts/bench.sh path/to/drug.db [runs]
set -eu
DB=$1
RUNS=${2:-20}
BIN=$(dirname "$0")/../target/release/drug
[ -x "$BIN" ] || cargo build --release
for q in metformin METFORMIN "metformin hydrochloride" augmentin "amoxicillin clavulanate" \
         metfor tformi metfromin acetaminophen a zzqqxx; do
  start=$(date +%s%N)
  i=0
  while [ "$i" -lt "$RUNS" ]; do
    "$BIN" --db "$DB" "$q" >/dev/null 2>&1 || true
    i=$((i + 1))
  done
  end=$(date +%s%N)
  us=$(( (end - start) / RUNS / 1000 ))
  printf "%-26s %4d.%d ms\n" "$q" $((us / 1000)) $((us % 1000 / 100))
done
