#!/bin/sh
# Regenerates tests/fixtures/rxnorm-mini from an unpacked RxNorm Current Prescribable
# Content release. Rows are copied verbatim; nothing is edited.
#
#   scripts/make-fixture.sh path/to/unpacked/RxNorm_full_prescribe_MMDDYYYY
set -eu
SRC=$1
OUT=$(dirname "$0")/../tests/fixtures/rxnorm-mini
SEEDS=$(mktemp)
trap 'rm -f "$SEEDS"' EXIT
# metformin, Glucophage, amoxicillin/clavulanate, Augmentin, acetaminophen, aspirin,
# and the components, forms and groups linking them.
printf '%s\n' 161 723 1191 6809 19711 48203 151392 151827 235743 317541 562508 824194 \
  861007 861006 329066 345821 368254 372803 860974 860976 1151131 1151133 1161610 2646570 >"$SEEDS"
mkdir -p "$OUT/rrf"
awk -F'|' 'NR==FNR{s[$1]=1;next} ($1 in s) && ($12=="RXNORM" || ($12=="MTHSPL" && $13=="SU"))' \
  "$SEEDS" "$SRC/rrf/RXNCONSO.RRF" >"$OUT/rrf/RXNCONSO.RRF"
awk -F'|' '$1=="861007" && $12=="MTHSPL" && $13=="DP"' "$SRC/rrf/RXNCONSO.RRF" | head -2 >>"$OUT/rrf/RXNCONSO.RRF"
awk -F'|' 'NR==FNR{s[$1]=1;next} ($1 in s) && ($5 in s) && $11=="RXNORM"' \
  "$SEEDS" "$SRC/rrf/RXNREL.RRF" >"$OUT/rrf/RXNREL.RRF"
awk -F'|' 'NR==FNR{s[$1]=1;next} ($1 in s) && $10=="RXNORM"' \
  "$SEEDS" "$SRC/rrf/RXNSAT.RRF" >"$OUT/rrf/RXNSAT.RRF"
awk -F'|' '$1=="861007" && $10=="MTHSPL"' "$SRC/rrf/RXNSAT.RRF" | head -3 >>"$OUT/rrf/RXNSAT.RRF"
README=$(ls "$SRC"/Readme_Full_Prescribe_*.txt)
head -12 "$README" >"$OUT/$(basename "$README")"
