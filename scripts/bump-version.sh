#!/usr/bin/env bash
# Bump the workspace version everywhere it is written. Usage: bump-version.sh 0.4.25
#
# WHY THIS EXISTS. The version appears in NINE places in the root Cargo.toml: once
# in [workspace.package] and once per inter-crate pin in [workspace.dependencies].
# Hand-editing them drifted before — the pins sat at 0.4.5 while the package
# version reached 0.4.24, i.e. 19 releases of skew, harmless only because
# 0.4.24 happens to satisfy ^0.4.5. It also matters for release: publish-crates.yml
# refuses a tag that does not match [workspace.package] version, so a forgotten
# bump fails preflight rather than shipping the wrong number.
#
# Does NOT commit and does NOT tag — those stay deliberate.
set -euo pipefail
new=${1:?usage: bump-version.sh <x.y.z>}
[[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a semver: $new" >&2; exit 2; }
cd "$(dirname "$0")/.."

old=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
[ -n "$old" ] || { echo "could not read [workspace.package] version" >&2; exit 1; }
if [ "$old" = "$new" ]; then echo "already $new; nothing to do"; exit 0; fi

# ATOMIC: every check below can fail, and a half-bumped manifest is its own trap
# (8 of 9 sites rewritten, exit 1, dirty tree). Back up first and restore on any
# failure, so the file is either fully bumped or untouched.
backup=$(mktemp); cp Cargo.toml "$backup"
lockbackup=""
if [ -f Cargo.lock ]; then lockbackup=$(mktemp); cp Cargo.lock "$lockbackup"; fi
restore() {
  cp "$backup" Cargo.toml
  [ -n "$lockbackup" ] && cp "$lockbackup" Cargo.lock
  rm -f "$backup" "$lockbackup"
}
# EXIT, not ERR: `set -e`/ERR does NOT fire on an explicit `exit 1`, and the
# leftover-site check below exits explicitly — an earlier ERR version left 8 of 9
# sites bumped on failure, caught only by deliberately sabotaging the check.
trap 'restore' EXIT

# Only touch the version literals: the package version line and the `version = "…"`
# inside each path-dependency spec. Anchored so an unrelated "0.4.24" elsewhere
# (a doc string, a changelog entry) is never rewritten.
python3 - "$old" "$new" <<'PY'
import re, sys
old, new = sys.argv[1], sys.argv[2]
s = open('Cargo.toml').read()
n_pkg = len(re.findall(r'(?m)^version = "%s"$' % re.escape(old), s))
s = re.sub(r'(?m)^version = "%s"$' % re.escape(old), 'version = "%s"' % new, s)
n_dep = len(re.findall(r'(path = "crates/[a-z0-9-]+", version = ")%s(")' % re.escape(old), s))
s = re.sub(r'(path = "crates/[a-z0-9-]+", version = ")%s(")' % re.escape(old),
           lambda m: m.group(1) + new + m.group(2), s)
open('Cargo.toml','w').write(s)
print(f"  [workspace.package] version: {n_pkg}")
print(f"  inter-crate pins:            {n_dep}")
PY

# A leftover literal means a site the patterns above did not cover — fail loudly
# rather than leave a half-bumped manifest.
if grep -qF "\"$old\"" Cargo.toml; then
  echo "STILL PRESENT after bump — a version site was missed:" >&2
  grep -nF "\"$old\"" Cargo.toml >&2
  echo "(manifest restored; no partial bump left behind)" >&2
  exit 1
fi

# Prove it to cargo, not just to grep.
got=$(cargo metadata --no-deps --format-version 1 \
      | python3 -c "import json,sys;print(next(p['version'] for p in json.load(sys.stdin)['packages'] if p['name']=='owl-dl-core'))")
[ "$got" = "$new" ] || { echo "cargo reports $got, expected $new" >&2; exit 1; }

trap - EXIT
rm -f "$backup" "$lockbackup"
echo "$old -> $new  (cargo agrees; Cargo.lock refreshed)"
echo "Next: review the diff, commit, then tag v$new to publish."
