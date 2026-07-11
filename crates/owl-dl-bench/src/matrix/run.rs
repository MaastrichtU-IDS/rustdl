use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct TimedRun {
    pub wall_ms: u64,
    pub peak_rss_mb: u64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Parse the two lines we need from macOS `/usr/bin/time -l` stderr:
///   "        0.17 real         0.04 user         0.04 sys"
///   "            31784960  maximum resident set size"
/// Returns (`wall_ms`, `peak_rss_mb`). RSS floored to whole MiB.
pub fn parse_time_l(stderr: &str) -> Option<(u64, u64)> {
    let mut wall_ms = None;
    let mut rss_mb = None;
    for line in stderr.lines() {
        let t = line.trim();
        if let Some(idx) = t.find(" real")
            && let Ok(secs) = t[..idx].trim().parse::<f64>()
        {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                wall_ms = Some((secs * 1000.0).round() as u64);
            }
        }
        if t.ends_with("maximum resident set size")
            && let Some(num) = t.split_whitespace().next()
            && let Ok(bytes) = num.parse::<u64>()
        {
            rss_mb = Some(bytes / (1024 * 1024));
        }
    }
    match (wall_ms, rss_mb) {
        (Some(w), Some(r)) => Some((w, r)),
        _ => None,
    }
}

/// Run `cmd` (argv, cmd[0] = program) wrapped in
/// `/usr/bin/time -l gtimeout <global_timeout_s> <cmd...>`.
pub fn timed(cmd: &[&str], global_timeout_s: u64) -> Result<TimedRun> {
    let mut c = Command::new("/usr/bin/time");
    c.arg("-l")
        .arg("gtimeout")
        .arg(global_timeout_s.to_string());
    c.args(cmd);
    let out = c.output().with_context(|| format!("spawn {cmd:?}"))?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let code = out.status.code();
    let timed_out = code == Some(124); // gtimeout signals timeout with 124
    let (wall_ms, peak_rss_mb) = parse_time_l(&stderr).unwrap_or((0, 0));
    Ok(TimedRun {
        wall_ms,
        peak_rss_mb,
        exit_code: code,
        timed_out,
        stdout,
        stderr,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn parses_real_time_and_rss() {
        // Verbatim macOS `/usr/bin/time -l` fragment (from a real Konclude run).
        let stderr = "        0.17 real         0.04 user         0.04 sys\n\
                      31784960  maximum resident set size\n\
                      0  peak memory footprint\n";
        let (wall_ms, rss_mb) = parse_time_l(stderr).expect("parsed");
        assert_eq!(wall_ms, 170); // 0.17 s -> 170 ms
        assert_eq!(rss_mb, 30); // 31784960 B -> 30 MiB (floor)
    }

    #[test]
    fn parses_multi_second_wall() {
        let stderr = "        6.39 real        12.10 user         1.02 sys\n\
                      239566848  maximum resident set size\n";
        let (wall_ms, rss_mb) = parse_time_l(stderr).unwrap();
        assert_eq!(wall_ms, 6390);
        assert_eq!(rss_mb, 228); // 239566848 -> 228 MiB
    }

    #[test]
    fn returns_none_without_time_output() {
        assert!(parse_time_l("no timing here\n").is_none());
    }
}
