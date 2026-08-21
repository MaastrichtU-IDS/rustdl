# RESUME HERE — 2026-08-21 shutdown handoff

Everything is committed. Nothing is lost. Four branches, none pushed, none merged.

## The one thing that must not be forgotten

**A fix the controller directed onto `main` was WRONG, and applying it would have created a
false positive.** It was caught before landing. Details: `probe-fix-was-wrong.md`.

`classify.rs`'s `unsatisfiable_idxs` is *named* as if it holds report positions, and its
**consumers** read it that way — but all three **producers** (`classify.rs:1323`, `:1511`,
`:3656`) insert `i` after deciding the satisfiability of `ClassId::new(i)`. So the set is
`{ i ∈ 0..n : ClassId(i) unsat }` — raw ids clipped to `< n`. Against those semantics the
existing `unsatisfiable_idxs.contains(&(c.index() as usize))` is **exact**, and the clip can
only cause a miss, never a false positive.

Rewriting it to an IRI/report-position lookup compares a report position against raw ids —
unsound exactly when a DKey sits below a user class. Measured: stock `main` answers correctly
on the fixture; with the "fix" applied, the KB is reported inconsistent with all four classes
unsatisfiable.

**Consequence: `origin/main` has NO live false positive from this site. Earlier claims that it
did are retracted.**

## OPEN QUESTION — check this first

`fix/dkey-id-aliasing-on-main` @ `002c7e8` contains a commit titled *"two NEW
report-position/ClassId conflations in main's code"*. Its "offender 1" is **exactly** the
probe-end-only change now shown to be unsound in isolation.

**Verify whether `002c7e8` changed only the probe end or both ends.** If only the probe end,
that branch carries a regression and must not merge as-is. The correct fix changes producers
and consumers **together** — that is `ReportedClasses`' job.

```sh
git show 002c7e8 -- crates/owl-dl-reasoner/src/classify.rs | grep -n -B4 -A8 'unsatisfiable_idxs'
```

## Branch inventory

| branch | tip | ahead of origin/main | state |
|---|---|---|---|
| `fix/classify-consistency-probe-aliasing` | `31b3cf6` | 1 | doc fix + 199-line regression canary. **Safe.** No behaviour change. |
| `fix/dkey-id-aliasing-on-main` | `002c7e8` | 5 | DKey fix rebased onto main, 1702 tests green — **but see OPEN QUESTION.** |
| `fix/dkey-id-aliasing` | `0cc64ac` | 17 | original DKey fix on the old `#48` base. Superseded by the above; keep as reference. Safety tag `premerge-dkey`. |
| `feat/incremental-reasoning-p1` | `8f317dc` | 39 | P1 incremental reasoning, complete + reviewed. Needs the same rebase off unmerged `#48`. |

`origin/main` is `b796bec`. Local `main` is 390 commits behind — `git pull` it before anything.
`feat/complex-class-expression-queries-48` was **never merged**; both DKey branches and P1
originally forked from it.

## Still true and still valuable

- **DKey id aliasing is a real bug on `main`** — `reportable_class_iris` filters DKey IRIs out of
  a `0..num_classes()` enumeration while ~29 sites map the report index back to a class id.
  Trigger: a **used-but-undeclared** class (declared classes always intern before DKeys, since
  `DeclareClass` sorts first). Full account: `dkey-fix-report.md`, `rebase-onto-main.md`.
- **A shipped source-scanning guard was passing vacuously** — `split_once("\n#[cfg(test)]")`
  truncated at line 1039, scanning 1,038 of 6,800 lines. Hardened with a coverage floor on
  `fix/dkey-id-aliasing-on-main`. Any other source-scanning check in this repo that truncates at
  `#[cfg(test)]` has the same failure mode.
- **`classify.rs:1564`'s doc said the consistency probe is "default OFF".** It is default-ON
  (`lib.rs:2469`, house convention). Corrected on the hotfix branch.
- **P1 incremental reasoning works**: 2.14× on GO-basic (51,986 classes, pure EL, `tableau=0`,
  `closure_answered=101`, 99/100 additions reused). See
  `docs/2026-08-20-go-basic-incremental-measurement.md` — including the correction that the
  lowering floor's share does **not** keep falling with size, so the ~13× galen ceiling does not
  extrapolate (real ceiling at GO scale ≈ 2.9×).

## Owner decisions still outstanding

1. Corpus FP=0 / MISSED=0 re-validation for the DKey branch. Expect **no** `bench-corpus` delta
   (measured across three independently built binaries with a diffing sentinel).
2. `Subsumers::unsatisfiable_bitset()` was **deleted** on the DKey branches — the only
   semver-visible change. Legal at 0.4.x in a minor bump, not a patch. One-line revert is to
   rename it `unsatisfiable_bitset_by_class_id` instead.
3. Whether to do the `ReportIdx` newtype. Three defects of this class have now shipped in
   `classify.rs` and **none** was caught by reading or by a source-level guard — each needed a
   behavioural oracle or an execution. A newtype would have made all three a compile error.

## Process note worth keeping

Across this work, **nine tests turned out vacuous** — green while proving nothing — every one
caught only by constructing and running a mutation. Two were in an identity gate; one was our
own source-scanning guard. The practice that worked: for every load-bearing assertion, build the
mutation that should break it and show that it does. Treat a test that cannot fail as a finding.
