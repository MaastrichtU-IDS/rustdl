use crate::matrix::model::HostInfo;
use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::SystemTime;
use walkdir::WalkDir;

pub fn parse_konclude_version(banner: &str) -> Option<String> {
    // find "Version vX.Y.Z-BUILD"
    for line in banner.lines() {
        if let Some(i) = line.find("Version v") {
            let rest = &line[i + "Version ".len()..];
            let token = rest.split_whitespace().next()?;
            return Some(token.to_string());
        }
    }
    None
}

pub fn parse_robot_version(s: &str) -> Option<String> {
    s.lines()
        .find_map(|l| l.trim().strip_prefix("ROBOT version "))
        .map(|v| v.trim().to_string())
}

fn cmd_output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().ok()?;
    // Konclude prints its banner to stderr; robot to stdout — merge both.
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

pub fn newest_source_mtime(repo_root: &Path) -> Result<SystemTime> {
    let mut newest = SystemTime::UNIX_EPOCH;
    for entry in WalkDir::new(repo_root.join("crates")).into_iter().filter_map(std::result::Result::ok) {
        let p = entry.path();
        let is_src = p.extension().is_some_and(|e| e == "rs")
            || p.file_name().is_some_and(|n| n == "Cargo.toml");
        if is_src
            && let Some(m) = entry.metadata().ok().and_then(|m| m.modified().ok())
            && m > newest
        {
            newest = m;
        }
    }
    // The workspace-root Cargo.lock AND Cargo.toml carry build-affecting config
    // (dep pins; [profile.release] lto/codegen-units, [workspace.lints]) that does
    // NOT flow through any file under crates/. Fold both into the max.
    for root_file in ["Cargo.lock", "Cargo.toml"] {
        if let Ok(m) = std::fs::metadata(repo_root.join(root_file)).and_then(|m| m.modified())
            && m > newest
        {
            newest = m;
        }
    }
    Ok(newest)
}

pub fn assert_fresh_binary(binary: &Path, repo_root: &Path) -> Result<()> {
    let bin_mtime = std::fs::metadata(binary)
        .with_context(|| format!("stat {}", binary.display()))?
        .modified()?;
    let src_mtime = newest_source_mtime(repo_root)?;
    if bin_mtime < src_mtime {
        return Err(anyhow!(
            "STALE BINARY: {} is older than the newest source file. Rebuild with \
             `RUSTUP_TOOLCHAIN=stable cargo build --release -p owl-dl-cli` before benchmarking.",
            binary.display()
        ));
    }
    Ok(())
}

pub fn capture_host() -> HostInfo {
    let one = |prog: &str, args: &[&str]| -> String {
        cmd_output(prog, args).unwrap_or_default().trim().to_string()
    };
    let cpu = one("sysctl", &["-n", "machdep.cpu.brand_string"]);
    let cores = one("sysctl", &["-n", "hw.ncpu"]).parse().unwrap_or(0);
    let ram_bytes: u64 = one("sysctl", &["-n", "hw.memsize"]).parse().unwrap_or(0);
    let os_name = one("sw_vers", &["-productName"]);
    let os_ver = one("sw_vers", &["-productVersion"]);
    let darwin = one("uname", &["-r"]);
    HostInfo {
        model: one("sysctl", &["-n", "hw.model"]),
        cpu,
        cores,
        ram_gb: ram_bytes / (1024 * 1024 * 1024),
        os: format!("{os_name} {os_ver} (Darwin {darwin})"),
        arch: one("uname", &["-m"]),
    }
}

fn git_short_sha(repo_root: &Path) -> String {
    Command::new("git").arg("-C").arg(repo_root).args(["rev-parse", "--short", "HEAD"])
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn whelk_sha(repo_root: &Path) -> String {
    // Parse Cargo.lock for the whelk git source revision.
    let lock = std::fs::read_to_string(repo_root.join("Cargo.lock")).unwrap_or_default();
    let mut in_whelk = false;
    for line in lock.lines() {
        // Reset at each package boundary so we only read whelk's OWN source line;
        // if whelk is ever [patch]ed to a path (no `source =`), we must not fall
        // through and return an unrelated later package's SHA.
        if line.trim() == "[[package]]" { in_whelk = false; }
        if line.trim() == "name = \"whelk\"" { in_whelk = true; }
        if in_whelk
            && let Some(src) = line.trim().strip_prefix("source = ")
            && let Some(hash) = src.rsplit('#').next()
        {
            return hash.trim_matches('"').to_string();
        }
    }
    String::new()
}

pub fn capture_reasoners(
    tools: &Path, rustdl_bin: &Path, repo_root: &Path,
) -> Result<BTreeMap<String, serde_json::Value>> {
    let konclude = tools.join("bin/konclude");
    let robot = tools.join("bin/robot");
    let kon_ver = cmd_output(&konclude.to_string_lossy(), &["-h"])
        .and_then(|s| parse_konclude_version(&s))
        .unwrap_or_else(|| "unknown".into());
    let robot_ver = cmd_output(&robot.to_string_lossy(), &["--version"])
        .and_then(|s| parse_robot_version(&s))
        .unwrap_or_else(|| "unknown".into());
    let rustdl_ver = env!("CARGO_PKG_VERSION").to_string();
    let bin_mtime = std::fs::metadata(rustdl_bin).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());

    let mut m = BTreeMap::new();
    m.insert("rustdl".into(), json!({
        "version": rustdl_ver, "git_sha": git_short_sha(repo_root),
        "binary_mtime_unix": bin_mtime, "build": "release stable-toolchain"
    }));
    m.insert("konclude".into(), json!({
        "version": kon_ver,
        "build": "OSX-x64 via Rosetta 2 (walls/RSS are upper bounds)"
    }));
    m.insert("hermit".into(), json!({ "version": format!("{robot_ver} (ROBOT)"),
        "note": "end-to-end JVM wall (~0.4-1s boot floor, ~240MB baseline)" }));
    m.insert("elk".into(), json!({ "version": format!("{robot_ver} (ROBOT)"),
        "note": "EL-only; end-to-end JVM wall" }));
    m.insert("whelk-rs".into(), json!({ "git_sha": whelk_sha(repo_root), "note": "EL-only" }));
    Ok(m)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_konclude_banner() {
        let banner = "{info} >> Konclude - Uni Ulm Parallel Reasoner\n\
                      {info} >> Reasoner for the SROIQV(D) Description Logic, 64-bit, Version v0.7.0-1138 - 500e11d9 (Jun 18 2021)\n";
        assert_eq!(parse_konclude_version(banner).as_deref(), Some("v0.7.0-1138"));
    }

    #[test]
    fn parses_robot_version() {
        assert_eq!(parse_robot_version("ROBOT version 1.9.10").as_deref(), Some("1.9.10"));
    }

    #[test]
    fn stale_binary_is_rejected() {
        // A binary whose mtime is in the distant past must fail the guard against
        // any repo whose sources are newer.
        let dir = std::env::temp_dir().join(format!("rustdl-fresh-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("crates/x/src")).unwrap();
        std::fs::write(dir.join("crates/x/src/lib.rs"), "// new").unwrap();
        let bin = dir.join("oldbin");
        std::fs::write(&bin, "x").unwrap();
        // Backdate the binary to a date definitively older than any 2026 source file.
        filetime_set(&bin);
        assert!(assert_fresh_binary(&bin, &dir).is_err());
    }

    #[test]
    fn stale_binary_rejected_when_only_root_cargo_toml_is_fresh() {
        // Build-affecting config (e.g. [profile.release] lto/codegen-units) lives
        // ONLY in the workspace-root Cargo.toml — it does not flow through
        // Cargo.lock or any file under crates/. Editing it without rebuilding must
        // still trip the freshness guard.
        let dir =
            std::env::temp_dir().join(format!("rustdl-fresh-roottoml-{}", std::process::id()));
        // A crates/ dir must exist (WalkDir::new on a missing path yields nothing,
        // which is fine) but we deliberately write NO fresh file under it.
        std::fs::create_dir_all(dir.join("crates")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[profile.release]\ncodegen-units = 1\n").unwrap();
        let bin = dir.join("oldbin");
        std::fs::write(&bin, "x").unwrap();
        filetime_set(&bin);
        assert!(assert_fresh_binary(&bin, &dir).is_err());
    }

    // Set mtime without an external crate: macOS `touch -t` (portable BSD/macOS
    // syntax; GNU's `touch -d @<epoch>` is NOT accepted by macOS `touch`).
    fn filetime_set(p: &std::path::Path) {
        std::process::Command::new("touch")
            .arg("-t")
            .arg("202001010000")
            .arg(p)
            .status()
            .ok();
    }
}
