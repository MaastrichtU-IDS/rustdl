# Parse is the wall on the largest ontologies — cost localised to COMPARISON, caller NOT yet found

**Status: partial. A real lever is confirmed and sized; its dominant cost is characterised but the
call site is unidentified.** Recorded so the next attempt starts from evidence rather than repeating
this.

## The lever is real and neither v0.4.22 fix touches it

`ore_ont_10926` is **557 MB** and its wall is parse-dominated:

| phase | ms |
|---|---:|
| parse | **17,276** |
| convert | 13,189 |
| — of which `build_told_tables` | **55** |

So the O(n²) memset fix (v0.4.22) and the told-sharing fix are both **inert here** — told is 0.4% of
its conversion. ~32 MB/s of parse throughput is the headline number.

Parse is also **not deadline-bound** (no budget bounds it, and `--global-timeout-ms` charges from
after parse), so a saving here converts **1:1 into wall** — the same property that made the
conversion bucket worth attacking.

## The cost is COMPARISON, not parsing

`perf` on `locality-stats` (parse + convert + locality), 4,761 samples:

| frame | share |
|---|---:|
| `ClassExpression::partial_cmp` | **12.34%** |
| `Component::partial_cmp` | 5.52% |
| `__memcmp_avx2_movbe` | 5.10% |
| **ordering subtotal** | **~23%** |
| pest (`Pairs::next`, `ParserState::{rule,match_range,sequence}`, `LineIndex::new`, `into_inner`) | ~14% |
| hashing (`sip::Hasher::write` ×2, `ClassExpression::hash`) | ~5.8% |
| `convert_ontology` + `convert_class_expression` | ~3.9% |

**Ordering costs more than the actual parsing.** The `memcmp` is consistent with deep recursive
comparison bottoming out in `Rc<str>` IRIs. Note `ClassExpression::eq` is only **0.23%** — three
orders of magnitude below `partial_cmp` — so this is genuinely *ordering*, not hash-set equality.

## Hypotheses ELIMINATED (do not re-check these)

1. **rustdl's own sorts** — `ir.rs:381/413` sort `ConceptId` (u32) for interning canonicalisation;
   `told.rs`/`role_hierarchy.rs` sort `ClassId`/`RoleId`; the `convert.rs` `BTreeSet`s are small DKey
   literal decodings. None touch a horned-owl type.
2. **`SetOntology` insertion** — `SetIndex` is `HashSet<AA>` in this checkout (verified at
   `ontology/set.rs:198`), so inserts hash and compare with `eq`, which is 0.23%.
3. **horned-owl's `set.rs` `v.sort()` calls** (lines 345/404/447/503/562) — all inside `#[test]`
   functions, not the production path.
4. **`Build`'s IRI interning cache** — already a `HashSet`, with an in-tree comment recording the
   change ("was BTreeSet").
5. **`ComponentMappedOntology`'s `BTreeMap<ComponentKind, BTreeSet<AA>>`** — that container *would*
   explain it, but the classify path parses into `SetOntology`, not `ComponentMapped`
   (`owl-dl-cli/src/main.rs:576`). Only `json_out.rs` uses `ComponentMapped`.

## Why it is unresolved, and the cheapest next step

`perf report` cannot symbolize a callgraph for this input inside a 9-minute budget — attempted twice,
once with `--call-graph=fp` (self-time only, no useful caller chain: `partial_cmp` shows 7.53%
*recursing into itself* and no external frame) and once with `--call-graph=dwarf,2048` at `-F 29`
(1.8 MB of data, report still timed out).

**Next step: reproduce on a SMALLER input where dwarf symbolization is affordable, then read the
caller chain.** Pick an ontology large enough that `ClassExpression::partial_cmp` is still visible in
a flat profile but small enough to symbolize — a few tens of MB. Confirm the frame is present before
spending time on the callgraph.

Two secondary observations worth keeping:

* `pest::iterators::line_index::LineIndex::new` costs **1.60%**, and a line index exists for *error
  reporting*. On a 557 MB file that is a real cost paid on the success path. Worth checking whether
  it can be built lazily.
* The remaining pest share (~14%) is genuine parsing work and is the floor for any front-end fix.

---

## Session 2 (2026-08-23): two more hypotheses eliminated; the blocker is TOOLING, not knowledge

Followed the recommended next step — reproduce on a smaller input where dwarf symbolization is
affordable. **The frame is present at mid size**: on `ore_ont_9768` (37 MB),
`ClassExpression::partial_cmp` is **6.33%** (vs 12.34% on the 557 MB file), with
`Component::partial_cmp` 3.00% and `ClassExpression::from_pair_unchecked` 3.33% right beside it. So
the effect is not confined to the giant files.

**Two further hypotheses eliminated (6 and 7; see the list above for 1–5):**

6. **The OFN reader's `ClassExpression` construction does not order.** `ObjectIntersectionOf` /
   `ObjectUnionOf` operands are built by `inner.into_inner().map(Self::from_pair).collect()` — file
   order, no `sort`, `dedup`, `binary_search`, `min`/`max` or `BTree*` anywhere in that impl.
7. **No `ClassExpression` variant uses a sorted container**, which would have made the `collect()`
   above order implicitly: `ObjectIntersectionOf(Vec<..>)`, `ObjectUnionOf(Vec<..>)`,
   `ObjectOneOf(Vec<..>)` — all `Vec` (`model.rs:1850/1856/1868`).

**The blocker is `perf` symbolization on this host, and it is tool-independent.** `perf report`
timed out at a 9-minute budget on 0.7 MB of dwarf data at `-F 99`; `perf script --no-demangle`
managed only **33 stacks in 500 s**. The rustdl binary is large with no split debug info, so every
sample costs an expensive DWARF lookup. Reducing input size does not help because the cost is
per-sample, not per-byte — which is why the "use a smaller reproducer" plan did not work.

### What the next attempt should do differently

Do **not** retry `perf report`/`perf script` on this binary as-is. Options, cheapest first:

1. **Split debug info** — build with `split-debuginfo = "packed"` / `debug = 1` in the release
   profile and retry; symbolization is the measured bottleneck, so this attacks it directly.
2. **Temporary instrumentation in the fork.** `horned-owl` is already `[patch]`ed from a local
   clone, so a counter (or a one-shot `Backtrace::force_capture()`) inside
   `ClassExpression::partial_cmp` would name the caller in one run. Invasive but decisive, and the
   patch infrastructure already exists.
3. A profiler that resolves symbols at capture time rather than at report time.

### Standing conclusion, unchanged

Parse is worth attacking — `ore_ont_10926` spends **17.3 s** of a 30.5 s front end in parse, told
tables are 55 ms of it, and parse is **not deadline-bound** so savings convert 1:1 into wall. What
is missing is only the call site of a comparison that costs more than the parsing itself.

---

## RESOLVED (2026-08-23): the caller is `convert_ontology`'s OWN `components.sort()`

Seven hypotheses about horned-owl were all wrong because **the sort is in rustdl**, not in the
parser. The caller chain, from a frame-pointer profile of `ore_ont_9768`:

```
owl_dl_core::convert::convert_ontology
  core::slice::sort::stable::driftsort_main
    core::slice::sort::stable::drift::sort
      core::slice::sort::stable::quicksort::quicksort   (recursing)
        core::ops::function::FnMut::call_mut
          <horned_owl::model::Component as PartialOrd>::partial_cmp
            <horned_owl::model::ClassExpression as PartialOrd>::partial_cmp
```

`convert.rs:2205` — `components.sort()` over `Vec<&AnnotatedComponent<A>>`, using the **derived
`Ord`**, which recurses `Component` → `ClassExpression` → `Rc<str>` and bottoms out in `memcmp`.
That is the 12.34% on `ore_ont_10926` and 7.60% here.

### The technique that found it, after two failed sessions

**Frame pointers.** rustc omits them in release, so a default `--call-graph=fp` profile showed
`partial_cmp` recursing into itself with **no external caller** — which is exactly what I misread as
"the caller is unidentifiable". `--call-graph=dwarf` has the information but cannot be symbolized on
this host inside a 9-minute budget, at any input size, because the cost is per-sample.

```sh
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --release -p owl-dl-cli
perf record -F 199 -g --call-graph=fp -o out.data ./target/release/rustdl locality-stats FILE
perf report -i out.data --no-children -g graph,2.0,caller --stdio --symbol-filter=SYMBOL
```

The perf data drops to **0.1 MB** and the report returns in seconds. **Reach for forced frame
pointers before dwarf** on this codebase; the previous plan ("use a smaller reproducer") could not
have worked, since shrinking the input does not shrink a per-sample cost.

### Why the sort exists, and why it cannot simply go

It is **load-bearing for determinism**: `SetOntology` is a `HashSet` whose iteration order is
per-process, so this sort pins the `intern_class`/`intern_role`/`intern_individual` sequence and
therefore all `ClassId`/`RoleId`/`ConceptId` assignment. Removing it reintroduces the #59 class of
nondeterminism.

### What was tried, and what remains

`sort` → `sort_unstable` is **behaviour-identical** (elements come from a `HashSet`, so pairwise
distinct, so stability is unobservable) and **measured NEUTRAL**: `convert_ms` 11,763 → 11,202 /
502 → 506 / 15,712 → 15,490. Kept for the avoided scratch allocation and the note, **not** as a win.
Its neutrality is the informative part — the cost is the comparisons, not the sort's overhead.

**The real lever, unbuilt:** sort by a cheap precomputed deterministic key (fixed-seed hash,
tie-broken by the full `Ord`) — O(n) hashing plus O(n log n) `u64` comparisons instead of O(n log n)
deep recursive ones. **It is not free:** the resulting canonical order differs, which reassigns
`ClassId`s — still deterministic, but observably different wherever a downstream tie breaks by id.
Given #59, that needs a full output differential before it ships, not a wall measurement.
