#!/usr/bin/env bash
# Replay the package F-lite mutants (#363): does a cross-command law catch each one?
#
# Each scripts/mutants/M*.patch plants one known engine fault. Its header gives the
# base commit it was written against, the layer, what it re-opens, the env it runs
# under, and the expected detector (`LAW@ontology`, or `none`). The patches are DATA:
# they are never applied in this tree, and nothing here is built by CI or by
# `cargo test`. Run by hand, on a quiet host.
#
# For the control and for each mutant, in a scratch worktree of HEAD:
#   git apply (a patch that does not apply is STALE, never caught/uncaught)
#   -> cargo build --release -p owl-dl-cli -> revert
#   -> scripts/cross-command-laws.py --quick --hermit [--env from the header]
#   -> detect.py: the laws that fire in the mutant run and not in the control run.
# Output: one line per mutant, MATCH / MISMATCH / STALE / BUILD-FAILED, and a TSV.
# Exit 0 only if every mutant MATCHes its expected detector; STALE, BUILD-FAILED
# and MISMATCH all exit non-zero, so drift is visible.
#
# Usage: scripts/mutants/replay-mutants.sh [M1 M3 ...]   (default: every patch)
# Env:   CARGO_TARGET_DIR (default ~/target-mutants), LAWS_CORES (e.g. 92,93,94),
#        MUTANT_OUT (default a fresh mktemp dir). Needs python3, the gitignored
#        fixtures in ontologies/real, and robot.jar + a JDK for --hermit.
# Cost: one release build per mutant (thin LTO: ~5-15 min) plus ~5 min of harness.
set -uo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MUT="$REPO/scripts/mutants"
OUT="${MUTANT_OUT:-$(mktemp -d -t rustdl-mutants.XXXXXX)}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/target-mutants}"
CORES=()
[[ -n "${LAWS_CORES:-}" ]] && CORES=(--cores "$LAWS_CORES")

ids=("$@")
if [[ ${#ids[@]} -eq 0 ]]; then
  for p in "$MUT"/M*.patch; do ids+=("$(basename "$p" .patch)"); done
fi

WT="$OUT/wt"
git -C "$REPO" worktree add --detach "$WT" HEAD >/dev/null || exit 2
trap 'git -C "$REPO" worktree remove --force "$WT" >/dev/null 2>&1' EXIT
# The gitignored fixtures: share the main checkout's.
rm -rf "$WT/ontologies" && ln -s "$REPO/ontologies" "$WT/ontologies"

header() { sed -n "s/^# $2: //p" "$1" | head -1; }

build() {  # $1 = label; leaves the binary at $OUT/bin-$1
  (cd "$WT" && cargo build --release -p owl-dl-cli >"$OUT/build-$1.log" 2>&1) || return 1
  cp "$CARGO_TARGET_DIR/release/rustdl" "$OUT/bin-$1"
}

harness() {  # $1 = label, $2 = env spec (space-separated K=V, may be empty)
  local envargs=()
  for kv in $2; do envargs+=(--env "$kv"); done
  python3 "$WT/scripts/cross-command-laws.py" --bin "$OUT/bin-$1" --quick --hermit \
    --out "$OUT/laws-$1" "${CORES[@]}" "${envargs[@]}" >"$OUT/laws-$1.log" 2>&1
  [[ -f "$OUT/laws-$1/report.json" ]]
}

echo "output: $OUT"
build control || { echo "control build failed: $OUT/build-control.log"; exit 2; }
# One control run per distinct env (a mutant is compared with the unmutated binary
# under the SAME env). The control must itself be clean, or every comparison
# below is meaningless.
declare -A CONTROL=()
control_for() {  # $1 = env spec; sets CTL to the control's label
  local key="${1:-default}"
  if [[ -z "${CONTROL[$key]:-}" ]]; then
    local lab="control-${#CONTROL[@]}"
    cp "$OUT/bin-control" "$OUT/bin-$lab"
    harness "$lab" "$1" || { echo "control harness failed: $OUT/laws-$lab.log"; exit 2; }
    if [[ -n "$(python3 -c "import json,sys; r=json.load(open(sys.argv[1])); print(len(r['failures']) or '')" "$OUT/laws-$lab/report.json")" ]]; then
      echo "control run ($key) has failures: $OUT/laws-$lab.log"; exit 2
    fi
    CONTROL[$key]="$lab"
  fi
  CTL="${CONTROL[$key]}"
}

status=0
printf 'id\tresult\texpected\tcaught\n' >"$OUT/results.tsv"
for id in "${ids[@]}"; do
  p="$MUT/$id.patch"
  want="$(header "$p" expect)"; envs="$(header "$p" env)"
  if [[ "$envs" == "-" ]]; then envs=""; fi
  if ! git -C "$WT" apply --check "$p" 2>/dev/null; then
    res=STALE; got="-"; status=1
  else
    git -C "$WT" apply "$p"
    if build "$id"; then
      git -C "$WT" apply -R "$p"
      control_for "$envs"
      if harness "$id" "$envs"; then
        got="$(python3 "$MUT/detect.py" "$OUT/laws-$CTL/report.json" \
                "$OUT/laws-$id/report.json" | paste -sd, -)"
        [[ -z "$got" ]] && got=none
        # MATCH when every expected detector fired (extra detectors are fine).
        res=MATCH
        if [[ "$want" == none ]]; then
          [[ "$got" == none ]] || res=MISMATCH
        else
          for w in ${want//,/ }; do [[ ",$got," == *",$w,"* ]] || res=MISMATCH; done
        fi
      else
        res=HARNESS-FAILED; got="-"
      fi
    else
      git -C "$WT" apply -R "$p"; res=BUILD-FAILED; got="-"
    fi
    [[ "$res" == MATCH ]] || status=1
  fi
  printf '%-4s %-14s expected %-28s caught %s\n' "$id" "$res" "$want" "$got"
  printf '%s\t%s\t%s\t%s\n' "$id" "$res" "$want" "$got" >>"$OUT/results.tsv"
done
echo "results: $OUT/results.tsv"
exit $status
