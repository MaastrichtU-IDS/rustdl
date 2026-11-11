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
for f in "$@"; do
  echo "==> $f"
  if "$JAVA" -Xmx8g -Dfile.encoding=UTF-8 -cp "$JAR" \
      org.semanticweb.HermiT.cli.CommandLine -c -o "$OUT/$f-classified.ofn" "$IN/$f.ofn" \
      > "$OUT/$f.log" 2>&1; then
    echo "   ok ($(wc -l < "$OUT/$f-classified.ofn") lines)"
  else
    echo "   HERMIT FAILED:"; tail -3 "$OUT/$f.log"
  fi
done
