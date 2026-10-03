#!/usr/bin/env bash
# Install the tools and local fixtures a fresh rustdl dev host is missing.
#
# Written for the coder pod (Ubuntu 26.04, passwordless sudo), but every step
# checks before acting, so it is safe to re-run and on a host that already has
# some of it. Nothing here touches the repo's tracked files.
#
#   ./scripts/install-deps.sh            # tools + PATH + wrappers + fixtures
#   SKIP_FIXTURES=1 ./scripts/install-deps.sh
#
# Steps:
#   1. apt: GNU time (CPU-time measurement), bc, perf, a JRE if none is found
#   2. maturin (Python wheel builds) via uv
#   3. pinned Rust 1.95.0 toolchain + java on PATH (marked block in ~/.bashrc)
#   4. ~/.local/bin/konclude wrapper (supplies the bundled libpcre.so.3)
#   5. gitignored fixtures rebuilt from the ORE pool: bibtex, ore_ont_10080
set -euo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN="$HOME/.local/bin"
TOOLCHAIN="$HOME/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin"
JDK_HOME=${JDK_HOME:-$(ls -d "$HOME"/bench/jdk-17* 2>/dev/null | head -1 || true)}
KONCLUDE_DIR=${KONCLUDE_DIR:-$(ls -d "$HOME"/peers/Konclude-* 2>/dev/null | head -1 || true)}
PCRE_LIB=${PCRE_LIB:-$HOME/peers/lib}
ORE_POOL=${ORE_POOL:-$HOME/data/ore-run/pool_sample/files}

say() { printf '\033[1m==> %s\033[0m\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
mkdir -p "$BIN"

# --- 1. system packages ------------------------------------------------------
pkgs=()
[ -x /usr/bin/time ] || pkgs+=(time)
have bc || pkgs+=(bc)   # release-corpus-report.sh's confirmation pass
have perf || pkgs+=(linux-perf)
if [ -z "$JDK_HOME" ] && ! have java; then pkgs+=(openjdk-17-jre-headless); fi
if [ ${#pkgs[@]} -gt 0 ]; then
  say "apt install ${pkgs[*]}"
  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${pkgs[@]}"
else
  say "system packages already present"
fi
# perf in a container is usually blocked by perf_event_paranoid; report, don't change.
if have perf && [ "$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 0)" -gt 1 ]; then
  echo "    note: perf_event_paranoid=$(cat /proc/sys/kernel/perf_event_paranoid);" \
       "'perf record' may need 'sudo sysctl kernel.perf_event_paranoid=1' (if the pod allows it)"
fi

# --- 2. maturin --------------------------------------------------------------
if have maturin; then
  say "maturin already present ($(maturin --version))"
elif have uv; then
  say "uv tool install maturin"
  uv tool install maturin
else
  say "pip install --user maturin (no uv found)"
  pip install --user --break-system-packages maturin
fi

# --- 3. PATH: pinned toolchain + JDK ------------------------------------------
# The pinned toolchain is put on PATH explicitly rather than relying on `stable`,
# which resolves to a different compiler on each host (see CLAUDE.md).
[ -x "$TOOLCHAIN/cargo" ] || { echo "ERROR: pinned toolchain missing at $TOOLCHAIN" >&2; exit 1; }
MARK_BEGIN="# >>> rustdl install-deps >>>"
MARK_END="# <<< rustdl install-deps <<<"
block="$MARK_BEGIN
export PATH=\"$TOOLCHAIN:\$PATH\""
if [ -n "$JDK_HOME" ]; then
  block="$block
export JAVA_HOME=\"$JDK_HOME\"
export PATH=\"\$JAVA_HOME/bin:\$PATH\""
fi
block="$block
$MARK_END"
if grep -qF "$MARK_BEGIN" "$HOME/.bashrc" 2>/dev/null; then
  # Replace the old block so re-runs pick up changed paths.
  python3 - "$HOME/.bashrc" "$MARK_BEGIN" "$MARK_END" "$block" <<'PY'
import sys, re
path, begin, end, block = sys.argv[1:]
text = open(path).read()
text = re.sub(re.escape(begin) + r".*?" + re.escape(end), lambda _: block, text, flags=re.S)
open(path, "w").write(text)
PY
  say "updated PATH block in ~/.bashrc"
else
  printf '\n%s\n' "$block" >> "$HOME/.bashrc"
  say "added PATH block to ~/.bashrc (open a new shell or: source ~/.bashrc)"
fi
export PATH="$TOOLCHAIN:$PATH"
[ -n "$JDK_HOME" ] && export JAVA_HOME="$JDK_HOME" PATH="$JDK_HOME/bin:$PATH"

# --- 4. Konclude wrapper -----------------------------------------------------
# The Linux static build links libpcre.so.3 (PCRE1), absent on modern Ubuntu.
# Use the real bundled library; never symlink PCRE2 in its place.
if [ -n "$KONCLUDE_DIR" ] && [ -x "$KONCLUDE_DIR/Binaries/Konclude" ]; then
  if [ ! -e "$PCRE_LIB/libpcre.so.3" ]; then
    echo "WARNING: $PCRE_LIB/libpcre.so.3 missing; fetch the libpcre3 .deb and extract it there" >&2
  fi
  cat > "$BIN/konclude" <<EOF
#!/usr/bin/env bash
export LD_LIBRARY_PATH="$PCRE_LIB\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
exec "$KONCLUDE_DIR/Binaries/Konclude" "\$@"
EOF
  chmod +x "$BIN/konclude"
  say "wrote $BIN/konclude"
else
  echo "WARNING: no Konclude under ~/peers; skipping wrapper and oracle generation" >&2
  SKIP_FIXTURES=1
fi

# --- 5. fixtures from the ORE pool (gitignored) --------------------------------
if [ "${SKIP_FIXTURES:-0}" != 1 ]; then
  if [ ! -d "$ORE_POOL" ]; then
    echo "WARNING: ORE pool not found at $ORE_POOL; skipping fixtures" >&2
  else
    # name  pool-file  input-path  oracle-path
    while read -r name src in out; do
      if [ -s "$in" ] && [ -s "$out" ]; then
        say "fixture $name already present"; continue
      fi
      say "fixture $name: copying input + generating Konclude oracle"
      mkdir -p "$(dirname "$in")" "$(dirname "$out")"
      [ -s "$in" ] || cp "$ORE_POOL/$src" "$in"
      "$BIN/konclude" classification -w AUTO -i "$in" -o "$out" >/dev/null 2>&1
      [ -s "$out" ] || { echo "ERROR: Konclude produced no oracle for $name" >&2; exit 1; }
    done <<EOF
bibtex ore_ont_3341.owl $REPO/ontologies/real/bibtex.ofn $REPO/ontologies/real/konclude-input/bibtex-classified.owx
ore-10080 ore_ont_10080.owl $HOME/data/ore-run/input/ore_ont_10080.ofn $HOME/data/ore-run/oracle/ore_ont_10080-classified.owx
EOF
  fi
fi

# --- summary -----------------------------------------------------------------
say "check"
printf '  %-8s %s\n' cargo "$(cargo --version 2>&1)"
printf '  %-8s %s\n' java "$(java -version 2>&1 | head -1)"
printf '  %-8s %s\n' time "$([ -x /usr/bin/time ] && /usr/bin/time --version 2>&1 | head -1 || echo MISSING)"
printf '  %-8s %s\n' bc "$(command -v bc || echo MISSING)"
printf '  %-8s %s\n' perf "$(perf --version 2>&1 || echo MISSING)"
printf '  %-8s %s\n' maturin "$(maturin --version 2>&1 || echo MISSING)"
printf '  %-8s %s\n' konclude "$("$BIN/konclude" -h 2>&1 | grep -m1 -o 'Version v[^ ]*' || echo MISSING)"
echo "Not obtainable anywhere (net reports them as the 2 expected failures): alehif-test, notgalen."
