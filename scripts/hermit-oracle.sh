#!/usr/bin/env bash
# Generate HermiT classification oracles for the per-query FP gate (#505).
#
# Writes ontologies/real/hermit/<f>-classified.ofn: HermiT's `-c` output in
# functional syntax, one axiom per line, with direct `SubClassOf` edges and
# `EquivalentClasses` groups. The unsatisfiable classes form the group that
# contains owl:Nothing. The Konclude oracles come from scripts/konclude-oracle.sh.
#
# Needs HermiT's CommandLine on a classpath, e.g. robot.jar:
#   ROBOT_JAR=/path/to/robot.jar [JAVA=/path/to/java] scripts/hermit-oracle.sh [f ...]
# Default fixtures: pizza ro sulo wine. ro takes about 5 minutes.
set -uo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IN="$REPO_ROOT/ontologies/real"
OUT="$IN/hermit"
JAR="${ROBOT_JAR:?set ROBOT_JAR to a jar with org.semanticweb.HermiT.cli.CommandLine}"
JAVA="${JAVA:-java}"
mkdir -p "$OUT"
[[ $# -eq 0 ]] && set -- pizza ro sulo wine
fail=0
for f in "$@"; do
  echo "==> $f"
  tmp="$OUT/.$f-classified.ofn.tmp"
  if "$JAVA" -Xmx8g -Dfile.encoding=UTF-8 -cp "$JAR" \
      org.semanticweb.HermiT.cli.CommandLine -c -o "$tmp" "$IN/$f.ofn" \
      > "$OUT/$f.log" 2>&1 && [[ -s "$tmp" ]]; then
    # Replace the oracle only on success, so a failed run never leaves a partial one.
    mv "$tmp" "$OUT/$f-classified.ofn"
    echo "   ok ($(wc -l < "$OUT/$f-classified.ofn") lines)"
  else
    rm -f "$tmp"; fail=1
    echo "   HERMIT FAILED (oracle left unchanged):"; tail -3 "$OUT/$f.log"
  fi
done
exit "$fail"
