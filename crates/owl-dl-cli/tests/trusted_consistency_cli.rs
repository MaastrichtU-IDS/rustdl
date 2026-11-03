//! #429: the CLI reports a consistency that rests on the consistency wedge's `Sat`
//! alone on `classify`, `realize`, `justify`, `repair` and `diagnose`: a JSON
//! field `relies_on_trusted_consistency` where the command has `--json`, and one
//! stderr note. Not a give-up: stdout and the exit code do not depend on it.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::process::Command;

/// #392's fixture: off pure EL, an `ABox`, no nominal; the wedge decides it `Sat`.
const ONTO: &str = "Prefix(:=<http://ex/#>)
Ontology(<http://ex/trusted429cli>
  Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
  Declaration(Class(:D)) Declaration(ObjectProperty(:r))
  SubClassOf(:A ObjectUnionOf(:C :D)) SubClassOf(:C :B) SubClassOf(:D :B)
  ClassAssertion(:A :a) ObjectPropertyAssertion(:r :a :b))
";

struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "rustdl-trusted429-{}-{tag}.ofn",
            std::process::id()
        ));
        std::fs::write(&path, ONTO).unwrap();
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Runs the binary with no inherited `RUSTDL_*` settings, plus `env`.
fn run(args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustdl"));
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("RUSTDL_") {
            cmd.env_remove(key);
        }
    }
    cmd.envs(env.iter().copied());
    let out = cmd.args(args).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}

const WEDGE_OFF: (&str, &str) = ("RUSTDL_WEDGE_CONSISTENCY", "0");
const NOTE: &str = "on the consistency wedge's model alone";
const ORACLE_NOTE: &str = "because the consistency wedge found a model";

fn json(r: &Run) -> serde_json::Value {
    assert_eq!(r.code, 0, "{}", r.stderr);
    serde_json::from_str(&r.stdout).unwrap()
}

#[test]
fn classify_and_realize_json_report_it() {
    let f = Fixture::new("cls");
    for cmd in ["classify", "realize"] {
        let on = run(&[cmd, "--json", f.path()], &[]);
        assert_eq!(json(&on)["relies_on_trusted_consistency"], true, "{cmd}");
        assert!(on.stderr.contains(NOTE), "{cmd}: {}", on.stderr);
        let off = run(&[cmd, "--json", f.path()], &[WEDGE_OFF]);
        assert_eq!(json(&off)["relies_on_trusted_consistency"], false, "{cmd}");
        assert!(!off.stderr.contains(NOTE), "{cmd}: {}", off.stderr);
    }
}

#[test]
fn text_mode_notes_on_stderr_only() {
    let f = Fixture::new("text");
    for cmd in ["classify", "realize", "diagnose"] {
        let on = run(&[cmd, f.path()], &[]);
        assert_eq!(on.code, 0, "{cmd}: {}", on.stderr);
        assert!(on.stderr.contains(NOTE), "{cmd}: {}", on.stderr);
        assert!(!on.stdout.contains(NOTE), "{cmd}: the note is not output");
    }
}

#[test]
fn justify_and_repair_report_a_trusted_not_entailed() {
    let f = Fixture::new("just");
    let q = ["subclass", "http://ex/#A", "http://ex/#C"];
    let mut args = vec!["justify", "--json", f.path()];
    args.extend(q);
    let on = run(&args, &[]);
    let v = json(&on);
    assert_eq!(v["status"], "not-entailed");
    assert_eq!(v["relies_on_trusted_consistency"], true);
    assert_eq!(v["incomplete"], false);
    assert!(on.stderr.contains(ORACLE_NOTE), "{}", on.stderr);
    let off = run(&args, &[WEDGE_OFF]);
    assert_eq!(json(&off)["relies_on_trusted_consistency"], false);

    let mut args = vec!["repair", f.path()];
    args.extend(q);
    let on = run(&args, &[]);
    assert_eq!(on.code, 0, "{}", on.stderr);
    assert!(on.stdout.starts_with("not entailed"), "{}", on.stdout);
    assert!(on.stderr.contains(ORACLE_NOTE), "{}", on.stderr);
}
