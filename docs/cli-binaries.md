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
object: whichever reaches release creation first makes a bare release, and —
**provided the PyPI publish succeeds** — `release-python.yml`'s `github-release`
job then fills in the CHANGELOG notes via its `edit` path. If that job never runs
(e.g. `publish-pypi` fails or the tag's version already exists on PyPI), the
release keeps the generic notes `release-cli.yml` set; re-running the release
workflow after fixing the PyPI issue restores the CHANGELOG notes. Asset uploads
use `--clobber`, so re-running either workflow is safe.

**Known caveat (benign, rare):** because both workflows create the release on the
same tag, there is a narrow create-create window. `release-cli.yml`'s create is
idempotent (`gh release create … || true`), but `release-python.yml`'s
`github-release` create branch is not, so if the CLI workflow creates the release
in the exact interval between that job's `gh release view` and its `gh release
create`, the Python job's release-asset/notes step can fail with a 422. **PyPI
publishing is never affected** (it completes before `github-release` runs); only
the GitHub Release's wheel attachments/notes are, and re-running the
`github-release` job fixes it. In practice the CLI job (gated only on the build
matrix) almost always creates the release well before the Python `github-release`
job (gated behind the full wheel build + `publish-pypi`), so the Python job takes
its race-free `edit` path. A shared `concurrency:` group across both workflows
would close the window entirely if it ever proves troublesome.
