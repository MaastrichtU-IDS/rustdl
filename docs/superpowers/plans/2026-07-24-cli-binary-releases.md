# Cross-Platform rustdl CLI Binary Releases — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On every `v*.*.*` tag push, build standalone `rustdl` CLI binaries for the four platforms the Protégé plugin will bundle, and attach them to the GitHub Release — without touching the existing PyPI/wheel release path.

**Architecture:** A NEW, standalone workflow `.github/workflows/release-cli.yml` (Deliverable 2 of the Protégé-plugin design, §3.2). A `build-cli` matrix job cross-builds `rustdl` on native runners for `{x86_64-unknown-linux-musl, aarch64-unknown-linux-musl, aarch64-apple-darwin, x86_64-pc-windows-msvc}`, strips it, renames it by Rust target triple, and uploads it as a workflow artifact. A `publish-cli` job aggregates the artifacts, generates `SHA256SUMS`, and — only on a tag push — attaches everything to the release using an idempotent `create-or-upload` that safely coexists with `release-python.yml`'s own `github-release` job. The existing `release-python.yml` is left completely untouched, so a CLI-build failure can never block PyPI publishing.

**Tech Stack:** GitHub Actions; `cargo build --release --target <triple>`; musl static linking (Linux); MSVC `crt-static` (Windows); `dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `actions/upload-artifact@v7`, `actions/download-artifact@v8`, `gh` CLI. `actionlint` for local static validation.

## Global Constraints

Every task's requirements implicitly include this section. Values are exact and load-bearing.

- **Toolchain:** build with `RUSTUP_TOOLCHAIN: stable` (env), matching the repo convention (CLAUDE.md; `release-python.yml`). This bypasses `rust-toolchain.toml`'s `channel = "1.95.0"` pin, which exists only for fmt/clippy lint consistency — release artifacts build on stable.
- **Static/portable linking (seamlessness — no runtime deps on the user's machine):**
  - Linux targets are **musl static** (`*-unknown-linux-musl`) — a single binary runs on any Linux regardless of glibc version. The `rustdl` binary has **no C/native dependencies** (verified: `cargo tree -p owl-dl-cli -i ring` → not in graph), so musl builds cleanly.
  - Windows uses **`-C target-feature=+crt-static`** — no VC++ redistributable required.
  - macOS sets **`MACOSX_DEPLOYMENT_TARGET: "11.0"`** (arm64's floor).
- **Strip for distribution:** pass **`-C strip=symbols`** via `RUSTFLAGS`. The `[profile.release]` in the root `Cargo.toml` sets `debug = "line-tables-only"`; distributed binaries must not carry it.
- **Release-asset naming = Rust target triple**, verbatim (this is the stability contract Plan C — the Java plugin — consumes):
  - `rustdl-x86_64-unknown-linux-musl`
  - `rustdl-aarch64-unknown-linux-musl`
  - `rustdl-aarch64-apple-darwin`
  - `rustdl-x86_64-pc-windows-msvc.exe`
  - plus `SHA256SUMS`
- **Do NOT modify `.github/workflows/release-python.yml`.** The PyPI path stays isolated. The two workflows share only the GitHub Release object, reconciled idempotently (Task 2).
- **Pinned action SHA:** reuse the repo's pinned `dtolnay/rust-toolchain` ref `3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, pinned` (as in `ci.yml`).
- **Runner labels** match `release-python.yml`: `ubuntu-latest`, `ubuntu-24.04-arm` (native ARM, no QEMU), `macos-14`, `windows-latest`.
- **GitHub's workflow_dispatch limitation:** a brand-new workflow file is only dispatchable once it exists on the **default branch**. Full 4-platform CI verification therefore runs **after merge** via a no-tag `workflow_dispatch` on `main` (zero release/PyPI side effects — the release-upload step is tag-guarded). Pre-merge per-task verification is `actionlint` + a real local native build.

## File Structure

- **Create:** `.github/workflows/release-cli.yml` — the entire workflow (Tasks 1 + 2 build it up: Task 1 = triggers + `build-cli` matrix; Task 2 = `publish-cli`).
- **Create:** `docs/cli-binaries.md` — the binary-distribution contract: target-triple ↔ (OS, arch) mapping, static-linking guarantees, asset names, `SHA256SUMS`, and the Java-side (`os.name`/`os.arch`) → triple mapping Plan C will implement (Task 3).
- **Do NOT touch:** `.github/workflows/release-python.yml`, `.github/workflows/ci.yml`, any `Cargo.toml`.

---

### Task 1: `build-cli` matrix job (cross-build + stage the four binaries)

**Files:**
- Create: `.github/workflows/release-cli.yml`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: workflow artifacts named `cli-<triple>`, each containing one file `rustdl-<triple>[.exe]` (staged, stripped, `--version`-verified). Task 2's `publish-cli` downloads these via the `cli-*` glob.

- [ ] **Step 1: Write the workflow file with triggers, permissions, and the `build-cli` matrix job**

Create `.github/workflows/release-cli.yml` with exactly this content:

```yaml
name: Release CLI binaries

# Standalone from release-python.yml on purpose: a CLI-build failure must
# never block PyPI publishing. Both fire on the same v*.*.* tag; they share
# only the GitHub Release object, reconciled idempotently in publish-cli.
on:
  push:
    tags: ['v*.*.*']
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build-cli:
    name: Build rustdl ${{ matrix.target }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-musl
            apt: musl-tools
          # Native ARM runner (ubuntu-24.04-arm, GA 2025) — no QEMU, ~4 min.
          - os: ubuntu-24.04-arm
            target: aarch64-unknown-linux-musl
            apt: musl-tools
          - os: macos-14
            target: aarch64-apple-darwin
            apt: ''
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            apt: ''
    env:
      # Build on stable regardless of rust-toolchain.toml's 1.95.0 lint pin
      # (see CLAUDE.md / release-python.yml). Strip line-tables debug info
      # that [profile.release] carries. Windows adds static CRT so no VC++
      # redistributable is needed on the user's machine.
      RUSTUP_TOOLCHAIN: stable
      MACOSX_DEPLOYMENT_TARGET: "11.0"
    steps:
      - uses: actions/checkout@v6

      - name: Install musl build tools (Linux)
        if: matrix.apt != ''
        run: sudo apt-get update && sudo apt-get install -y ${{ matrix.apt }}

      - uses: dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9 # master, pinned
        with:
          toolchain: stable
          targets: ${{ matrix.target }}

      - uses: Swatinem/rust-cache@v2
        with:
          key: cli-${{ matrix.target }}

      - name: Set RUSTFLAGS (unix)
        if: runner.os != 'Windows'
        run: echo "RUSTFLAGS=-C strip=symbols" >> "$GITHUB_ENV"

      - name: Set RUSTFLAGS (windows)
        if: runner.os == 'Windows'
        shell: bash
        run: echo "RUSTFLAGS=-C strip=symbols -C target-feature=+crt-static" >> "$GITHUB_ENV"

      - name: Build rustdl (release)
        run: cargo build --release --locked --target ${{ matrix.target }} -p owl-dl-cli --bin rustdl

      - name: Stage + smoke-test binary (unix)
        if: runner.os != 'Windows'
        run: |
          mkdir -p staging
          cp "target/${{ matrix.target }}/release/rustdl" "staging/rustdl-${{ matrix.target }}"
          "target/${{ matrix.target }}/release/rustdl" --version

      - name: Stage + smoke-test binary (windows)
        if: runner.os == 'Windows'
        shell: bash
        run: |
          mkdir -p staging
          cp "target/${{ matrix.target }}/release/rustdl.exe" "staging/rustdl-${{ matrix.target }}.exe"
          "target/${{ matrix.target }}/release/rustdl.exe" --version

      - uses: actions/upload-artifact@v7
        with:
          name: cli-${{ matrix.target }}
          path: staging/*
          if-no-files-found: error
```

Notes for the implementer:
- `RUSTFLAGS` is set via `$GITHUB_ENV` (not a job-level `env:`) because the Windows value differs; the `--version` smoke test both proves the binary runs on its build host (the Linux musl and macOS/Windows-native jobs all run their own artifact) and fails the job loudly if the build produced a broken binary.
- `--locked` is safe: `Cargo.lock` is committed and current. If `--locked` ever fails in CI because the lockfile is stale, that is a real signal to commit an updated lockfile — do NOT drop `--locked` to paper over it.
- The aarch64-linux job runs on a **native** ARM runner, so its `--version` smoke test executes natively (no emulation).

- [ ] **Step 2: Validate the workflow statically with actionlint**

Install if needed, then lint:

```bash
command -v actionlint >/dev/null || brew install actionlint
actionlint .github/workflows/release-cli.yml
```

Expected: no output, exit 0. actionlint parses the YAML, checks `${{ }}` expressions, matrix references (`matrix.target`, `matrix.apt`), `runner.os`, and step shell usage. Fix any reported issue.

- [ ] **Step 3: Prove the build recipe locally on the host target**

The dev host is `aarch64-apple-darwin` (one of the four targets). Run the exact command the workflow runs, then verify the binary is stripped and works:

```bash
cd /Users/micheldumontier/code/rustdl
RUSTUP_TOOLCHAIN=stable RUSTFLAGS="-C strip=symbols" \
  cargo build --release --locked --target aarch64-apple-darwin -p owl-dl-cli --bin rustdl
./target/aarch64-apple-darwin/release/rustdl --version
# stripped check: no debug/symbol table sections of note
size -m ./target/aarch64-apple-darwin/release/rustdl 2>/dev/null | head -3 || true
```

Expected: build succeeds; `--version` prints the rustdl version line (exit 0). This proves the `cargo build … --target … -p owl-dl-cli --bin rustdl` recipe, the `RUSTFLAGS` strip, and the staged filename all match reality. (The other three triples build on their native CI runners; they cannot be cross-built from macOS without extra toolchains and are verified post-merge — see Global Constraints.)

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release-cli.yml
git commit -m "ci: add release-cli build matrix (four-platform rustdl binaries)"
```

---

### Task 2: `publish-cli` job (SHA256SUMS + idempotent tag-gated release upload)

**Files:**
- Modify: `.github/workflows/release-cli.yml` (append the `publish-cli` job; add `contents: write` where it needs it)

**Interfaces:**
- Consumes: the `cli-<triple>` workflow artifacts produced by `build-cli` (Task 1), gathered via the `cli-*` pattern.
- Produces: on a tag push, the release assets `rustdl-<triple>[.exe]` (×4) and `SHA256SUMS` attached to the `v*.*.*` GitHub Release.

- [ ] **Step 1: Append the `publish-cli` job**

Add this job to `.github/workflows/release-cli.yml` (after `build-cli`). It grants `contents: write` on the job (the top-level `permissions:` stays `contents: read`):

```yaml
  publish-cli:
    name: Attach binaries to the release
    needs: build-cli
    runs-on: ubuntu-latest
    permissions:
      contents: write   # create/upload release assets
    steps:
      - uses: actions/download-artifact@v8
        with:
          pattern: 'cli-*'
          path: dist
          merge-multiple: true

      - name: Generate SHA256SUMS
        working-directory: dist
        run: |
          sha256sum rustdl-* > SHA256SUMS
          cat SHA256SUMS

      - name: List staged assets
        run: ls -l dist

      - name: Attach to the GitHub Release
        # Only on a real tag; a workflow_dispatch dry run stops after building
        # + checksumming, leaving the four binaries as inspectable workflow
        # artifacts and touching no release.
        if: startsWith(github.ref, 'refs/tags/')
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        run: |
          tag="${GITHUB_REF_NAME}"
          # Idempotent + race-safe with release-python.yml's github-release job,
          # which also creates this release. Whichever workflow arrives first
          # creates a bare release; the loser's create is a harmless no-op
          # (|| true). release-python's github-release `edit` step later
          # overwrites the notes with the CHANGELOG section, so final notes are
          # always the CHANGELOG regardless of order. --clobber makes re-runs
          # (and asset name collisions across runs) safe.
          gh release create "$tag" --title "rustdl ${tag#v}" \
            --notes "Release ${tag}. See CHANGELOG.md for details." || true
          gh release upload "$tag" dist/rustdl-* dist/SHA256SUMS --clobber
```

- [ ] **Step 2: Validate with actionlint**

```bash
actionlint .github/workflows/release-cli.yml
```

Expected: exit 0, no findings. Confirms the `needs`, the job-level `permissions`, the `if:` expression, and the shell script parse cleanly.

- [ ] **Step 3: Statically verify the tag-guard and upload logic by inspection**

There is no local way to exercise `gh release` without a real release; verify the two invariants by reading the job:
1. The `Attach to the GitHub Release` step is the ONLY step gated by `if: startsWith(github.ref, 'refs/tags/')` — so a `workflow_dispatch` run performs the download + `SHA256SUMS` + `ls` and then stops. Confirm no other step has release side effects.
2. The upload is idempotent: `gh release create … || true` never fails the job, and `gh release upload … --clobber` overwrites existing assets. Confirm `--clobber` is present and the create is `|| true`.

Optionally shell-lint the script bodies:

```bash
command -v shellcheck >/dev/null && \
  awk '/run: \|/{f=1;next} f&&/^[^ ]/{f=0} f' .github/workflows/release-cli.yml | shellcheck - || echo "shellcheck skipped"
```

(Advisory only — `shellcheck` on extracted fragments can report false positives about the leading indentation; the binding gate is actionlint + inspection.)

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release-cli.yml
git commit -m "ci: attach rustdl binaries + SHA256SUMS to the release (idempotent, tag-gated)"
```

---

### Task 3: Binary-distribution contract doc (`docs/cli-binaries.md`)

**Files:**
- Create: `docs/cli-binaries.md`

**Interfaces:**
- Consumes: the asset names produced by Tasks 1–2.
- Produces: the documented mapping Plan C (the Java plugin's `RustdlBinary`) implements to pick the right embedded binary at runtime.

- [ ] **Step 1: Write the doc**

Create `docs/cli-binaries.md` with this content:

```markdown
# rustdl CLI binary releases

Every `v*.*.*` tag triggers `.github/workflows/release-cli.yml`, which builds
standalone `rustdl` binaries and attaches them to that version's GitHub
Release. These are the binaries the Protégé plugin (see
`docs/superpowers/specs/2026-07-24-protege-plugin-design.md`) bundles so the
user never installs a binary or edits their PATH.

## Release assets

| Asset | Target triple | Linking |
|-------|---------------|---------|
| `rustdl-x86_64-unknown-linux-musl`   | `x86_64-unknown-linux-musl`  | fully static (musl) |
| `rustdl-aarch64-unknown-linux-musl`  | `aarch64-unknown-linux-musl` | fully static (musl) |
| `rustdl-aarch64-apple-darwin`        | `aarch64-apple-darwin`       | dynamic vs. system libSystem; `MACOSX_DEPLOYMENT_TARGET=11.0` |
| `rustdl-x86_64-pc-windows-msvc.exe`  | `x86_64-pc-windows-msvc`     | static CRT (`+crt-static`); no VC++ redistributable |
| `SHA256SUMS`                         | —                            | `sha256sum` of every `rustdl-*` asset |

All binaries are stripped (`-C strip=symbols`). The Linux binaries are fully
static and run on any Linux regardless of glibc version.

## Not shipped

- **macOS x86_64 (Intel):** intentionally omitted (Apple is sunsetting Intel;
  GitHub's Intel Mac runners are heavily oversubscribed). Intel-Mac users build
  from source (`cargo install --path crates/owl-dl-cli`) and point the plugin
  at it via the `RUSTDL_BIN` override.
- Any other platform: same override path.

## Consuming from Java (Plan C — the plugin's `RustdlBinary`)

Map the JVM's `os.name` / `os.arch` to the bundled asset:

| `os.name` starts with | `os.arch` | Bundled binary |
|-----------------------|-----------|----------------|
| `Linux`   | `amd64`, `x86_64`  | `rustdl-x86_64-unknown-linux-musl` |
| `Linux`   | `aarch64`, `arm64` | `rustdl-aarch64-unknown-linux-musl` |
| `Mac`     | `aarch64`, `arm64` | `rustdl-aarch64-apple-darwin` |
| `Windows` | `amd64`, `x86_64`  | `rustdl-x86_64-pc-windows-msvc.exe` |
| (anything else)       | —         | none — require the `RUSTDL_BIN` override |

The plugin extracts the matching binary to a per-user cache dir, `chmod +x`
(non-Windows), verifies it with `rustdl --version`, and invokes it per the JSON
contract (`docs/json-schema.md`).

## Release coordination

`release-cli.yml` is independent of `release-python.yml` so a CLI-build failure
never blocks PyPI. Both fire on the same tag and share only the GitHub Release
object: whichever reaches release creation first makes a bare release, and
`release-python.yml`'s `github-release` job fills in the CHANGELOG notes via its
`edit` path. Asset uploads use `--clobber`, so re-running either workflow is
safe.
```

- [ ] **Step 2: Verify internal consistency**

Confirm every asset name and triple in `docs/cli-binaries.md` matches, character-for-character, the names produced in Tasks 1–2, and that the referenced files exist:

```bash
cd /Users/micheldumontier/code/rustdl
for f in docs/json-schema.md docs/superpowers/specs/2026-07-24-protege-plugin-design.md; do
  test -f "$f" && echo "ok: $f" || echo "MISSING: $f"
done
grep -c "unknown-linux-musl" docs/cli-binaries.md   # expect 4 (2 assets rows + 2 mapping rows)
```

Expected: both referenced docs exist; the grep count is ≥ 4.

- [ ] **Step 3: Commit**

```bash
git add docs/cli-binaries.md
git commit -m "docs: document the CLI binary release assets + plugin consumption contract"
```

---

## Integration Verification (run after merge — controller, not a task)

Because GitHub only registers `workflow_dispatch` from the default branch, the
real four-platform build is verified **after** `release-cli.yml` lands on `main`,
with **no** release/PyPI side effects (no tag ⇒ the upload step is skipped):

```bash
gh workflow run release-cli.yml --ref main
gh run watch "$(gh run list --workflow=release-cli.yml --limit 1 --json databaseId -q '.[0].databaseId')"
```

Expected: all four `build-cli` matrix legs succeed; `publish-cli` downloads the
four `cli-*` artifacts, prints `SHA256SUMS`, and the `Attach to the GitHub
Release` step shows as **skipped**. Download the run artifacts and confirm four
`rustdl-<triple>[.exe]` binaries plus `SHA256SUMS` are present.

The first subsequent real `v*.*.*` tag then attaches these assets to the
release automatically.

## Self-Review

- **Spec coverage:** design §3.2 (cross-platform CLI binaries, same targets as
  the wheel job) → Tasks 1–2; §5 `RustdlBinary` platform routing / seamless
  bundling → the consumption contract in Task 3; §8 "seamless install / four
  binaries" target set → the matrix. ✔
- **Placeholder scan:** none — all YAML and doc content is complete and literal.
- **Type/name consistency:** the four triples and asset names are identical
  across the Global Constraints, Task 1 staging, Task 2 upload glob
  (`rustdl-*`), and Task 3's tables. `cli-<triple>` artifact names ↔ the
  `cli-*` download pattern match.
