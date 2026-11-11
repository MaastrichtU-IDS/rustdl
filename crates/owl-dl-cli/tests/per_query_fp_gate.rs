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
//! `RUSTDL_PQ_GATE_JOBS` sets the parallelism of the ignored test (default: the
//! available cores, at most 8). Unix only: the CPU cap uses `/bin/sh`'s `ulimit`.

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
    /// No verdict: `unknown`, the CPU or wall cap, or an error.
    GaveUp(String),
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
        .arg(format!("ulimit -t {CPU_CAP_S}; exec \"$0\" \"$@\""))
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
    if status.code().is_none() {
        return Answer::GaveUp(format!("killed (CPU cap {CPU_CAP_S}s)"));
    }
    if stderr.contains("ontology is inconsistent") {
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
    verdict.unwrap_or_else(|| {
        Answer::GaveUp(format!(
            "rc {:?}: {} {}",
            status.code(),
            first,
            stderr.lines().last().unwrap_or("")
        ))
    })
}

/// FPs, MISSes and give-ups, each as a line naming the call.
#[derive(Default)]
struct Report {
    calls: usize,
    fps: Vec<String>,
    misses: Vec<String>,
    give_ups: Vec<String>,
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
        "[pq-fp] {label}: {} calls, {} FP, {} MISS, {} give-up",
        r.calls,
        r.fps.len(),
        r.misses.len(),
        r.give_ups.len()
    );
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
        report.give_ups.is_empty(),
        "an in-repo fixture gave up; these decide in milliseconds, so a cap is a defect here"
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

/// HermiT's `-c` output: one `SubClassOf( <a> <b> )` or `EquivalentClasses( … )`
/// per line, full IRIs.
fn hermit(path: &Path) -> Oracle {
    let src =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (mut edges, mut unsat, mut thing) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for line in src.lines() {
        let iris: Vec<String> = line
            .split('<')
            .skip(1)
            .filter_map(|s| s.split('>').next())
            .map(str::to_owned)
            .collect();
        if line.starts_with("SubClassOf(") && iris.len() == 2 {
            if iris[0] != OWL_NOTHING && iris[1] != OWL_THING {
                edges.insert((iris[0].clone(), iris[1].clone()));
            }
        } else if line.starts_with("EquivalentClasses(") {
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
        }
    }
    Oracle {
        closure: closure_of(&edges),
        unsat,
        thing,
    }
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

/// The jobs for one fixture, and how many pairs were skipped as disputed.
fn gate_jobs(file: &Path, k: &Oracle, h: &Oracle, seed: u64) -> (Vec<Job>, usize, usize) {
    let classes = declared_classes(file);
    let mut jobs = Vec::new();
    let mut disputed = 0;
    for c in &classes {
        match (k.unsat.contains(c), h.unsat.contains(c)) {
            (false, false) => jobs.push(sat_job(file, c, true)),
            (true, true) => jobs.push(sat_job(file, c, false)),
            _ => disputed += 1,
        }
    }
    let either_unsat = |c: &String| k.unsat.contains(c) || h.unsat.contains(c);
    let either_thing = |c: &String| k.thing.contains(c) || h.thing.contains(c);
    let positives: Vec<&(String, String)> = k
        .closure
        .intersection(&h.closure)
        .filter(|(a, _)| !either_unsat(a))
        .collect();
    disputed += k.closure.symmetric_difference(&h.closure).count();
    for (a, b) in &positives {
        jobs.extend(pair_jobs(file, a, b, true));
    }
    let mut negatives: Vec<(String, String)> = Vec::new();
    for a in &classes {
        if either_unsat(a) {
            continue;
        }
        for b in &classes {
            if a == b || either_thing(b) {
                continue;
            }
            let p = (a.clone(), b.clone());
            if !k.closure.contains(&p) && !h.closure.contains(&p) {
                negatives.push(p);
            }
        }
    }
    let mut rng = Rng(seed | 1);
    let want = positives.len().min(negatives.len());
    for i in 0..want {
        let j = i + usize::try_from(rng.next() % (negatives.len() - i) as u64).expect("index");
        negatives.swap(i, j);
    }
    for (a, b) in &negatives[..want] {
        jobs.extend(pair_jobs(file, a, b, false));
    }
    (jobs, positives.len(), disputed)
}

#[test]
#[ignore = "needs the gitignored ontologies/real/{pizza,ro,sulo}.ofn, konclude-input/*-classified.owx \
            and hermit/*-classified.ofn; runs a few thousand rustdl subprocesses"]
fn per_query_answers_match_both_oracles_on_the_gate_fixtures() {
    let workers = std::env::var("RUSTDL_PQ_GATE_JOBS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get().min(8)));
    let base = workspace_path("ontologies/real");
    let mut fps = Vec::new();
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
        let seed = 505 + u64::try_from(i).expect("index");
        let (jobs, positives, disputed) = gate_jobs(&file, &k, &h, seed);
        eprintln!(
            "[pq-fp] {name}: {} calls ({positives} entailed pairs and as many refuted ones, \
             each by subclass and explain; sat on every class), {disputed} disputed skipped, \
             seed {seed}",
            jobs.len()
        );
        let report = run_all(&jobs, workers);
        print_report(name, &report);
        fps.extend(report.fps.into_iter().map(|l| format!("{name}: {l}")));
    }
    assert!(
        fps.is_empty(),
        "per-query false positives on the gate fixtures: {fps:#?}"
    );
}
