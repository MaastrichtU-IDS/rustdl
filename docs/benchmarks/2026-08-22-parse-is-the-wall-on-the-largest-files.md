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
