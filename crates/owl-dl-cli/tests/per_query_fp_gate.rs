//! The PER-QUERY false-positive gate (#505).
//!
//! `owl-dl-reasoner/tests/per_pair_fp_gate.rs` reaches the per-pair surface through
//! `classify` under `RUSTDL_CLASSIFY_VERIFY_REFUTATIONS=1`. The commands themselves
//! (`sat`, `subclass`, `explain`) take another route: no per-pair deadline, the #428
//! guard and #401's per-query consistency, the seeded `ABox`, and the query wedge that
//! only proves. Two false positives (#76, #498) lived on that route while the gate
//! was green. So this file drives the `rustdl` binary itself, one subprocess per
//! call, at default settings, and checks each answer against BOTH oracles.
//!
//! - Every call runs under `ulimit -t 30` (a CPU cap in the child), with a wall-clock
//!   backstop that kills it. A capped call is a give-up: it is reported, and it does
//!   not fail the test. So do MISSes. Only a false positive fails it: a `no`/`unsat`
//!   both oracles contradict, a `yes` on a pair both oracles refute, or an
//!   "inconsistent" on an ontology both oracles find consistent.
//! - `per_query_answers_on_in_repo_fixtures` runs in CI on #498's `c5e` / `q5` and
//!   #76's five-axiom synthetic, with answers adjudicated by HermiT and Konclude when
//!   the fixtures were added. Sabotage: reverting #504 (b3c0f7f6) in `rules.rs` turns
//!   the c5e / q5 cases red; `RUSTDL_MAX_CHOICE_POINT=0 RUSTDL_MAX_TRIAL_MERGE=0` in
//!   the test's environment (inherited by the children) turns the #76 case red.
//! - `per_query_answers_match_both_oracles_on_the_gate_fixtures` (ignored: gitignored
//!   fixtures) runs `sat` on every declared class, and `subclass` + `explain` on every
//!   pair in both oracles' closures and on an equal seeded sample of pairs both
//!   oracles refute (skipping pairs whose sub-class either oracle finds
//!   unsatisfiable, which are trivially `yes`). Pairs the oracles disagree on are
//!   skipped. Fixtures: pizza v2, ro and sulo, from `ontologies/real/`, with Konclude's
//!   `konclude-input/<f>-classified.owx` (`scripts/konclude-oracle.sh`) and HermiT's
//!   `hermit/<f>-classified.ofn` (`scripts/hermit-oracle.sh`). Wine is left out: every
//!   per-query `sat` on it runs past the cap today (#490), so it would only report
//!   give-ups.
//!
//! Run: `cargo test --release -p owl-dl-cli --test per_query_fp_gate -- --include-ignored
//! --nocapture` (the counts and every FP / MISS / give-up go to stderr).
//! `RUSTDL_PQ_GATE_JOBS` sets the ignored test's parallelism (default: the available
//! cores, at most 8). The children inherit the environment, so a stray `RUSTDL_*`
//! variable would change "default settings": the tests fail on one unless
//! `RUSTDL_PQ_GATE_ALLOW_ENV=1` (which the sabotage runs set). A crash, a panic, an
//! error exit or unparsable output is an error, and fails both tests. Unix only: the
//! CPU cap uses `/bin/sh`'s `ulimit`.

#![cfg(unix)]
#![allow(clippy::doc_markdown)]

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use horned_owl::io::ParserConfiguration;
use horned_owl::io::ofn::reader::read as read_ofn;
use horned_owl::model::{Component, DeclareClass, RcStr};
use horned_owl::ontology::set::SetOntology;
use owl_dl_reasoner::oracle_diff::read_owx_verdict;

const CPU_CAP_S: u64 = 30;
const WALL_CAP: Duration = Duration::from_secs(120);
const OWL_THING: &str = "http://www.w3.org/2002/07/owl#Thing";
const OWL_NOTHING: &str = "http://www.w3.org/2002/07/owl#Nothing";

/// One command's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    /// `sat` / `subclass yes` / `explain … : yes`.
    Yes,
    /// `unsat` / `subclass no` / `explain … : no`.
    No,
    /// The command reported the ontology inconsistent.
    Inconsistent,
    /// No verdict: `unknown` (exit 3), or the CPU or wall cap.
    GaveUp(String),
    /// A crash, a panic, another error exit, or output the parser does not know.
    Errored(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Sat,
    Subclass,
    Explain,
}

/// One call and the answer both oracles agree on.
#[derive(Debug, Clone)]
struct Job {
    cmd: Cmd,
    file: PathBuf,
    args: Vec<String>,
    /// `true`: satisfiable / subsumed. `false`: unsatisfiable / not subsumed.
    expect: bool,
}

/// Runs `rustdl <cmd> <file> <args…>` under a CPU cap and a wall backstop.
fn run(job: &Job) -> Answer {
    let name = match job.cmd {
        Cmd::Sat => "sat",
        Cmd::Subclass => "subclass",
        Cmd::Explain => "explain",
    };
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("ulimit -t {CPU_CAP_S} && exec \"$0\" \"$@\""))
        .arg(env!("CARGO_BIN_EXE_rustdl"))
        .arg(name)
        .arg(&job.file)
        .args(&job.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rustdl");
    let mut out = child.stdout.take().expect("stdout");
    let mut err = child.stderr.take().expect("stderr");
    let out_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = err.read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().expect("wait") {
            break Some(st);
        }
        if start.elapsed() > WALL_CAP {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    let Some(status) = status else {
        return Answer::GaveUp(format!("wall cap {}s", WALL_CAP.as_secs()));
    };
    if let Some(sig) = std::os::unix::process::ExitStatusExt::signal(&status) {
        // `ulimit -t` delivers SIGXCPU (24) at the limit, then SIGKILL (9).
        return if sig == 24 || sig == 9 {
            Answer::GaveUp(format!("CPU cap {CPU_CAP_S}s (signal {sig})"))
        } else {
            Answer::Errored(format!(
                "signal {sig}: {}",
                stderr.lines().last().unwrap_or("")
            ))
        };
    }
    if status.code() == Some(1)
        && stderr
            .lines()
            .any(|l| l.trim() == "ontology is inconsistent; every assertion is trivially entailed")
    {
        return Answer::Inconsistent;
    }
    let first = stdout.lines().next().unwrap_or("").trim();
    let verdict = match job.cmd {
        Cmd::Sat => match first {
            "sat" => Some(Answer::Yes),
            "unsat" => Some(Answer::No),
            _ => None,
        },
        Cmd::Subclass => match first {
            "yes" => Some(Answer::Yes),
            "no" => Some(Answer::No),
            _ => None,
        },
        Cmd::Explain => {
            if first.contains(" : yes") {
                Some(Answer::Yes)
            } else if first.contains(" : no") {
                Some(Answer::No)
            } else {
                None
            }
        }
    };
    match (verdict, status.code()) {
        (Some(v), Some(0)) => v,
        (None, Some(3)) if first == "unknown" => Answer::GaveUp("unknown (exit 3)".to_owned()),
        (_, code) => Answer::Errored(format!(
            "exit {code:?}: {first} {}",
            stderr.lines().last().unwrap_or("")
        )),
    }
}

/// FPs, MISSes and give-ups, each as a line naming the call.
#[derive(Default)]
struct Report {
    calls: usize,
    fps: Vec<String>,
    misses: Vec<String>,
    give_ups: Vec<String>,
    errors: Vec<String>,
}

fn describe(job: &Job) -> String {
    format!(
        "{:?} {} {}",
        job.cmd,
        job.file
            .file_name()
            .map_or_else(String::new, |f| f.to_string_lossy().into_owned()),
        job.args.join(" ")
    )
}

/// Every answer is judged against `expect`, which both oracles agree on; every
/// fixture here is consistent in both, so "inconsistent" is a false positive.
fn judge(job: &Job, got: &Answer, report: &mut Report) {
    report.calls += 1;
    let what = describe(job);
    match (got, job.expect) {
        (Answer::Yes, true) | (Answer::No, false) => {}
        (Answer::No, true) => {
            if job.cmd == Cmd::Sat {
                report
                    .fps
                    .push(format!("{what}: unsat, both oracles find it satisfiable"));
            } else {
                report
                    .misses
                    .push(format!("{what}: no, both oracles entail it"));
            }
        }
        (Answer::Yes, false) => {
            if job.cmd == Cmd::Sat {
                report
                    .misses
                    .push(format!("{what}: sat, both oracles find it unsatisfiable"));
            } else {
                report
                    .fps
                    .push(format!("{what}: yes, both oracles refute it"));
            }
        }
        (Answer::Inconsistent, _) => report.fps.push(format!(
            "{what}: inconsistent, both oracles find the ontology consistent"
        )),
        (Answer::GaveUp(why), _) => report.give_ups.push(format!("{what}: {why}")),
        (Answer::Errored(why), _) => report.errors.push(format!("{what}: {why}")),
    }
}

fn run_all(jobs: &[Job], workers: usize) -> Report {
    let next = AtomicUsize::new(0);
    let report = Mutex::new(Report::default());
    std::thread::scope(|s| {
        for _ in 0..workers.max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    let got = run(job);
                    judge(job, &got, &mut report.lock().expect("report lock"));
                }
            });
        }
    });
    report.into_inner().expect("report lock")
}

fn print_report(label: &str, r: &Report) {
    eprintln!(
        "[pq-fp] {label}: {} calls, {} FP, {} MISS, {} give-up, {} error",
        r.calls,
        r.fps.len(),
        r.misses.len(),
        r.give_ups.len(),
        r.errors.len()
    );
    for l in &r.errors {
        eprintln!("[pq-fp]   ERROR {l}");
    }
    for l in &r.fps {
        eprintln!("[pq-fp]   FP    {l}");
    }
    for l in &r.misses {
        eprintln!("[pq-fp]   MISS  {l}");
    }
    for l in &r.give_ups {
        eprintln!("[pq-fp]   GAVE UP {l}");
    }
}

fn workspace_path(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// Fails on a stray `RUSTDL_*` variable (it would reach every child) unless the run
/// opts in with `RUSTDL_PQ_GATE_ALLOW_ENV=1`, and prints whatever is set.
fn require_default_settings() {
    let set: Vec<String> = std::env::vars()
        .filter(|(k, _)| k.starts_with("RUSTDL_") && !k.starts_with("RUSTDL_PQ_GATE_"))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    if !set.is_empty() {
        eprintln!("[pq-fp] children inherit: {}", set.join(" "));
        assert!(
            std::env::var("RUSTDL_PQ_GATE_ALLOW_ENV").is_ok_and(|v| v == "1"),
            "[pq-fp] {set:?} would change the commands' default settings; unset them, or set \
             RUSTDL_PQ_GATE_ALLOW_ENV=1 for a deliberate run (a sabotage)"
        );
    }
}

// ---------------------------------------------------------------------------
// The in-repo arm (CI).

fn sat_job(file: &Path, class: &str, expect: bool) -> Job {
    Job {
        cmd: Cmd::Sat,
        file: file.to_owned(),
        args: vec![class.to_owned()],
        expect,
    }
}

fn pair_jobs(file: &Path, sub: &str, sup: &str, expect: bool) -> [Job; 2] {
    [Cmd::Subclass, Cmd::Explain].map(|cmd| Job {
        cmd,
        file: file.to_owned(),
        args: vec![sub.to_owned(), sup.to_owned()],
        expect,
    })
}

/// #498's `c5e` / `q5` and #76's synthetic. The expected answers were adjudicated
/// by HermiT (`-k`, `-U`, `-S`) and Konclude when the fixtures were added: every
/// class is satisfiable in all three, `E ⋢ Zb` in c5e / q5, and `A ⋢ I` in the
/// synthetic (`A` has only `owl:Thing` above it).
#[test]
fn per_query_answers_on_in_repo_fixtures() {
    require_default_settings();
    let mut jobs = Vec::new();
    for name in ["c5e", "q5"] {
        let file = workspace_path(&format!(
            "crates/owl-dl-reasoner/tests/fixtures/issue498/{name}.ofn"
        ));
        let ns = format!("http://example.org/{name}#");
        for c in ["E", "Z", "Nz", "Zb", "Dummy1", "Dummy2"] {
            jobs.push(sat_job(&file, &format!("{ns}{c}"), true));
        }
        jobs.extend(pair_jobs(
            &file,
            &format!("{ns}E"),
            &format!("{ns}Zb"),
            false,
        ));
    }
    let file = workspace_path("crates/owl-dl-cli/tests/fixtures/laws/maxmerge_fp.ofn");
    for c in ["A", "M", "T", "PT", "I"] {
        jobs.push(sat_job(&file, &format!("http://ex.org/p#{c}"), true));
    }
    jobs.extend(pair_jobs(
        &file,
        "http://ex.org/p#A",
        "http://ex.org/p#I",
        false,
    ));
    let report = run_all(&jobs, 4);
    print_report("in-repo", &report);
    assert!(
        report.give_ups.is_empty() && report.errors.is_empty(),
        "an in-repo call gave up or failed; these decide in milliseconds: {:#?} {:#?}",
        report.give_ups,
        report.errors
    );
    assert!(
        report.fps.is_empty(),
        "per-query false positive on an in-repo fixture: {:#?}",
        report.fps
    );
}

// ---------------------------------------------------------------------------
// The gate-fixture arm (ignored: needs the gitignored fixtures and oracles).

/// One oracle's verdict: the strict subsumption closure, the unsatisfiable
/// classes and the classes equivalent to `owl:Thing`.
struct Oracle {
    closure: BTreeSet<(String, String)>,
    unsat: BTreeSet<String>,
    thing: BTreeSet<String>,
}

fn closure_of(edges: &BTreeSet<(String, String)>) -> BTreeSet<(String, String)> {
    let mut up: std::collections::BTreeMap<&str, Vec<&str>> = std::collections::BTreeMap::new();
    for (a, b) in edges {
        up.entry(a).or_default().push(b);
    }
    let mut out = BTreeSet::new();
    for &start in up.keys() {
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(x) = stack.pop() {
            for &y in up.get(x).map_or(&[][..], Vec::as_slice) {
                if seen.insert(y) {
                    stack.push(y);
                }
            }
        }
        for y in seen {
            if y != start {
                out.insert((start.to_owned(), y.to_owned()));
            }
        }
    }
    out
}

fn konclude(path: &Path) -> Oracle {
    let v = read_owx_verdict(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Oracle {
        closure: closure_of(&v.edges),
        unsat: v.unsat,
        thing: v.thing_equiv,
    }
}

fn hermit(path: &Path) -> Oracle {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse_hermit(&src).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// HermiT's `-c` output: one `SubClassOf( <a> <b> )` or `EquivalentClasses( … )`
/// per line, full IRIs. Any other non-blank line is an error, not a skip: a format
/// drift must not quietly empty the oracle.
fn parse_hermit(src: &str) -> Result<Oracle, String> {
    let (mut edges, mut unsat, mut thing) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let iris: Vec<String> = line
            .split('<')
            .skip(1)
            .filter_map(|s| s.split('>').next())
            .map(str::to_owned)
            .collect();
        if line.starts_with("SubClassOf(") && iris.len() == 2 {
            if iris[1] == OWL_NOTHING {
                unsat.insert(iris[0].clone());
            } else if iris[0] != OWL_NOTHING && iris[1] != OWL_THING {
                edges.insert((iris[0].clone(), iris[1].clone()));
            }
        } else if line.starts_with("EquivalentClasses(") && iris.len() >= 2 {
            let others = iris.iter().filter(|i| *i != OWL_NOTHING && *i != OWL_THING);
            if iris.iter().any(|i| i == OWL_NOTHING) {
                unsat.extend(others.cloned());
            } else if iris.iter().any(|i| i == OWL_THING) {
                thing.extend(others.cloned());
            } else {
                for a in &iris {
                    for b in &iris {
                        if a != b {
                            edges.insert((a.clone(), b.clone()));
                        }
                    }
                }
            }
        } else {
            return Err(format!("unrecognised HermiT line: {line}"));
        }
    }
    Ok(Oracle {
        closure: closure_of(&edges),
        unsat,
        thing,
    })
}

fn declared_classes(path: &Path) -> Vec<String> {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (onto, _): (SetOntology<RcStr>, _) = read_ofn(
        &mut std::io::Cursor::new(src),
        ParserConfiguration::default(),
    )
    .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut out: Vec<String> = onto
        .iter()
        .filter_map(|ac| match &ac.component {
            Component::DeclareClass(DeclareClass(c)) => Some(c.0.to_string()),
            _ => None,
        })
        .filter(|c| c != OWL_THING && c != OWL_NOTHING)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// A small deterministic generator for the non-pair sample (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// One fixture's jobs and the counts behind them.
struct GateJobs {
    jobs: Vec<Job>,
    positives: usize,
    disputed_classes: usize,
    disputed_pairs: usize,
}

/// The jobs for one fixture: `sat` on every class both oracles agree on, and
/// `subclass` + `explain` on every pair both entail (sub-class satisfiable in both)
/// and on an equal seeded sample of pairs both refute (sub satisfiable in both, sup
/// not `owl:Thing`-equivalent in either).
fn gate_jobs(file: &Path, classes: &[String], k: &Oracle, h: &Oracle, seed: u64) -> GateJobs {
    let mut jobs = Vec::new();
    let mut disputed_classes = 0;
    for c in classes {
        match (k.unsat.contains(c), h.unsat.contains(c)) {
            (false, false) => jobs.push(sat_job(file, c, true)),
            (true, true) => jobs.push(sat_job(file, c, false)),
            _ => disputed_classes += 1,
        }
    }
    let either_unsat = |c: &String| k.unsat.contains(c) || h.unsat.contains(c);
    let either_thing = |c: &String| k.thing.contains(c) || h.thing.contains(c);
    let positives: Vec<&(String, String)> = k
        .closure
        .intersection(&h.closure)
        .filter(|(a, _)| !either_unsat(a))
        .collect();
    let disputed_pairs = k.closure.symmetric_difference(&h.closure).count();
    for (a, b) in &positives {
        jobs.extend(pair_jobs(file, a, b, true));
    }
    let mut negatives: Vec<(String, String)> = Vec::new();
    for a in classes {
        if either_unsat(a) {
            continue;
        }
        for b in classes {
            if a == b || either_thing(b) {
                continue;
            }
            let p = (a.clone(), b.clone());
            if !k.closure.contains(&p) && !h.closure.contains(&p) {
                negatives.push(p);
            }
        }
    }
    // A zero state stays zero; any other seed is used as given.
    let mut rng = Rng(seed.max(1));
    let want = positives.len().min(negatives.len());
    for i in 0..want {
        let j = i + usize::try_from(rng.next() % (negatives.len() - i) as u64).expect("index");
        negatives.swap(i, j);
    }
    for (a, b) in &negatives[..want] {
        jobs.extend(pair_jobs(file, a, b, false));
    }
    GateJobs {
        jobs,
        positives: positives.len(),
        disputed_classes,
        disputed_pairs,
    }
}

#[test]
#[ignore = "needs the gitignored ontologies/real/{pizza,ro,sulo}.ofn, konclude-input/*-classified.owx \
            and hermit/*-classified.ofn; runs a few thousand rustdl subprocesses"]
fn per_query_answers_match_both_oracles_on_the_gate_fixtures() {
    require_default_settings();
    let workers = std::env::var("RUSTDL_PQ_GATE_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get().min(8)));
    let base = workspace_path("ontologies/real");
    let (mut fps, mut errors) = (Vec::new(), Vec::new());
    for (i, name) in ["pizza", "ro", "sulo"].iter().enumerate() {
        let file = base.join(format!("{name}.ofn"));
        let kfile = base.join(format!("konclude-input/{name}-classified.owx"));
        let hfile = base.join(format!("hermit/{name}-classified.ofn"));
        for p in [&file, &kfile, &hfile] {
            assert!(
                p.exists(),
                "[pq-fp] REQUIRED fixture {} is missing: fetch the ontologies with \
                 scripts/fetch-real-ontologies.sh and generate the oracles with \
                 scripts/konclude-oracle.sh and scripts/hermit-oracle.sh",
                p.display()
            );
        }
        if *name == "pizza" {
            let src = std::fs::read_to_string(&file).expect("read pizza");
            assert!(
                src.contains("Margherita") && src.contains("InterestingPizza"),
                "[pq-fp] {} is not the Stanford pizza v2.0 (#76's fixture)",
                file.display()
            );
        }
        let (k, h) = (konclude(&kfile), hermit(&hfile));
        let classes = declared_classes(&file);
        let seed = 505 + u64::try_from(i).expect("index");
        let g = gate_jobs(&file, &classes, &k, &h, seed);
        eprintln!(
            "[pq-fp] {name}: {} calls ({} classes; {} entailed pairs and as many refuted ones, \
             each by subclass and explain), {} disputed classes and {} disputed pairs skipped, \
             seed {seed}",
            g.jobs.len(),
            classes.len(),
            g.positives,
            g.disputed_classes,
            g.disputed_pairs
        );
        // A drifted or empty oracle would run no pair at all and pass.
        assert!(
            !classes.is_empty() && g.positives > 0,
            "[pq-fp] {name}: {} classes, {} entailed pairs: an oracle or the fixture is empty",
            classes.len(),
            g.positives
        );
        let report = run_all(&g.jobs, workers);
        print_report(name, &report);
        fps.extend(report.fps.into_iter().map(|l| format!("{name}: {l}")));
        errors.extend(report.errors.into_iter().map(|l| format!("{name}: {l}")));
    }
    assert!(
        fps.is_empty() && errors.is_empty(),
        "per-query false positives / errors on the gate fixtures: {fps:#?} {errors:#?}"
    );
}

/// The oracle parser and the job builder, on a synthetic pair of oracles (no
/// fixtures needed): the equivalence star, the `owl:Nothing` / `owl:Thing` groups,
/// `SubClassOf(X owl:Nothing)`, a rejected line, the skip rules and the disputed
/// accounting.
#[test]
fn oracle_parsing_and_job_selection() {
    let iri = |c: &str| format!("http://t#{c}");
    let line = |kind: &str, cs: &[&str]| {
        let parts: Vec<String> = cs
            .iter()
            .map(|c| match *c {
                "Thing" => format!("<{OWL_THING}>"),
                "Nothing" => format!("<{OWL_NOTHING}>"),
                c => format!("<{}>", iri(c)),
            })
            .collect();
        format!("{kind}( {} )", parts.join(" "))
    };
    let src = [
        line("SubClassOf", &["A", "B"]),
        line("SubClassOf", &["B", "C"]),
        line("EquivalentClasses", &["C", "C2"]),
        line("EquivalentClasses", &["Nothing", "U"]),
        line("SubClassOf", &["U2", "Nothing"]),
        line("EquivalentClasses", &["Thing", "T"]),
    ]
    .join("\n");
    let h = parse_hermit(&src).expect("parses");
    for (a, b) in [
        ("A", "B"),
        ("A", "C"),
        ("A", "C2"),
        ("B", "C2"),
        ("C2", "C"),
    ] {
        assert!(
            h.closure.contains(&(iri(a), iri(b))),
            "{a} ⊑ {b} in the closure"
        );
    }
    assert!(!h.closure.contains(&(iri("B"), iri("A"))));
    assert_eq!(h.unsat, BTreeSet::from([iri("U"), iri("U2")]));
    assert_eq!(h.thing, BTreeSet::from([iri("T")]));
    assert!(parse_hermit("Declaration( Class( <http://t#A> ) )").is_err());

    // Konclude agrees except on `A ⊑ C2` (and so on `A ⊑ C`): two disputed pairs.
    let mut k = parse_hermit(&src).expect("parses");
    k.closure.remove(&(iri("A"), iri("C2")));
    k.closure.remove(&(iri("A"), iri("C")));
    let classes: Vec<String> = ["A", "B", "C", "C2", "U", "U2", "T", "X"]
        .iter()
        .map(|c| iri(c))
        .collect();
    let g = gate_jobs(Path::new("t.ofn"), &classes, &k, &h, 7);
    assert_eq!(g.disputed_pairs, 2);
    assert_eq!(g.disputed_classes, 0);
    // Entailed in both: A⊑B, B⊑C, B⊑C2, C⊑C2, C2⊑C.
    assert_eq!(g.positives, 5);
    let pairs: Vec<(&str, &str, bool)> = g
        .jobs
        .iter()
        .filter(|j| j.cmd == Cmd::Subclass)
        .map(|j| (j.args[0].as_str(), j.args[1].as_str(), j.expect))
        .collect();
    assert_eq!(pairs.iter().filter(|p| p.2).count(), 5);
    assert_eq!(
        pairs.iter().filter(|p| !p.2).count(),
        5,
        "as many refuted pairs"
    );
    for (a, b, expect) in &pairs {
        if !expect {
            assert!(
                !h.unsat.contains(*a) && !h.thing.contains(*b),
                "a refuted pair never has an unsat sub or a Thing-equivalent sup: {a} {b}"
            );
            assert!(!k.closure.contains(&((*a).to_owned(), (*b).to_owned())));
        }
    }
    let sat: Vec<(&str, bool)> = g
        .jobs
        .iter()
        .filter(|j| j.cmd == Cmd::Sat)
        .map(|j| (j.args[0].as_str(), j.expect))
        .collect();
    assert_eq!(sat.len(), classes.len());
    assert!(sat.contains(&(iri("U").as_str(), false)) && sat.contains(&(iri("X").as_str(), true)));
}
