#!/usr/bin/env bash
# Phase 2b.0 Task 3: Build per-pair minimal HermiT-verified GALEN repros.
# Produces 3 files per pair: pair_NN.owx, pair_NN.ofn, pair_NN.hermit.owx
# under crates/owl-dl-reasoner/tests/fixtures/phase2b/
#
# IMPORTANT: classify-oracle.sh uses ROBOT reason with --axiom-generators "subclass"
# which only produces DIRECT inferred SubClassOf edges. Verification must use
# transitive closure over the output to detect indirect subsumptions.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURES_DIR="$REPO_ROOT/crates/owl-dl-reasoner/tests/fixtures/phase2b"
RUSTDL="$REPO_ROOT/target/release/rustdl"
CLASSIFY_ORACLE="$REPO_ROOT/docker/robot/classify-oracle.sh"
ROBOT_IMAGE="obolibrary/robot:v1.9.6"
LOG_FILE="$FIXTURES_DIR/phase2b-verdicts.log"

mkdir -p "$FIXTURES_DIR"
: > "$LOG_FILE"

BASE_NS="http://example.org/factkb#"

declare -a PAIRS=(
  "01|FemoralHead|ExactlyPairedBodyStructure|A"
  "02|HeadOfHumerus|MirrorImagedBodyStructure|A"
  "03|MeniscusOfKneeJoint|ExactlyPairedBodyStructure|A"
  "04|KneeJointRecessus|HollowStructure|B"
  "05|SupraPatellarPouch|ActuallyHollowBodyStructure|B"
  "06|CongestiveCardiacFailure|IntrinsicallyPathologicalBodyProcess|C"
  "07|AcuteGastricUlcer|DigestiveSystemPathology|D"
  "08|KneeJointStability|JointStability|E"
)

# Python helper: checks if sub ⊑ sup in an OWL/XML file via transitive closure
# over SubClassOf and EquivalentClasses axioms (both asserted and inferred).
# Prints FOUND / NOT_FOUND.
python_check_subsumption() {
  local owx_file="$1"
  local sub_iri="$2"
  local sup_iri="$3"
  python3 - "$owx_file" "$sub_iri" "$sup_iri" <<'PYEOF'
import xml.etree.ElementTree as ET
import sys
from collections import defaultdict, deque

owx_file, sub_iri, sup_iri = sys.argv[1], sys.argv[2], sys.argv[3]
NS = '{http://www.w3.org/2002/07/owl#}'

try:
    tree = ET.parse(owx_file)
except Exception as e:
    print(f'PARSE_ERROR: {e}', file=sys.stderr)
    print('NOT_FOUND')
    sys.exit(0)

# Build a graph of SubClassOf relations (transitive closure of direct edges)
graph = defaultdict(set)

for sc in tree.iter(NS + 'SubClassOf'):
    classes = [c.get('IRI') for c in sc.findall(NS + 'Class')]
    if len(classes) == 2 and all(c for c in classes):
        graph[classes[0]].add(classes[1])

# EquivalentClasses: treat as bidirectional SubClassOf
for ec in tree.iter(NS + 'EquivalentClasses'):
    classes = [c.get('IRI') for c in ec.findall(NS + 'Class')]
    for i in range(len(classes)):
        for j in range(len(classes)):
            if i != j and classes[i] and classes[j]:
                graph[classes[i]].add(classes[j])

# BFS from sub_iri to find sup_iri
visited = set()
queue = deque([sub_iri])
visited.add(sub_iri)
found = False
while queue:
    cur = queue.popleft()
    if cur == sup_iri:
        found = True
        break
    for supr in graph[cur]:
        if supr not in visited:
            visited.add(supr)
            queue.append(supr)

print('FOUND' if found else 'NOT_FOUND')
PYEOF
}

for entry in "${PAIRS[@]}"; do
  IFS='|' read -r NN SUB_LOCAL SUP_LOCAL CLUSTER <<< "$entry"
  SUB_IRI="${BASE_NS}${SUB_LOCAL}"
  SUP_IRI="${BASE_NS}${SUP_LOCAL}"
  OWX_OUT="$FIXTURES_DIR/pair_${NN}.owx"
  OFN_OUT="$FIXTURES_DIR/pair_${NN}.ofn"
  HERMIT_OUT="$FIXTURES_DIR/pair_${NN}.hermit.owx"
  TERMS_REL="crates/owl-dl-reasoner/tests/fixtures/phase2b/p2b0-terms-${NN}.txt"
  TERMS_ABS="$FIXTURES_DIR/p2b0-terms-${NN}.txt"

  echo "========================================"
  echo "Pair $NN: $SUB_LOCAL ⊑ $SUP_LOCAL (Cluster $CLUSTER)"

  # Step a — write term file (inside repo so Docker mount can see it)
  printf "%s\n%s\n" "$SUB_IRI" "$SUP_IRI" > "$TERMS_ABS"

  # Step b — ROBOT bot-locality extract
  echo "  [b] Extracting bot-locality module..."
  docker run --rm \
    -v "$REPO_ROOT:/work" -w /work "$ROBOT_IMAGE" \
    robot extract \
      --input "ontologies/external/galen.owx" \
      --method bot \
      --term-file "$TERMS_REL" \
      --output "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.owx"
  MODULE_LINES=$(wc -l < "$OWX_OUT")
  echo "  [b] Module lines: $MODULE_LINES"

  # Step c — convert to OFN + strip Datatype declarations
  echo "  [c] Converting to OFN..."
  docker run --rm \
    -v "$REPO_ROOT:/work" -w /work "$ROBOT_IMAGE" \
    robot convert \
      --input "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.owx" \
      --format ofn \
      --output "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.ofn"
  sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' "$OFN_OUT"
  OFN_LINES=$(wc -l < "$OFN_OUT")
  echo "  [c] OFN lines: $OFN_LINES"

  # Step d — HermiT classification
  echo "  [d] Running HermiT classification..."
  bash "$CLASSIFY_ORACLE" "$OFN_OUT" "$HERMIT_OUT"

  # Step e — verify HermiT derives SUB ⊑ SUP (via transitive closure)
  echo "  [e] Verifying HermiT derivation (transitive closure)..."
  HERMIT_VERDICT=$(python_check_subsumption "$HERMIT_OUT" "$SUB_IRI" "$SUP_IRI")
  echo "  [e] HermiT verdict: $HERMIT_VERDICT"
  FALLBACK_USED="BOT"

  # If NOT_FOUND, try star method fallback
  if [[ "$HERMIT_VERDICT" == "NOT_FOUND" ]]; then
    echo "  [e] Retrying with --method star..."
    docker run --rm \
      -v "$REPO_ROOT:/work" -w /work "$ROBOT_IMAGE" \
      robot extract \
        --input "ontologies/external/galen.owx" \
        --method star \
        --term-file "$TERMS_REL" \
        --output "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.owx"
    MODULE_LINES=$(wc -l < "$OWX_OUT")
    echo "  [e] STAR module lines: $MODULE_LINES"

    docker run --rm \
      -v "$REPO_ROOT:/work" -w /work "$ROBOT_IMAGE" \
      robot convert \
        --input "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.owx" \
        --format ofn \
        --output "crates/owl-dl-reasoner/tests/fixtures/phase2b/pair_${NN}.ofn"
    sed -i -E '/^[[:space:]]*Declaration\(Datatype\(/d' "$OFN_OUT"

    bash "$CLASSIFY_ORACLE" "$OFN_OUT" "$HERMIT_OUT"

    STAR_VERDICT=$(python_check_subsumption "$HERMIT_OUT" "$SUB_IRI" "$SUP_IRI")
    if [[ "$STAR_VERDICT" == "FOUND" ]]; then
      HERMIT_VERDICT="STAR_FOUND"
      FALLBACK_USED="STAR"
    else
      HERMIT_VERDICT="NON_LOCAL"
      FALLBACK_USED="STAR"
    fi
    echo "  [e] STAR verdict: $HERMIT_VERDICT"
  fi

  # Step f — rustdl verdicts
  # sat-only: use subclass --saturation-only (fast, no tableau)
  echo "  [f] Running rustdl --saturation-only..."
  SAT_RAW=$(timeout 30s "$RUSTDL" subclass --saturation-only "$OFN_OUT" "$SUB_IRI" "$SUP_IRI" 2>&1 || echo "TIMEOUT_OR_ERROR")
  echo "  [f] rustdl sat-only raw: $SAT_RAW"
  if echo "$SAT_RAW" | grep -qiE "^yes$|^yes |subsumed|^true$"; then
    SAT_RESULT="HIT"
  elif echo "$SAT_RAW" | grep -qi "TIMEOUT_OR_ERROR"; then
    SAT_RESULT="TIMEOUT"
  else
    SAT_RESULT="MISS"
  fi

  # default: use classify --pair-timeout-ms 5000 and grep for sub ⊑ sup in output
  # (the subclass command has no per-call timeout and can hang on large modules)
  echo "  [f] Running rustdl default (classify --pair-timeout-ms 5000)..."
  DEFAULT_RAW=$("$RUSTDL" classify --pair-timeout-ms 5000 "$OFN_OUT" 2>&1 | \
    grep -F "$SUB_IRI" | grep -F "$SUP_IRI" || echo "MISS_OR_ERROR")
  echo "  [f] rustdl default raw: $DEFAULT_RAW"
  if echo "$DEFAULT_RAW" | grep -qF "$SUP_IRI"; then
    DEFAULT_RESULT="HIT"
  else
    DEFAULT_RESULT="MISS"
  fi

  # Record row
  ROW="Pair $NN | $SUB_LOCAL ⊑ $SUP_LOCAL | Cluster $CLUSTER | Module: $MODULE_LINES lines ($FALLBACK_USED) | HermiT: $HERMIT_VERDICT | sat-only: $SAT_RESULT | default: $DEFAULT_RESULT"
  echo "$ROW" | tee -a "$LOG_FILE"

done

echo ""
echo "========================================"
echo "Phase 2b.0 fixture build complete."
echo ""
cat "$LOG_FILE"
echo "========================================"
