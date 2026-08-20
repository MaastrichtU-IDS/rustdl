//! Conversion from `horned-owl`'s model into our [`InternalOntology`].
//!
//! - Day 10: concept-level conversion ([`convert_class_expression`],
//!   [`convert_object_property`], [`convert_individual`]).
//! - Day 11: axiom-level conversion ([`convert_component`],
//!   [`convert_ontology`]) — this file.
//! - Day 12: reverse conversion + round-trip proptest (still to come).

use horned_owl::model::{
    AnnotatedComponent, Class, DataRange, Literal, SubObjectPropertyExpression,
};
use horned_owl::model::{ClassExpression, Component, ForIRI, Individual, ObjectPropertyExpression};
use horned_owl::ontology::set::SetOntology;
use thiserror::Error;

use crate::ConceptPool;
use crate::Vocabulary;
use crate::data_axioms::{
    DataIntersectionDkey, DateKey, DateTimeKey, Decimal, FloatRange, IntegerRange, OrdRange,
    RangeBucket, StrSet, exact_string_literal, parse_data_intersection_dkey, parse_date,
    parse_datetime, parse_decimal,
};
use crate::ir::{ClassId, ConceptExpr, ConceptId, IndividualId, Role};
use crate::ontology::{Axiom, InternalOntology, SubRolePath};

/// IRI namespace for synthetic *data-key* (`DKey`) classes. These are
/// opaque atomic fillers introduced when lowering `xsd:integer`-typed
/// `DataHasValue` / `DataSomeValuesFrom` restrictions to the
/// object-style `∃p.DKey(range)` encoding (see the data-property arms of
/// [`convert_class_expression`]). They are NOT user classes: the
/// classifier filters this prefix out of the reported class list so
/// `DKey` subsumptions never appear in output (see
/// `crates/owl-dl-reasoner/src/classify.rs`).
///
/// The full IRI deterministically encodes the range, DATATYPE-TAGGED so
/// integer and float (real) keys live in disjoint namespaces and can
/// NEVER cross-subsume — a soundness requirement (an integer value must
/// not subsume a float range or vice versa):
/// - **integer**: `urn:rustdl-dkey:<min>:<max>` where each bound is a
///   decimal i64 or `*` for the unbounded (`None`) end.
/// - **float/double** (Phase D6 Part B): `urn:rustdl-dkey:f:<min>:<min_incl>:<max>:<max_incl>`
///   where each bound is the `f64::to_bits()` decimal (EXACT round-trip,
///   so `"1.0"` and `"1.00"` key identically) or `*`, and each `_incl`
///   flag is `i` (inclusive) or `e` (exclusive). The `f:` tag makes the
///   integer parser reject float keys (its `split_once(':')` yields the
///   token `"f"`, which fails `parse::<i64>()`).
///
/// This makes interning idempotent (same range → same IRI → same
/// `ClassId` via vocabulary dedup) and lets the post-conversion pass
/// recover each range — and its datatype kind — by parsing the IRI.
pub const DKEY_IRI_PREFIX: &str = "urn:rustdl-dkey:";

/// Reserved IRI namespace for anonymous individuals interned during conversion.
/// Anonymous individuals are first-class `IndividualId`s under this prefix; they
/// participate in all ABox/identity reasoning but are filtered from named-individual
/// output surfaces (they have no real IRI). Cannot collide with an input individual IRI.
pub const ANON_IRI_PREFIX: &str = "urn:rustdl-anon:";

/// Datatype tag for the `xsd:float` (f32-precision) `DKey` namespace.
/// Full prefix is `urn:rustdl-dkey:f:`.
const DKEY_FLOAT_TAG: &str = "f:";

/// Datatype tag for the `xsd:double` (f64-precision) `DKey` namespace.
/// Full prefix is `urn:rustdl-dkey:db:`. The prefix `db:` is mutually
/// non-prefixing with all other tags (`f:`, `fo:`, `dec:`, `deo:`, `date:`,
/// `dao:`, `dt:`, `dto:`, `str:`, and the untagged integer form), so the
/// decoder exclusivity property is preserved.
const DKEY_DOUBLE_TAG: &str = "db:";

/// Build the deterministic integer `DKey` IRI.
fn dkey_iri(range: IntegerRange) -> String {
    fn bound(b: Option<i64>) -> String {
        b.map_or_else(|| "*".to_string(), |v| v.to_string())
    }
    format!("{DKEY_IRI_PREFIX}{}:{}", bound(range.min), bound(range.max))
}

/// Internal helper: build a float-family `DKey` IRI with the given `tag`.
/// Bounds are encoded via `f64::to_bits()` for exact round-trip;
/// inclusivity is `i`/`e`.
fn tagged_float_dkey_iri(tag: &str, range: FloatRange) -> String {
    fn bound(b: Option<f64>) -> String {
        b.map_or_else(|| "*".to_string(), |v| v.to_bits().to_string())
    }
    fn flag(incl: bool) -> char {
        if incl { 'i' } else { 'e' }
    }
    format!(
        "{DKEY_IRI_PREFIX}{tag}{}:{}:{}:{}",
        bound(range.min),
        flag(range.min_incl),
        bound(range.max),
        flag(range.max_incl),
    )
}

/// Build the deterministic `xsd:float` (`f:`) `DKey` IRI.
/// Bounds are f64 representations of f32 values (exact by construction
/// since every f32 is an exact f64); same-f32 lexicals map to the same
/// bit pattern.
fn float_dkey_iri(range: FloatRange) -> String {
    tagged_float_dkey_iri(DKEY_FLOAT_TAG, range)
}

/// Build the deterministic `xsd:double` (`db:`) `DKey` IRI.
fn double_dkey_iri(range: FloatRange) -> String {
    tagged_float_dkey_iri(DKEY_DOUBLE_TAG, range)
}

/// Whether `iri` is a synthetic `DKey` class IRI (any datatype bucket).
/// The tableau has no vocabulary, so the reasoner uses this (+ the
/// `decode_*_dkey` family) to build a `ClassId → range` side-map for the
/// concrete-domain check. See the P2/P3 design spec.
#[must_use]
pub fn is_dkey_iri(iri: &str) -> bool {
    iri.starts_with(DKEY_IRI_PREFIX)
}

/// Public single-point decode of an INTEGER `DKey` IRI into its inclusive
/// `(min, max)` bounds (`None` = unbounded on that side). Returns `None`
/// for any non-integer-bucket or malformed `DKey` IRI. Primitive bounds
/// are returned so the internal `IntegerRange` type need not be exposed.
#[must_use]
pub fn decode_integer_dkey(iri: &str) -> Option<(Option<i64>, Option<i64>)> {
    parse_dkey_iri(iri).map(|r| (r.min, r.max))
}

/// Parse a `DKey` IRI back into its INTEGER range. Returns `None` for
/// any IRI that is not a well-formed integer `DKey` IRI (including all
/// float-tagged keys — the `f:` tag's `"f"` token fails the i64 parse).
pub(crate) fn parse_dkey_iri(iri: &str) -> Option<IntegerRange> {
    let rest = iri.strip_prefix(DKEY_IRI_PREFIX)?;
    let (min_s, max_s) = rest.split_once(':')?;
    // Each bound is "*" (unbounded → None) or a decimal i64. A non-"*"
    // token that fails to parse is a malformed IRI → reject the whole.
    fn bound(s: &str) -> Result<Option<i64>, ()> {
        if s == "*" {
            Ok(None)
        } else {
            s.parse::<i64>().map(Some).map_err(|_| ())
        }
    }
    Some(IntegerRange {
        min: bound(min_s).ok()?,
        max: bound(max_s).ok()?,
    })
}

/// Internal helper: parse a float-family `DKey` IRI with the given `tag`
/// back into a [`FloatRange`]. Returns `None` for any IRI not carrying
/// exactly this tag, or any malformed bound/flag.
fn parse_tagged_float_dkey_iri(iri: &str, tag: &str) -> Option<FloatRange> {
    let rest = iri.strip_prefix(DKEY_IRI_PREFIX)?;
    let rest = rest.strip_prefix(tag)?;
    let mut parts = rest.splitn(4, ':');
    let min_s = parts.next()?;
    let min_f = parts.next()?;
    let max_s = parts.next()?;
    let max_f = parts.next()?;
    fn bound(s: &str) -> Result<Option<f64>, ()> {
        if s == "*" {
            Ok(None)
        } else {
            s.parse::<u64>()
                .map(|bits| Some(f64::from_bits(bits)))
                .map_err(|_| ())
        }
    }
    fn flag(s: &str) -> Result<bool, ()> {
        match s {
            "i" => Ok(true),
            "e" => Ok(false),
            _ => Err(()),
        }
    }
    Some(FloatRange {
        min: bound(min_s).ok()?,
        min_incl: flag(min_f).ok()?,
        max: bound(max_s).ok()?,
        max_incl: flag(max_f).ok()?,
    })
}

/// Parse a `DKey` IRI back into its `xsd:float` (`f:`) range. Returns `None`
/// for any IRI that is not a well-formed `xsd:float`-tagged `DKey` IRI.
pub(crate) fn parse_float_dkey_iri(iri: &str) -> Option<FloatRange> {
    parse_tagged_float_dkey_iri(iri, DKEY_FLOAT_TAG)
}

/// Parse a `DKey` IRI back into its `xsd:double` (`db:`) range. Returns
/// `None` for any IRI that is not a well-formed `xsd:double`-tagged `DKey` IRI.
pub(crate) fn parse_double_dkey_iri(iri: &str) -> Option<FloatRange> {
    parse_tagged_float_dkey_iri(iri, DKEY_DOUBLE_TAG)
}

// ── Phase D8 (2026-06-09): decimal / date / dateTime DKey buckets ────────
//
// Three more datatype-tagged `DKey` namespaces, each DISJOINT from integer,
// float, and each other — a soundness requirement (a decimal value space is
// not a binary-float value space; mixing timezone-free temporals with
// numerics is meaningless). The tags are mutually non-prefixing and
// non-numeric so the five `parse_*_dkey_iri` decoders are pairwise
// exclusive (verified by the `parser_matrix_*` canaries):
//   integer   → untagged      `urn:rustdl-dkey:<i64|*>:<i64|*>`
//   float     → `f:`          (existing)
//   decimal   → `dec:`        exact lexical key (no `:`)
//   date      → `date:`       `y.mo.d`  (no `:`)
//   dateTime  → `dt:`         `y.mo.d.h.mi.s`  (no `:`)
// Inner component separators are `.` (never `:`), so the four `:` in the
// `{min}:{min_incl}:{max}:{max_incl}` envelope are the ONLY colons and the
// `splitn(4, ':')` decode is unambiguous.
const DKEY_DECIMAL_TAG: &str = "dec:";
const DKEY_DATE_TAG: &str = "date:";
const DKEY_DATETIME_TAG: &str = "dt:";

/// First-class data-property lowering. **Default ON** — set
/// `RUSTDL_DATA_PROPERTIES=0` to opt out. (Like Konclude/HermiT, an OWL 2 DL
/// reasoner reasons about data properties by default. Flipped ON after the
/// gate-ON path was validated: full unit suite green, ORE classification net
/// FP=0, and the `xsd:float` value-identity FP fixed by dropping float in the
/// gate-ON `ABox` arms. Sound under-approximation — `xsd:float`, disjoint-dp
/// value clashes, and `a dp v` queries are deliberately not decided.) Read per
/// call so tests can toggle.
fn data_properties_enabled() -> bool {
    std::env::var("RUSTDL_DATA_PROPERTIES").map_or(true, |v| v != "0")
}

/// Build a tagged `DKey` IRI for an [`OrdRange<T>`], encoding each bound via
/// `key` (which MUST NOT emit a `:`) and inclusivity as `i`/`e`.
fn ord_dkey_iri<T>(tag: &str, range: &OrdRange<T>, key: impl Fn(&T) -> String) -> String {
    let bound = |b: &Option<T>| b.as_ref().map_or_else(|| "*".to_string(), &key);
    let flag = |incl: bool| if incl { 'i' } else { 'e' };
    format!(
        "{DKEY_IRI_PREFIX}{tag}{}:{}:{}:{}",
        bound(&range.min),
        flag(range.min_incl),
        bound(&range.max),
        flag(range.max_incl),
    )
}

/// Inverse of [`ord_dkey_iri`]. Returns `None` for any IRI not carrying
/// EXACTLY this `tag` (so it rejects every other bucket's keys, including
/// the untagged integer form), or any malformed bound/flag.
fn parse_ord_dkey_iri<T: Ord + Clone>(
    iri: &str,
    tag: &str,
    key: impl Fn(&str) -> Option<T>,
) -> Option<OrdRange<T>> {
    let rest = iri.strip_prefix(DKEY_IRI_PREFIX)?.strip_prefix(tag)?;
    let mut parts = rest.splitn(4, ':');
    let min_s = parts.next()?;
    let min_f = parts.next()?;
    let max_s = parts.next()?;
    let max_f = parts.next()?;
    let bound = |s: &str| -> Result<Option<T>, ()> {
        if s == "*" {
            Ok(None)
        } else {
            key(s).map(Some).ok_or(())
        }
    };
    let flag = |s: &str| -> Result<bool, ()> {
        match s {
            "i" => Ok(true),
            "e" => Ok(false),
            _ => Err(()),
        }
    };
    Some(OrdRange {
        min: bound(min_s).ok()?,
        min_incl: flag(min_f).ok()?,
        max: bound(max_s).ok()?,
        max_incl: flag(max_f).ok()?,
    })
}

/// Canonical `:`-free key for a [`Decimal`] bound (its own normalized
/// lexical form: `[-]int[.frac]`, with a `0` integer part for sub-1 values).
fn decimal_key(d: &Decimal) -> String {
    let sign = if d.negative { "-" } else { "" };
    let int = if d.int.is_empty() {
        "0"
    } else {
        d.int.as_str()
    };
    if d.frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{}", d.frac)
    }
}

fn date_key(k: &DateKey) -> String {
    format!("{}.{}.{}", k.0, k.1, k.2)
}

fn parse_date_key(s: &str) -> Option<DateKey> {
    let mut it = s.split('.');
    let y = it.next()?.parse().ok()?;
    let mo = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((y, mo, d))
}

fn datetime_key(k: &DateTimeKey) -> String {
    format!("{}.{}.{}.{}.{}.{}", k.0, k.1, k.2, k.3, k.4, k.5)
}

fn parse_datetime_key(s: &str) -> Option<DateTimeKey> {
    let mut it = s.split('.');
    let y = it.next()?.parse().ok()?;
    let mo = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    let h = it.next()?.parse().ok()?;
    let mi = it.next()?.parse().ok()?;
    let sec = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((y, mo, d, h, mi, sec))
}

pub(crate) fn parse_decimal_dkey_iri(iri: &str) -> Option<OrdRange<Decimal>> {
    parse_ord_dkey_iri(iri, DKEY_DECIMAL_TAG, parse_decimal)
}

pub(crate) fn parse_date_dkey_iri(iri: &str) -> Option<OrdRange<DateKey>> {
    parse_ord_dkey_iri(iri, DKEY_DATE_TAG, parse_date_key)
}

pub(crate) fn parse_datetime_dkey_iri(iri: &str) -> Option<OrdRange<DateTimeKey>> {
    parse_ord_dkey_iri(iri, DKEY_DATETIME_TAG, parse_datetime_key)
}

/// Public single-point decode of an `xsd:float` (`f:`) `DKey` IRI into its
/// bound components `(min, min_incl, max, max_incl)`. Returns `None` for any
/// non-float-bucket or malformed `DKey` IRI. Mirrors [`decode_integer_dkey`] —
/// returns decomposed components so the internal `FloatRange` type need not be
/// exposed.
#[must_use]
pub fn decode_float_dkey(iri: &str) -> Option<(Option<f64>, bool, Option<f64>, bool)> {
    parse_float_dkey_iri(iri).map(|r| (r.min, r.min_incl, r.max, r.max_incl))
}

/// Public single-point decode of an `xsd:double` (`db:`) `DKey` IRI into its
/// bound components `(min, min_incl, max, max_incl)`. Returns `None` for any
/// non-double-bucket or malformed `DKey` IRI.
#[must_use]
pub fn decode_double_dkey(iri: &str) -> Option<(Option<f64>, bool, Option<f64>, bool)> {
    parse_double_dkey_iri(iri).map(|r| (r.min, r.min_incl, r.max, r.max_incl))
}

/// Public single-point decode of a DECIMAL `DKey` IRI into its bound components
/// `(min, min_incl, max, max_incl)`. Returns `None` for any non-decimal-bucket
/// or malformed `DKey` IRI.
#[must_use]
pub fn decode_decimal_dkey(iri: &str) -> Option<(Option<Decimal>, bool, Option<Decimal>, bool)> {
    parse_decimal_dkey_iri(iri).map(|r| (r.min, r.min_incl, r.max, r.max_incl))
}

/// Public single-point decode of a DATE `DKey` IRI into its bound components
/// `(min, min_incl, max, max_incl)`. Returns `None` for any non-date-bucket
/// or malformed `DKey` IRI.
#[must_use]
pub fn decode_date_dkey(iri: &str) -> Option<(Option<DateKey>, bool, Option<DateKey>, bool)> {
    parse_date_dkey_iri(iri).map(|r| (r.min, r.min_incl, r.max, r.max_incl))
}

/// Public single-point decode of a DATETIME `DKey` IRI into its bound components
/// `(min, min_incl, max, max_incl)`. Returns `None` for any non-datetime-bucket
/// or malformed `DKey` IRI.
#[must_use]
pub fn decode_datetime_dkey(
    iri: &str,
) -> Option<(Option<DateTimeKey>, bool, Option<DateTimeKey>, bool)> {
    parse_datetime_dkey_iri(iri).map(|r| (r.min, r.min_incl, r.max, r.max_incl))
}

/// Generic counterpart of [`lower_data_to_some`] for the [`OrdRange`]
/// datatypes: intern the tagged `DKey(range)` filler and return
/// `∃p.DKey(range)`.
fn lower_ord_data_to_some<T: Ord + Clone>(
    range: &OrdRange<T>,
    tag: &str,
    key: impl Fn(&T) -> String,
    dp_iri: &str,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> ConceptId {
    let role_id = vocab.intern_role(dp_iri);
    let dkey_class = vocab.intern_class(&ord_dkey_iri(tag, range, key));
    let filler = pool.atomic(dkey_class);
    pool.some(Role::named(role_id), filler)
}

// ── Phase D9 (2026-06-09): xsd:string value-set DKey bucket ──────────────
//
// Strings are EQUALITY-typed (not ordered), so the key is a set, not an
// interval. Own `str:` tag, strictly disjoint from the five numeric/temporal
// buckets. Members are hex-encoded (UTF-8 bytes → `[0-9a-f]*`) so arbitrary
// string content — including `:`, `.`, unicode — round-trips through the
// `:`-delimited IRI unambiguously; `*` (not valid hex) marks `Top`.
const DKEY_STRING_TAG: &str = "str:";

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn hex_decode(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let b = s.as_bytes();
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        // hi, lo ∈ 0..=15 ⟹ hi*16+lo ∈ 0..=255; try_from documents that.
        bytes.push(u8::try_from(hi * 16 + lo).ok()?);
        i += 2;
    }
    String::from_utf8(bytes).ok()
}

fn str_dkey_iri(set: &StrSet) -> String {
    match set {
        StrSet::Top => format!("{DKEY_IRI_PREFIX}{DKEY_STRING_TAG}*"),
        StrSet::Set(members) => {
            let body = members
                .iter()
                .map(|m| hex_encode(m.as_bytes()))
                .collect::<Vec<_>>()
                .join(":");
            format!("{DKEY_IRI_PREFIX}{DKEY_STRING_TAG}{body}")
        }
    }
}

/// Public single-point decode of a STRING `DKey` IRI into its [`StrSet`]
/// (`Top` or a finite string set). Returns `None` for any non-string-bucket
/// or malformed `DKey` IRI, mirroring [`decode_integer_dkey`].
#[must_use]
pub fn decode_string_dkey(iri: &str) -> Option<StrSet> {
    parse_string_dkey_iri(iri)
}

fn parse_string_dkey_iri(iri: &str) -> Option<StrSet> {
    let rest = iri
        .strip_prefix(DKEY_IRI_PREFIX)?
        .strip_prefix(DKEY_STRING_TAG)?;
    if rest == "*" {
        return Some(StrSet::Top);
    }
    let mut set = std::collections::BTreeSet::new();
    for tok in rest.split(':') {
        set.insert(hex_decode(tok)?);
    }
    Some(StrSet::Set(set))
}

fn lower_str_data_to_some(
    set: &StrSet,
    dp_iri: &str,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> ConceptId {
    let role_id = vocab.intern_role(dp_iri);
    let dkey_class = vocab.intern_class(&str_dkey_iri(set));
    let filler = pool.atomic(dkey_class);
    pool.some(Role::named(role_id), filler)
}

// ── Numeric DataOneOf DKey buckets (Phase D-numeric-oneof) ───────────────
//
// Five new ONEOF tags, each strictly disjoint from each other and from all
// interval tags. Using `io:`, `fo:`, `deo:`, `dao:`, `dto:` avoids any
// collision with the existing tags (`f:`, `dec:`, `date:`, `dt:`, `str:`,
// untagged integer). The inner separator is `;` (not `:` or `.`) so the
// `:`-delimited four-field envelope decode stays unambiguous.
//
// Each IRI encodes the SET of distinct values, semicolon-separated:
//   io:<v1>;<v2>;…       integer oneof  (decimal i64 strings)
//   fo:<bits1>;<bits2>;… float oneof    (xsd:float, f32-rounded then widened;
//                                        f64::to_bits decimal, normalized)
//   dbo:<bits1>;<bits2>;… double oneof  (xsd:double, f64::to_bits decimal)
//   deo:<k1>;<k2>;…      decimal oneof  (decimal_key encoding, no `:`)
//   dao:<k1>;<k2>;…      date oneof     (date_key  encoding, no `:`)
//   dto:<k1>;<k2>;…      dateTime oneof (datetime_key encoding, no `:`)
//
// Soundness: pairwise mutual exclusivity is enforced by the unique prefix;
// a float-oneof IRI (`fo:...`) will return `None` from all non-float-oneof
// parsers because none of their prefixes match `fo:`. Verified by the
// `numeric_oneof_parser_matrix_exclusivity` canary below.

const DKEY_INT_ONEOF_TAG: &str = "io:";
const DKEY_FLOAT_ONEOF_TAG: &str = "fo:";
const DKEY_DOUBLE_ONEOF_TAG: &str = "dbo:";
const DKEY_DECIMAL_ONEOF_TAG: &str = "deo:";
const DKEY_DATE_ONEOF_TAG: &str = "dao:";
const DKEY_DATETIME_ONEOF_TAG: &str = "dto:";

/// Encode a numeric-oneof set using a per-item encoding function.
/// Items are joined by `;` (inner separator; `key` MUST NOT emit `;` or `:`).
fn numeric_oneof_iri<T>(
    tag: &str,
    set: &std::collections::BTreeSet<T>,
    key: impl Fn(&T) -> String,
) -> String {
    let body = set.iter().map(key).collect::<Vec<_>>().join(";");
    format!("{DKEY_IRI_PREFIX}{tag}{body}")
}

/// Decode a numeric-oneof `DKey` IRI back into its set, parsing each
/// `;`-separated token via `parse_key`. Returns `None` for any non-matching
/// tag or malformed token.
fn parse_numeric_oneof_iri<T: Ord>(
    iri: &str,
    tag: &str,
    parse_key: impl Fn(&str) -> Option<T>,
) -> Option<std::collections::BTreeSet<T>> {
    let rest = iri.strip_prefix(DKEY_IRI_PREFIX)?.strip_prefix(tag)?;
    let mut set = std::collections::BTreeSet::new();
    for tok in rest.split(';') {
        set.insert(parse_key(tok)?);
    }
    Some(set)
}

// ── INTEGER ONEOF ──────────────────────────────────────────────────────

fn int_oneof_iri(set: &std::collections::BTreeSet<i64>) -> String {
    numeric_oneof_iri(DKEY_INT_ONEOF_TAG, set, std::string::ToString::to_string)
}

fn parse_int_oneof_iri(iri: &str) -> Option<std::collections::BTreeSet<i64>> {
    parse_numeric_oneof_iri(iri, DKEY_INT_ONEOF_TAG, |s| s.parse().ok())
}

/// Public decoder for an INTEGER-ONEOF `DKey` IRI. Returns `None` for any
/// non-integer-oneof or malformed IRI.
#[must_use]
pub fn decode_int_oneof_dkey(iri: &str) -> Option<std::collections::BTreeSet<i64>> {
    parse_int_oneof_iri(iri)
}

// ── FLOAT ONEOF ────────────────────────────────────────────────────────

/// Key for a float oneof member: `f64::to_bits()` decimal (exact round-trip).
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "must match Fn(&T)->String bound in numeric_oneof_iri"
)]
fn float_oneof_member_key(w: &crate::data_axioms::OrdF64) -> String {
    // Encode as u64 bit-representation (exact round-trip, same format as before).
    w.to_f64().to_bits().to_string()
}

fn float_oneof_iri(set: &std::collections::BTreeSet<crate::data_axioms::OrdF64>) -> String {
    numeric_oneof_iri(DKEY_FLOAT_ONEOF_TAG, set, float_oneof_member_key)
}

fn parse_float_oneof_iri(
    iri: &str,
) -> Option<std::collections::BTreeSet<crate::data_axioms::OrdF64>> {
    parse_numeric_oneof_iri(iri, DKEY_FLOAT_ONEOF_TAG, |s| {
        let bits: u64 = s.parse().ok()?;
        let v = f64::from_bits(bits);
        // Reject NaN / ±∞ defensively (shouldn't appear: they were rejected at
        // parse time, but if someone hand-crafts an IRI, don't propagate).
        if !v.is_finite() {
            return None;
        }
        Some(crate::data_axioms::OrdF64::new(v))
    })
}

/// Public decoder for a FLOAT-ONEOF `DKey` IRI.
#[must_use]
pub fn decode_float_oneof_dkey(
    iri: &str,
) -> Option<std::collections::BTreeSet<crate::data_axioms::OrdF64>> {
    parse_float_oneof_iri(iri)
}

// ── DOUBLE ONEOF ───────────────────────────────────────────────────────
// A SEPARATE bucket from `fo:` because OWL 2 gives `xsd:float` and `xsd:double`
// disjoint value spaces — see `data_axioms::parse_xsd_double_oneof`. The `db:`
// interval tag and this `dbo:` tag do not shadow each other: `"dbo:…"` does not
// start with `"db:"` (the char after `db` is `o`, not `:`), and vice versa.

fn double_oneof_iri(set: &std::collections::BTreeSet<crate::data_axioms::OrdF64>) -> String {
    numeric_oneof_iri(DKEY_DOUBLE_ONEOF_TAG, set, float_oneof_member_key)
}

fn parse_double_oneof_iri(
    iri: &str,
) -> Option<std::collections::BTreeSet<crate::data_axioms::OrdF64>> {
    parse_numeric_oneof_iri(iri, DKEY_DOUBLE_ONEOF_TAG, |s| {
        let bits: u64 = s.parse().ok()?;
        let v = f64::from_bits(bits);
        if !v.is_finite() {
            return None;
        }
        Some(crate::data_axioms::OrdF64::new(v))
    })
}

/// Public decoder for a DOUBLE-ONEOF `DKey` IRI.
#[must_use]
pub fn decode_double_oneof_dkey(
    iri: &str,
) -> Option<std::collections::BTreeSet<crate::data_axioms::OrdF64>> {
    parse_double_oneof_iri(iri)
}

// ── DECIMAL ONEOF ──────────────────────────────────────────────────────

fn decimal_oneof_iri(set: &std::collections::BTreeSet<Decimal>) -> String {
    numeric_oneof_iri(DKEY_DECIMAL_ONEOF_TAG, set, decimal_key)
}

fn parse_decimal_key_from_str(s: &str) -> Option<Decimal> {
    crate::data_axioms::parse_decimal(s)
}

fn parse_decimal_oneof_iri(iri: &str) -> Option<std::collections::BTreeSet<Decimal>> {
    parse_numeric_oneof_iri(iri, DKEY_DECIMAL_ONEOF_TAG, parse_decimal_key_from_str)
}

/// Public decoder for a DECIMAL-ONEOF `DKey` IRI.
#[must_use]
pub fn decode_decimal_oneof_dkey(iri: &str) -> Option<std::collections::BTreeSet<Decimal>> {
    parse_decimal_oneof_iri(iri)
}

// ── DATE ONEOF ─────────────────────────────────────────────────────────

fn date_oneof_iri(set: &std::collections::BTreeSet<DateKey>) -> String {
    numeric_oneof_iri(DKEY_DATE_ONEOF_TAG, set, date_key)
}

fn parse_date_oneof_iri(iri: &str) -> Option<std::collections::BTreeSet<DateKey>> {
    parse_numeric_oneof_iri(iri, DKEY_DATE_ONEOF_TAG, parse_date_key)
}

/// Public decoder for a DATE-ONEOF `DKey` IRI.
#[must_use]
pub fn decode_date_oneof_dkey(iri: &str) -> Option<std::collections::BTreeSet<DateKey>> {
    parse_date_oneof_iri(iri)
}

// ── DATETIME ONEOF ─────────────────────────────────────────────────────

fn datetime_oneof_iri(set: &std::collections::BTreeSet<DateTimeKey>) -> String {
    numeric_oneof_iri(DKEY_DATETIME_ONEOF_TAG, set, datetime_key)
}

fn parse_datetime_oneof_iri(iri: &str) -> Option<std::collections::BTreeSet<DateTimeKey>> {
    parse_numeric_oneof_iri(iri, DKEY_DATETIME_ONEOF_TAG, parse_datetime_key)
}

/// Public decoder for a DATETIME-ONEOF `DKey` IRI.
#[must_use]
pub fn decode_datetime_oneof_dkey(iri: &str) -> Option<std::collections::BTreeSet<DateTimeKey>> {
    parse_datetime_oneof_iri(iri)
}

/// Build the deterministic `DKey` IRI for a folded `RangeBucket`.
/// Used by the `DataIntersectionOf` lowering path: after the bucket's ranges
/// have been intersected in `data_axioms.rs`, convert.rs must turn the result
/// back into an IRI using the same encoding as the individual-range parsers.
pub(crate) fn bucket_to_dkey_iri(b: RangeBucket) -> String {
    match b {
        RangeBucket::Integer(r) => dkey_iri(r),
        RangeBucket::Float(r) => float_dkey_iri(r),
        RangeBucket::Double(r) => double_dkey_iri(r),
        RangeBucket::Decimal(r) => ord_dkey_iri(DKEY_DECIMAL_TAG, &r, decimal_key),
        RangeBucket::Date(r) => ord_dkey_iri(DKEY_DATE_TAG, &r, date_key),
        RangeBucket::DateTime(r) => ord_dkey_iri(DKEY_DATETIME_TAG, &r, datetime_key),
        RangeBucket::Str(s) => str_dkey_iri(&s),
    }
}

/// Phase D11: the shared core of the data-restriction encodings — lower a
/// recognized `DataRange` to `(role, DKey-filler)` where `role` is the data
/// property treated as a forward object role and the filler is the opaque
/// `DKey(range)` atomic class. `DataSomeValuesFrom` wraps this in `∃`,
/// `DataAllValuesFrom` in `∀`; both then share the told `DKey ⊑ DKey`
/// subsumption (and, for ∀, the `DisjointClasses` seeding). Returns `None`
/// for any range the datatype machinery doesn't recognize, so the caller
/// emits `UnsupportedDataRange` and the whole axiom drops (sound).
fn data_range_dkey<A: ForIRI>(
    dr: &DataRange<A>,
    dp_iri: &str,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Option<(Role, ConceptId)> {
    let iri = if let Some(r) = crate::data_axioms::parse_integer_range(dr) {
        dkey_iri(r)
    } else if let Some(r) = crate::data_axioms::parse_xsd_float_range(dr) {
        // xsd:float: f32-precision — same-f32 lexicals → same DKey IRI.
        float_dkey_iri(r)
    } else if let Some(r) = crate::data_axioms::parse_xsd_double_range(dr) {
        // xsd:double: f64-precision — separate bucket from xsd:float.
        double_dkey_iri(r)
    } else if let Some(r) = crate::data_axioms::parse_decimal_range(dr) {
        ord_dkey_iri(DKEY_DECIMAL_TAG, &r, decimal_key)
    } else if let Some(r) = crate::data_axioms::parse_date_range(dr) {
        ord_dkey_iri(DKEY_DATE_TAG, &r, date_key)
    } else if let Some(r) = crate::data_axioms::parse_datetime_range(dr) {
        ord_dkey_iri(DKEY_DATETIME_TAG, &r, datetime_key)
    } else if let Some(s) = crate::data_axioms::parse_string_range(dr) {
        str_dkey_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_integer_oneof(dr) {
        int_oneof_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_xsd_float_oneof(dr) {
        float_oneof_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_xsd_double_oneof(dr) {
        double_oneof_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_decimal_oneof(dr) {
        decimal_oneof_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_date_oneof(dr) {
        date_oneof_iri(&s)
    } else if let Some(s) = crate::data_axioms::parse_datetime_oneof(dr) {
        datetime_oneof_iri(&s)
    } else {
        return None;
    };
    let role = Role::named(vocab.intern_role(dp_iri));
    let dkey_class = vocab.intern_class(&iri);
    Some((role, pool.atomic(dkey_class)))
}

/// Intern the synthetic integer `DKey(range)` filler class and return the
/// `∃p.DKey(range)` concept. `p` is the data property treated as a
/// (forward) object-style role.
fn lower_data_to_some(
    range: IntegerRange,
    dp_iri: &str,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> ConceptId {
    let role_id = vocab.intern_role(dp_iri);
    let dkey_class = vocab.intern_class(&dkey_iri(range));
    let filler = pool.atomic(dkey_class);
    pool.some(Role::named(role_id), filler)
}

/// Float-family counterpart of [`lower_data_to_some`]: intern the float-family
/// `DKey(range)` filler (tagged by `tag` — `DKEY_FLOAT_TAG` for xsd:float,
/// `DKEY_DOUBLE_TAG` for xsd:double) and return `∃p.DKey(range)`.
fn lower_float_data_to_some(
    tag: &str,
    range: FloatRange,
    dp_iri: &str,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> ConceptId {
    let role_id = vocab.intern_role(dp_iri);
    let dkey_class = vocab.intern_class(&tagged_float_dkey_iri(tag, range));
    let filler = pool.atomic(dkey_class);
    pool.some(Role::named(role_id), filler)
}

/// Lower a single literal `l` to the concept `∃dp.DKey(point l)`, reusing the
/// per-datatype point-range `DKey` encoding. `None` for any literal whose datatype
/// the `DKey` machinery does not recognize (caller drops — sound under-approximation).
fn data_point_some<A: ForIRI>(
    dp_iri: &str,
    l: &Literal<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Option<ConceptId> {
    if let Some(v) = integer_literal_value(l) {
        Some(lower_data_to_some(
            IntegerRange::point(v),
            dp_iri,
            vocab,
            pool,
        ))
    } else if let Some(fv) = float_literal_value(l) {
        Some(lower_float_data_to_some(
            fv.tag,
            FloatRange::point(fv.value),
            dp_iri,
            vocab,
            pool,
        ))
    } else if let Some(v) = decimal_literal_value(l) {
        Some(lower_ord_data_to_some(
            &OrdRange::point(v),
            DKEY_DECIMAL_TAG,
            decimal_key,
            dp_iri,
            vocab,
            pool,
        ))
    } else if let Some(v) = date_literal_value(l) {
        Some(lower_ord_data_to_some(
            &OrdRange::point(v),
            DKEY_DATE_TAG,
            date_key,
            dp_iri,
            vocab,
            pool,
        ))
    } else if let Some(v) = datetime_literal_value(l) {
        Some(lower_ord_data_to_some(
            &OrdRange::point(v),
            DKEY_DATETIME_TAG,
            datetime_key,
            dp_iri,
            vocab,
            pool,
        ))
    } else {
        exact_string_literal(l)
            .map(|s| lower_str_data_to_some(&StrSet::singleton(s), dp_iri, vocab, pool))
    }
}

/// Errors produced by conversion from `horned-owl` to our IR.
#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ConversionError {
    /// A class expression variant our IR cannot represent in this phase.
    /// The `kind` field names the offending constructor.
    #[error("unsupported class expression variant: {kind}")]
    UnsupportedConcept { kind: &'static str },

    /// An axiom variant our IR cannot represent in this phase.
    #[error("unsupported axiom kind: {kind}")]
    UnsupportedAxiom { kind: &'static str },

    /// Anonymous individuals are not part of our IR in Phase 0; they are
    /// scheduled for the `ABox` work in Phase 7.
    #[error("anonymous individuals are not supported (planned for Phase 7)")]
    AnonymousIndividual,

    /// Data ranges (everything `xsd:*`-like) wait until Phase 3 minimal
    /// datatype support and Phase 7 full concrete domains.
    #[error("data ranges and data properties are not supported until Phase 3")]
    UnsupportedDataRange,
}

/// Convert a horned-owl [`ClassExpression`] to a [`ConceptId`] in `pool`,
/// interning any encountered class IRIs into `vocab`.
///
/// Concept-level rewriting is performed here because our IR has no direct
/// counterpart for some horned-owl constructors:
///
/// | horned-owl                  | IR encoding              |
/// |-----------------------------|--------------------------|
/// | `ObjectHasValue { r, i }`   | `Some(r, Nominal(i))`    |
/// | `ObjectExactCardinality`    | `And(Min(n,r,c), Max(n,r,c))` |
/// | `ObjectOneOf([a, b, ...])`  | `Or(Nominal(a), Nominal(b), ...)` |
/// | `ObjectIntersectionOf([])`  | `Top`                    |
/// | `ObjectUnionOf([])`         | `Bot`                    |
///
/// These rewrites are logically lossless — our IR's `And/Or/Some/Max/Min`
/// already canonicalize internally.
pub fn convert_class_expression<A: ForIRI>(
    ce: &ClassExpression<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<ConceptId, ConversionError> {
    match ce {
        ClassExpression::Class(c) => {
            let iri: &str = c.0.as_ref();
            // OWL 2 built-in vocabulary: owl:Thing ≡ ⊤, owl:Nothing ≡ ⊥.
            // The IRI form is the only legal way to refer to them in
            // ClassExpression (horned-owl has no dedicated Top/Bottom
            // variants), so we intercept here and lower to the IR's
            // structural Top/Bot rather than interning the IRI as if
            // it were an arbitrary user class.
            match iri {
                "http://www.w3.org/2002/07/owl#Thing" => Ok(pool.top()),
                "http://www.w3.org/2002/07/owl#Nothing" => Ok(pool.bot()),
                _ => {
                    let class_id = vocab.intern_class(iri);
                    Ok(pool.atomic(class_id))
                }
            }
        }
        ClassExpression::ObjectIntersectionOf(xs) => {
            let ids = convert_many(xs, vocab, pool)?;
            if ids.is_empty() {
                Ok(pool.top())
            } else {
                Ok(pool.and(ids))
            }
        }
        ClassExpression::ObjectUnionOf(xs) => {
            let ids = convert_many(xs, vocab, pool)?;
            if ids.is_empty() {
                Ok(pool.bot())
            } else {
                Ok(pool.or(ids))
            }
        }
        ClassExpression::ObjectComplementOf(inner) => {
            let inner_id = convert_class_expression(inner, vocab, pool)?;
            Ok(pool.not(inner_id))
        }
        ClassExpression::ObjectOneOf(xs) => {
            let mut ids = Vec::with_capacity(xs.len());
            for ind in xs {
                let id = convert_individual(ind, vocab)?;
                ids.push(pool.nominal(id));
            }
            if ids.is_empty() {
                Ok(pool.bot())
            } else {
                Ok(pool.or(ids))
            }
        }
        ClassExpression::ObjectSomeValuesFrom { ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.some(role, inner))
        }
        ClassExpression::ObjectAllValuesFrom { ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.all(role, inner))
        }
        ClassExpression::ObjectHasValue { ope, i } => {
            let role = convert_object_property(ope, vocab)?;
            let ind = convert_individual(i, vocab)?;
            let nom = pool.nominal(ind);
            Ok(pool.some(role, nom))
        }
        ClassExpression::ObjectHasSelf(ope) => {
            let role = convert_object_property(ope, vocab)?;
            Ok(pool.self_restriction(role))
        }
        ClassExpression::ObjectMinCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.min(*n, role, inner))
        }
        ClassExpression::ObjectMaxCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            Ok(pool.max(*n, role, inner))
        }
        ClassExpression::ObjectExactCardinality { n, ope, bce } => {
            let role = convert_object_property(ope, vocab)?;
            let inner = convert_class_expression(bce, vocab, pool)?;
            let lo = pool.min(*n, role, inner);
            let hi = pool.max(*n, role, inner);
            Ok(pool.and([lo, hi]))
        }
        // Integer-facet data restrictions lower to the object-style
        // `∃p.DKey(range)` encoding so the enclosing existential /
        // conjunction axiom SURVIVES `ce_or_skip!` (instead of being
        // dropped wholesale). Sound by construction: we only ever ADD
        // subsumptions that genuinely hold (value ∈ range / r1 ⊆ r2,
        // seeded in `convert_ontology`). The property `p` becomes the
        // role, so CR5's role-match keeps height-keys from subsuming
        // width-keys for free.
        //
        // SCOPE — integer facets ONLY. Any unrecognized data range
        // (non-integer datatype, float/decimal/dateTime/string, an
        // unparseable literal, `DataAllValuesFrom`, or any data
        // cardinality) MUST still return `UnsupportedDataRange` so the
        // whole axiom drops. Partial lowering of a mixed conjunction
        // would WEAKEN an equivalence RHS → false-positive subsumption
        // via the sufficient direction. Do not best-effort.
        ClassExpression::DataSomeValuesFrom { dp, dr } => {
            // `∃p.DKey(range)` for any recognized datatype range (integer
            // incl. bare xsd:integer, float/double, decimal, date, dateTime,
            // string/oneOf). Distinct DKey datatype buckets never
            // cross-subsume. Any other range drops (UnsupportedDataRange).
            //
            // DataIntersectionOf: fold members into a single range (exact —
            // no approximation). If the intersection is provably empty, lower
            // to ⊥ directly (`pool.bot()`): any class C with `C ⊑ ∃p.empty`
            // is unsatisfiable. Drop on any unrecognized member or nested
            // composite (sound under-approximation).
            if data_properties_enabled()
                && let Some(intersection) = parse_data_intersection_dkey(dr)
            {
                return match intersection {
                    DataIntersectionDkey::Bucket(b) => {
                        let iri = bucket_to_dkey_iri(b);
                        let role = Role::named(vocab.intern_role(dp.0.as_ref()));
                        let filler = pool.atomic(vocab.intern_class(&iri));
                        Ok(pool.some(role, filler))
                    }
                    DataIntersectionDkey::Empty => Ok(pool.bot()),
                };
            }
            // DataUnionOf: lower to a class-level disjunction
            // `∃p.DKey(r1) ⊔ ∃p.DKey(r2) ⊔ ...` — EXACT for the ∃ direction.
            //
            // Soundness: ALL members must be lowerable; if ANY member is
            // unrecognized (nested composite, DataComplementOf, etc.) we drop
            // the ENTIRE union (return UnsupportedDataRange). Partial lowering
            // would make the ∃ restriction narrower than the original range,
            // which could create false-positive subsumptions — FP=0 is sacred.
            //
            // Empty DataUnionOf = ∃p.∅ = ⊥ (any class needing ∃p.empty is unsat).
            if data_properties_enabled()
                && let DataRange::DataUnionOf(members) = dr
            {
                if members.is_empty() {
                    return Ok(pool.bot());
                }
                let mut disjuncts: Vec<ConceptId> = Vec::with_capacity(members.len());
                for m in members {
                    match data_range_dkey(m, dp.0.as_ref(), vocab, pool) {
                        Some((role, filler)) => disjuncts.push(pool.some(role, filler)),
                        // ANY unrecognized member → drop the whole union (sound).
                        None => return Err(ConversionError::UnsupportedDataRange),
                    }
                }
                return Ok(pool.or(disjuncts));
            }
            // DataComplementOf: lower the inner range to DKey(r), then wrap
            // the filler in a negation → `∃p.¬DKey(r)`.  A value-node carries
            // `DKey({v})` (a point); a clash `DKey({v}) ⊓ ¬DKey(r)` fires ONLY
            // when the told edge `DKey({v}) ⊑ DKey(r)` exists (seeded iff v∈r),
            // so this is FP-safe. Composite inner ranges (DataUnionOf,
            // DataIntersectionOf, nested DataComplementOf) return None from
            // data_range_dkey → drop the whole axiom (sound under-approximation).
            if data_properties_enabled()
                && let DataRange::DataComplementOf(inner) = dr
            {
                return match data_range_dkey(inner, dp.0.as_ref(), vocab, pool) {
                    Some((role, filler)) => {
                        let not_filler = pool.not(filler); // split to satisfy borrow-checker
                        Ok(pool.some(role, not_filler))
                    }
                    None => Err(ConversionError::UnsupportedDataRange),
                };
            }
            match data_range_dkey(dr, dp.0.as_ref(), vocab, pool) {
                Some((role, filler)) => Ok(pool.some(role, filler)),
                None => Err(ConversionError::UnsupportedDataRange),
            }
        }
        ClassExpression::DataHasValue { dp, l } => data_point_some(dp.0.as_ref(), l, vocab, pool)
            .ok_or(ConversionError::UnsupportedDataRange),
        // Phase D11: `∀p.DKey(range)` — the universal-restriction counterpart
        // of DataSomeValuesFrom. Sound object-encoding (under-approximate:
        // a `DKey(range)` member need not be a real in-range value, so object
        // models are MORE permissive ⟹ subsumption/unsat can only MISS,
        // never FP). The lowering yields a `ConceptExpr::All`, which is OUT of
        // `saturator_complete_fragment` (the saturator has no ∀-rule), so any
        // ontology bearing it routes to the complete hybrid tableau (Phase
        // D10). There the told `DKey ⊑ DKey` edges give ∀-monotonicity and
        // the seeded `DisjointClasses` (D11b) give the `∃p.DKey(v) ⊓
        // ∀p.DKey(r)` membership clash when `v ∉ r`.
        //
        // DataIntersectionOf: fold → DKey. Empty intersection → DROP (sound
        // under-approximation: `∀p.empty` means "no p-successor allowed" —
        // correct, but we can't confidently lower it without tableau support
        // for the range-side empty-∀; dropping is safe).
        ClassExpression::DataAllValuesFrom { dp, dr } => {
            if data_properties_enabled()
                && let Some(intersection) = parse_data_intersection_dkey(dr)
            {
                return match intersection {
                    DataIntersectionDkey::Bucket(b) => {
                        let iri = bucket_to_dkey_iri(b);
                        let role = Role::named(vocab.intern_role(dp.0.as_ref()));
                        let filler = pool.atomic(vocab.intern_class(&iri));
                        Ok(pool.all(role, filler))
                    }
                    DataIntersectionDkey::Empty => Err(ConversionError::UnsupportedDataRange),
                };
            }
            // DataComplementOf: `∀p.¬DKey(r)` — contravariant with ∀-monotonicity
            // (∀p.¬DKey(r1) ⊑ ∀p.¬DKey(r2) iff r2 ⊆ r1). Clash fires when a
            // value-node carries DKey({v}) ⊓ ¬DKey(r) with told DKey({v})⊑DKey(r).
            // Drop on composite inner (data_range_dkey returns None).
            if data_properties_enabled()
                && let DataRange::DataComplementOf(inner) = dr
            {
                return match data_range_dkey(inner, dp.0.as_ref(), vocab, pool) {
                    Some((role, filler)) => {
                        let not_filler = pool.not(filler); // split to satisfy borrow-checker
                        Ok(pool.all(role, not_filler))
                    }
                    None => Err(ConversionError::UnsupportedDataRange),
                };
            }
            match data_range_dkey(dr, dp.0.as_ref(), vocab, pool) {
                Some((role, filler)) => Ok(pool.all(role, filler)),
                None => Err(ConversionError::UnsupportedDataRange),
            }
        }
        // Concrete-domain solver (P3): lower qualified data cardinality to
        // object `Min`/`Max` over the DKey filler so the tableau's
        // concrete-domain clash can count it (capacity / conflict). Try each
        // bucket in turn; parsers are mutually exclusive by datatype IRI so
        // only one branch matches. Unrecognized (specific) datatypes still drop
        // (UnsupportedDataRange → whole axiom drops, soundly). Unqualified
        // `rdfs:Literal` cardinality is handled by the final `.or_else` fallback
        // (lower_unqualified_data_cardinality) when the data-properties gate is ON.
        // The tableau SUPPRESSES object-cardinality expansion for DKey fillers,
        // so this never materialises successors (it would otherwise blow up on
        // a large `≥n` over a tiny range).
        ClassExpression::DataMinCardinality { n, dp, dr } => {
            lower_int_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                .or_else(|_| lower_str_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| lower_float_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| lower_decimal_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| lower_date_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| lower_datetime_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| lower_int_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, false))
                .or_else(|_| {
                    lower_float_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                })
                .or_else(|_| {
                    lower_decimal_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                })
                .or_else(|_| {
                    lower_date_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                })
                .or_else(|_| {
                    lower_datetime_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                })
                .or_else(|_| {
                    lower_unqualified_data_cardinality(*n, dp, dr, vocab, pool, true, false)
                        .ok_or(ConversionError::UnsupportedDataRange)
                })
        }
        ClassExpression::DataMaxCardinality { n, dp, dr } => {
            lower_int_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                .or_else(|_| lower_str_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| lower_float_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| lower_decimal_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| lower_date_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| lower_datetime_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| lower_int_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true))
                .or_else(|_| {
                    lower_float_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                })
                .or_else(|_| {
                    lower_decimal_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                })
                .or_else(|_| {
                    lower_date_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                })
                .or_else(|_| {
                    lower_datetime_oneof_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                })
                .or_else(|_| {
                    lower_unqualified_data_cardinality(*n, dp, dr, vocab, pool, false, true)
                        .ok_or(ConversionError::UnsupportedDataRange)
                })
        }
        ClassExpression::DataExactCardinality { n, dp, dr } => {
            lower_int_data_cardinality(*n, dp, dr, vocab, pool, true, true)
                .or_else(|_| lower_str_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| lower_float_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| lower_decimal_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| lower_date_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| lower_datetime_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| lower_int_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| {
                    lower_float_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, true)
                })
                .or_else(|_| {
                    lower_decimal_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, true)
                })
                .or_else(|_| lower_date_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, true))
                .or_else(|_| {
                    lower_datetime_oneof_data_cardinality(*n, dp, dr, vocab, pool, true, true)
                })
                .or_else(|_| {
                    lower_unqualified_data_cardinality(*n, dp, dr, vocab, pool, true, true)
                        .ok_or(ConversionError::UnsupportedDataRange)
                })
        }
    }
}

/// Lower an integer-qualified data cardinality restriction to object
/// `Min`/`Max` (or their `And` for `Exact`) over the integer `DKey` filler.
/// Returns `UnsupportedDataRange` (⇒ the whole axiom drops, soundly) for any
/// non-integer-bucket or unqualified qualifier — integer-first scoping.
fn lower_int_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_integer_range(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a STRING-qualified data cardinality restriction to object
/// `Min`/`Max` (or their `And` for `Exact`) over the string `DKey` filler.
/// Returns `UnsupportedDataRange` (⇒ the whole axiom drops, soundly) for any
/// non-string-bucket qualifier — mirrors [`lower_int_data_cardinality`].
fn lower_str_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_string_range(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a FLOAT-family-qualified data cardinality restriction (xsd:float OR
/// xsd:double) to object `Min`/`Max` (or their `And` for `Exact`) over the
/// appropriate float-family `DKey` filler. Returns `UnsupportedDataRange` for
/// any non-float-family qualifier — mirrors [`lower_int_data_cardinality`].
fn lower_float_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    // Accept both xsd:float (f32-precision) and xsd:double (f64-precision);
    // data_range_dkey will route each to the correct tagged bucket.
    if crate::data_axioms::parse_xsd_float_range(dr).is_none()
        && crate::data_axioms::parse_xsd_double_range(dr).is_none()
    {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DECIMAL-qualified data cardinality restriction to object `Min`/`Max`
/// (or their `And` for `Exact`) over the decimal `DKey` filler. Returns
/// `UnsupportedDataRange` for any non-decimal-bucket qualifier.
fn lower_decimal_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_decimal_range(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DATE-qualified data cardinality restriction to object `Min`/`Max`
/// (or their `And` for `Exact`) over the date `DKey` filler. Returns
/// `UnsupportedDataRange` for any non-date-bucket qualifier.
fn lower_date_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_date_range(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DATETIME-qualified data cardinality restriction to object `Min`/`Max`
/// (or their `And` for `Exact`) over the datetime `DKey` filler. Returns
/// `UnsupportedDataRange` for any non-datetime-bucket qualifier.
fn lower_datetime_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_datetime_range(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

// ── Numeric-oneof data cardinality lowering ────────────────────────────────
//
// Five new `lower_*_oneof_data_cardinality` functions, one per numeric type.
// Each is the "oneof" counterpart of its interval sibling:
//   `lower_int_data_cardinality`    → `lower_int_oneof_data_cardinality`
//   …etc.
// All use the same pattern: check their respective `parse_*_oneof` gate
// (returns `Err(UnsupportedDataRange)` if wrong type), then call
// `data_range_dkey` (which by this point can produce an oneof IRI) and
// wrap in `Min`/`Max`/`And` as requested.
//
// They are added to the `DataMinCardinality`/`DataMaxCardinality`/
// `DataExactCardinality` dispatch chain in `convert_class_expression`.

/// Lower an INTEGER-ONEOF qualified data cardinality to `Min`/`Max` over the
/// integer-oneof `DKey` filler. Returns `UnsupportedDataRange` for any
/// non-`DataOneOf`-of-integers qualifier.
fn lower_int_oneof_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_integer_oneof(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a FLOAT-ONEOF or DOUBLE-ONEOF qualified data cardinality. Returns
/// `UnsupportedDataRange` for any other qualifier. The two datatypes share this
/// entry point but land in DIFFERENT `DKey` buckets (`fo:` / `dbo:`) — see
/// `data_axioms::parse_xsd_double_oneof`.
fn lower_float_oneof_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_xsd_float_oneof(dr).is_none()
        && crate::data_axioms::parse_xsd_double_oneof(dr).is_none()
    {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DECIMAL-ONEOF qualified data cardinality. Returns `UnsupportedDataRange`
/// for any non-`DataOneOf`-of-decimals qualifier.
fn lower_decimal_oneof_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_decimal_oneof(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DATE-ONEOF qualified data cardinality. Returns `UnsupportedDataRange`
/// for any non-`DataOneOf`-of-dates qualifier.
fn lower_date_oneof_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_date_oneof(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Lower a DATETIME-ONEOF qualified data cardinality. Returns `UnsupportedDataRange`
/// for any non-`DataOneOf`-of-dateTimes qualifier.
fn lower_datetime_oneof_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Result<ConceptId, ConversionError> {
    if crate::data_axioms::parse_datetime_oneof(dr).is_none() {
        return Err(ConversionError::UnsupportedDataRange);
    }
    let (role, filler) = data_range_dkey(dr, dp.0.as_ref(), vocab, pool)
        .ok_or(ConversionError::UnsupportedDataRange)?;
    match (want_min, want_max) {
        (true, false) => Ok(pool.min(n, role, filler)),
        (false, true) => Ok(pool.max(n, role, filler)),
        (true, true) => {
            let lo = pool.min(n, role, filler);
            let hi = pool.max(n, role, filler);
            Ok(pool.and([lo, hi]))
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    }
}

/// Whether `dr` is exactly the unqualified `rdfs:Literal` data range. Only this
/// range may fall back to a `⊤` filler for data cardinality — a specific
/// unrecognized datatype must keep dropping (using `⊤` there over-constrains a
/// `≤n` restriction → unsound FP).
fn is_rdfs_literal<A: ForIRI>(dr: &DataRange<A>) -> bool {
    matches!(dr, DataRange::Datatype(dt)
        if dt.0.as_ref() == "http://www.w3.org/2000/01/rdf-schema#Literal")
}

/// Gated fallback for UNQUALIFIED data cardinality (`≥n dp` / `≤n dp` over
/// `rdfs:Literal`): lower to the same cardinality over the IR `⊤` filler.
/// `None` ⇒ not applicable (gate off, or range not `rdfs:Literal`) ⇒ caller drops.
fn lower_unqualified_data_cardinality<A: ForIRI>(
    n: u32,
    dp: &horned_owl::model::DataProperty<A>,
    dr: &DataRange<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
    want_min: bool,
    want_max: bool,
) -> Option<ConceptId> {
    if !data_properties_enabled() || !is_rdfs_literal(dr) {
        return None;
    }
    let role = Role::named(vocab.intern_role(dp.0.as_ref()));
    let top = pool.top();
    Some(match (want_min, want_max) {
        (true, false) => pool.min(n, role, top),
        (false, true) => pool.max(n, role, top),
        (true, true) => {
            let lo = pool.min(n, role, top);
            let hi = pool.max(n, role, top);
            pool.and([lo, hi])
        }
        (false, false) => unreachable!("at least one of min/max requested"),
    })
}

/// Extract an `xsd:integer`-typed literal's value. Returns `None` for
/// any other literal kind (`Simple` = `xsd:string`, `Language`, or a
/// `Datatype` whose IRI is not exactly `xsd:integer`) or an unparseable
/// value — sound under-approximation (a dropped restriction is a miss,
/// never an FP).
fn integer_literal_value<A: ForIRI>(l: &Literal<A>) -> Option<i64> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#integer" => {
            literal.parse::<i64>().ok()
        }
        _ => None,
    }
}

/// A float-family literal value paired with its `DKey` tag so the caller
/// can put it in the correct bucket (`f:` for xsd:float, `db:` for xsd:double).
struct FloatLiteralValue {
    value: f64,
    tag: &'static str,
}

/// Phase D6 (Part B): extract an `xsd:float` / `xsd:double`-typed literal's
/// value. Returns `None` for any other literal kind or an unparseable /
/// non-finite (NaN, ±∞) value — sound under-approximation.
///
/// **Precision**: `xsd:float` is f32; we parse as `f32` then widen to `f64`
/// so that two lexicals denoting the same f32 value map to the SAME f64 bit
/// pattern (and therefore the same `DKey` IRI). `xsd:double` is f64-exact and
/// is parsed directly as `f64`. The returned `tag` selects the correct bucket.
fn float_literal_value<A: ForIRI>(l: &Literal<A>) -> Option<FloatLiteralValue> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#float" => {
            // Parse as f32, widen: same-f32 lexicals → same f64 bit pattern.
            let v = literal
                .parse::<f32>()
                .ok()
                .map(f64::from)
                .filter(|v| v.is_finite())?;
            Some(FloatLiteralValue {
                value: v,
                tag: DKEY_FLOAT_TAG,
            })
        }
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#double" => {
            let v = literal.parse::<f64>().ok().filter(|v| v.is_finite())?;
            Some(FloatLiteralValue {
                value: v,
                tag: DKEY_DOUBLE_TAG,
            })
        }
        _ => None,
    }
}

/// Phase D8: extract an `xsd:decimal`-typed literal as an exact [`Decimal`].
fn decimal_literal_value<A: ForIRI>(l: &Literal<A>) -> Option<Decimal> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#decimal" => {
            parse_decimal(literal)
        }
        _ => None,
    }
}

/// Phase D8: extract an `xsd:date`-typed literal as a [`DateKey`]. Timezone
/// suffixes are dropped by `parse_date` (sound under-approx).
fn date_literal_value<A: ForIRI>(l: &Literal<A>) -> Option<DateKey> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#date" => {
            parse_date(literal)
        }
        _ => None,
    }
}

/// Phase D8: extract an `xsd:dateTime`-typed literal as a [`DateTimeKey`].
/// Fractional seconds / timezones are dropped by `parse_datetime`.
fn datetime_literal_value<A: ForIRI>(l: &Literal<A>) -> Option<DateTimeKey> {
    match l {
        Literal::Datatype {
            literal,
            datatype_iri,
        } if datatype_iri.as_ref() == "http://www.w3.org/2001/XMLSchema#dateTime" => {
            parse_datetime(literal)
        }
        _ => None,
    }
}

fn convert_many<A: ForIRI>(
    xs: &[ClassExpression<A>],
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<Vec<ConceptId>, ConversionError> {
    let mut out = Vec::with_capacity(xs.len());
    for ce in xs {
        out.push(convert_class_expression(ce, vocab, pool)?);
    }
    Ok(out)
}

/// Convert a horned-owl [`ObjectPropertyExpression`] to a [`Role`].
///
/// `InverseObjectProperty` lowers to [`Role::Inverse`] as of Phase 3
/// commit 2. The named property inside the inversion is interned in
/// the role vocabulary just like a forward use; downstream rules
/// decide direction by inspecting [`Role::is_inverse`].
pub fn convert_object_property<A: ForIRI>(
    ope: &ObjectPropertyExpression<A>,
    vocab: &mut Vocabulary,
) -> Result<Role, ConversionError> {
    match ope {
        ObjectPropertyExpression::ObjectProperty(op) => {
            let iri: &str = op.0.as_ref();
            Ok(Role::named(vocab.intern_role(iri)))
        }
        ObjectPropertyExpression::InverseObjectProperty(op) => {
            let iri: &str = op.0.as_ref();
            Ok(Role::inverse(vocab.intern_role(iri)))
        }
    }
}

/// Convert a horned-owl [`Individual`] (named individuals by IRI; anonymous individuals interned under `ANON_IRI_PREFIX`).
pub fn convert_individual<A: ForIRI>(
    i: &Individual<A>,
    vocab: &mut Vocabulary,
) -> Result<IndividualId, ConversionError> {
    match i {
        Individual::Named(ni) => {
            let iri: &str = ni.0.as_ref();
            Ok(vocab.intern_individual(iri))
        }
        Individual::Anonymous(anon) => {
            let label: &str = anon.0.as_ref();
            let synthetic = format!("{ANON_IRI_PREFIX}{label}");
            Ok(vocab.intern_individual(&synthetic))
        }
    }
}

fn intern_class_decl<A: ForIRI>(c: &Class<A>, vocab: &mut Vocabulary) -> ClassId {
    let iri: &str = c.0.as_ref();
    vocab.intern_class(iri)
}

fn convert_sub_role_path<A: ForIRI>(
    sub: &SubObjectPropertyExpression<A>,
    vocab: &mut Vocabulary,
) -> Result<SubRolePath, ConversionError> {
    match sub {
        SubObjectPropertyExpression::ObjectPropertyExpression(ope) => {
            Ok(SubRolePath::Role(convert_object_property(ope, vocab)?))
        }
        SubObjectPropertyExpression::ObjectPropertyChain(chain) => {
            let mut roles = Vec::with_capacity(chain.len());
            for link in chain {
                roles.push(convert_object_property(link, vocab)?);
            }
            Ok(SubRolePath::Chain(roles))
        }
    }
}

fn convert_individuals<A: ForIRI>(
    inds: &[Individual<A>],
    vocab: &mut Vocabulary,
) -> Result<Vec<IndividualId>, ConversionError> {
    let mut out = Vec::with_capacity(inds.len());
    for i in inds {
        out.push(convert_individual(i, vocab)?);
    }
    Ok(out)
}

fn convert_roles<A: ForIRI>(
    opes: &[ObjectPropertyExpression<A>],
    vocab: &mut Vocabulary,
) -> Result<Vec<Role>, ConversionError> {
    let mut out = Vec::with_capacity(opes.len());
    for o in opes {
        out.push(convert_object_property(o, vocab)?);
    }
    Ok(out)
}

/// Axiom-site helper: convert a `ClassExpression`, propagating ANY error
/// (including [`ConversionError::UnsupportedDataRange`]) out of
/// `convert_component` via `Err`. Issue #43: this axiom-carrying component is
/// no longer silently swallowed here — `convert_ontology`'s caller-side match
/// records the drop (`DroppedAxioms`) and continues, so nothing aborts and
/// nothing is lost from the diagnostic surface.
macro_rules! ce_or_skip {
    ($expr:expr) => {
        match $expr {
            Ok(c) => c,
            Err(e) => return Err(e),
        }
    };
}

/// Convert a single horned-owl [`Component`] to one of our axioms.
///
/// Returns:
/// - `Ok(Some(axiom))` when the component maps to an axiom in our IR.
/// - `Ok(None)` when the component is metadata, annotation-related, or a
///   benign declaration (`DeclareDataProperty`, `DeclareDatatype`, …) and
///   has no representation in our IR — silently dropped, benign (see the
///   module docs for the rationale); never counted as a "dropped axiom".
/// - `Err(_)` when the component is semantically meaningful (reasoning-load-
///   bearing content: assertions, ranges, domains, …) but unsupported in this
///   phase — unsupported data ranges via the `ce_or_skip!` macro, unrecognized
///   data-property literals/ranges, the `RUSTDL_DATA_PROPERTIES=0` gate-off
///   fall-through, anonymous individuals, `HasKey`, etc. Issue #43:
///   `convert_ontology` no longer aborts on this — it records the drop in
///   `InternalOntology::dropped` (via [`drop_label`]) and continues with the
///   remaining components.
#[allow(clippy::too_many_lines)] // intrinsic to the breadth of horned-owl's Component enum
pub fn convert_component<A: ForIRI>(
    c: &Component<A>,
    vocab: &mut Vocabulary,
    pool: &mut ConceptPool,
) -> Result<Option<Axiom>, ConversionError> {
    use Component as C;
    match c {
        // ── Silently dropped: metadata + annotation axioms ──────────────
        // None of these carry reasoning-load-bearing content.
        #[allow(clippy::match_same_arms)]
        C::OntologyID(_)
        | C::DocIRI(_)
        | C::OntologyAnnotation(_)
        | C::Import(_)
        | C::DeclareAnnotationProperty(_)
        | C::AnnotationAssertion(_)
        | C::SubAnnotationPropertyOf(_)
        | C::AnnotationPropertyDomain(_)
        | C::AnnotationPropertyRange(_) => Ok(None),

        // ── Declarations ────────────────────────────────────────────────
        C::DeclareClass(d) => Ok(Some(Axiom::DeclareClass(intern_class_decl(&d.0, vocab)))),
        C::DeclareObjectProperty(d) => {
            let iri: &str = d.0.0.as_ref();
            Ok(Some(Axiom::DeclareObjectProperty(vocab.intern_role(iri))))
        }
        C::DeclareNamedIndividual(d) => {
            let iri: &str = d.0.0.as_ref();
            Ok(Some(Axiom::DeclareNamedIndividual(
                vocab.intern_individual(iri),
            )))
        }
        // ── Data properties + datatypes: sound under-approximation ──────
        // Phase D1 (2026-06-03): silently drop data-related declarations
        // and axioms. Class subsumption inferences that DEPEND on data
        // axioms (e.g., disjointness derivable from
        // DataMaxCardinality(1, dp) + DataMinCardinality(2, dp)) are
        // missed; no false positives are introduced. Class expressions
        // containing data-range constructors cause the enclosing axiom
        // to be dropped via the `ce_or_skip!` macro at axiom sites
        // (see `convert_class_expression`'s UnsupportedDataRange returns).
        // Phase D2 measurement decides whether real data-cardinality
        // reasoning (Tier B) is needed; Phase D3+ would add datatype
        // ranges (Tier C).
        C::DeclareDataProperty(_) | C::DeclareDatatype(_) => Ok(None),

        // ── TBox ────────────────────────────────────────────────────────
        C::SubClassOf(ax) => {
            let sub = ce_or_skip!(convert_class_expression(&ax.sub, vocab, pool));
            let sup = ce_or_skip!(convert_class_expression(&ax.sup, vocab, pool));
            Ok(Some(Axiom::SubClassOf { sub, sup }))
        }
        C::EquivalentClasses(ax) => {
            let mut ids = Vec::with_capacity(ax.0.len());
            for ce in &ax.0 {
                ids.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::EquivalentClasses(ids)))
        }
        C::DisjointClasses(ax) => {
            let mut ids = Vec::with_capacity(ax.0.len());
            for ce in &ax.0 {
                ids.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::DisjointClasses(ids)))
        }
        C::DisjointUnion(ax) => {
            let class = intern_class_decl(&ax.0, vocab);
            let mut members = Vec::with_capacity(ax.1.len());
            for ce in &ax.1 {
                members.push(ce_or_skip!(convert_class_expression(ce, vocab, pool)));
            }
            Ok(Some(Axiom::DisjointUnion { class, members }))
        }

        // ── RBox ────────────────────────────────────────────────────────
        C::SubObjectPropertyOf(ax) => {
            let sub = convert_sub_role_path(&ax.sub, vocab)?;
            let sup = convert_object_property(&ax.sup, vocab)?;
            Ok(Some(Axiom::SubObjectPropertyOf { sub, sup }))
        }
        C::EquivalentObjectProperties(ax) => {
            let roles = convert_roles(&ax.0, vocab)?;
            Ok(Some(Axiom::EquivalentObjectProperties(roles)))
        }
        C::DisjointObjectProperties(ax) => {
            let roles = convert_roles(&ax.0, vocab)?;
            Ok(Some(Axiom::DisjointObjectProperties(roles)))
        }
        C::InverseObjectProperties(ax) => {
            let a = Role::named(vocab.intern_role(ax.0.0.as_ref()));
            let b = Role::named(vocab.intern_role(ax.1.0.as_ref()));
            Ok(Some(Axiom::InverseObjectProperties(a, b)))
        }
        C::ObjectPropertyDomain(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let domain = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyDomain { role, domain }))
        }
        C::ObjectPropertyRange(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let range = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyRange { role, range }))
        }
        C::FunctionalObjectProperty(ax) => Ok(Some(Axiom::FunctionalRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::InverseFunctionalObjectProperty(ax) => Ok(Some(Axiom::InverseFunctionalRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::ReflexiveObjectProperty(ax) => Ok(Some(Axiom::ReflexiveRole(convert_object_property(
            &ax.0, vocab,
        )?))),
        C::IrreflexiveObjectProperty(ax) => Ok(Some(Axiom::IrreflexiveRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::SymmetricObjectProperty(ax) => Ok(Some(Axiom::SymmetricRole(convert_object_property(
            &ax.0, vocab,
        )?))),
        C::AsymmetricObjectProperty(ax) => Ok(Some(Axiom::AsymmetricRole(
            convert_object_property(&ax.0, vocab)?,
        ))),
        C::TransitiveObjectProperty(ax) => Ok(Some(Axiom::TransitiveRole(
            convert_object_property(&ax.0, vocab)?,
        ))),

        // ── ABox ────────────────────────────────────────────────────────
        C::ClassAssertion(ax) => {
            let class = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            let individual = convert_individual(&ax.i, vocab)?;
            Ok(Some(Axiom::ClassAssertion { class, individual }))
        }
        C::ObjectPropertyAssertion(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let subject = convert_individual(&ax.from, vocab)?;
            let object = convert_individual(&ax.to, vocab)?;
            Ok(Some(Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            }))
        }
        C::NegativeObjectPropertyAssertion(ax) => {
            let role = convert_object_property(&ax.ope, vocab)?;
            let subject = convert_individual(&ax.from, vocab)?;
            let object = convert_individual(&ax.to, vocab)?;
            Ok(Some(Axiom::NegativeObjectPropertyAssertion {
                role,
                subject,
                object,
            }))
        }
        C::SameIndividual(ax) => Ok(Some(Axiom::SameIndividual(convert_individuals(
            &ax.0, vocab,
        )?))),
        C::DifferentIndividuals(ax) => Ok(Some(Axiom::DifferentIndividuals(convert_individuals(
            &ax.0, vocab,
        )?))),

        // ── DataPropertyAssertion: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // When the gate is ON, lower `dp(from, to)` to a `ClassAssertion` via
        // the DKey reduction (`∃dp.DKey(point v)` — the individual is then a
        // member of that concept). When the gate is OFF, or the literal is an
        // unrecognized datatype, drop silently (sound under-approximation).
        C::DataPropertyAssertion(ax) if data_properties_enabled() => {
            // xsd:float is now handled with f32-precision parsing (same-f32
            // lexicals → same DKey IRI) so the drop guard is no longer needed.
            // xsd:double is f64-exact. Both flow through data_point_some.
            match data_point_some(ax.dp.0.as_ref(), &ax.to, vocab, pool) {
                Some(class) => {
                    let individual = convert_individual(&ax.from, vocab)?;
                    Ok(Some(Axiom::ClassAssertion { class, individual }))
                }
                // unrecognized literal datatype — drop (sound), but this is
                // CONTENT (an ABox assertion), so record it (issue #43 review).
                None => Err(ConversionError::UnsupportedDataRange),
            }
        }

        // ── NegativeDataPropertyAssertion: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // Lower `¬dp(from, to)` to a `ClassAssertion` of `¬∃dp.DKey(point v)`:
        // the individual `from` must NOT have the data value `to` for property `dp`.
        // When the gate is OFF, or the literal is an unrecognized datatype, drop
        // silently (sound under-approximation).
        C::NegativeDataPropertyAssertion(ax) if data_properties_enabled() => {
            // xsd:float is now handled with f32-precision parsing so the drop
            // guard is no longer needed.  xsd:double is f64-exact. Both flow
            // through data_point_some.
            match data_point_some(ax.dp.0.as_ref(), &ax.to, vocab, pool) {
                Some(some_concept) => {
                    let class = pool.not(some_concept); // ¬∃dp.DKey(point v)
                    let individual = convert_individual(&ax.from, vocab)?;
                    Ok(Some(Axiom::ClassAssertion { class, individual }))
                }
                // unrecognized literal datatype — drop (sound), but this is
                // CONTENT (an ABox assertion), so record it (issue #43 review).
                None => Err(ConversionError::UnsupportedDataRange),
            }
        }

        // ── SubDataPropertyOf: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // Lower `sub ⊑ sup` (data properties) to a `SubObjectPropertyOf`
        // role-hierarchy axiom by interning both IRIs into the shared role
        // table. When the gate is OFF, falls through to the catch-all Ok(None).
        C::SubDataPropertyOf(ax) if data_properties_enabled() => {
            let sub = SubRolePath::Role(Role::named(vocab.intern_role(ax.sub.0.as_ref())));
            let sup = Role::named(vocab.intern_role(ax.sup.0.as_ref()));
            Ok(Some(Axiom::SubObjectPropertyOf { sub, sup }))
        }

        // ── EquivalentDataProperties: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // Lower an equivalence cluster of data properties to
        // `EquivalentObjectProperties` over the shared role table.
        // When the gate is OFF, falls through to the catch-all Ok(None).
        C::EquivalentDataProperties(ax) if data_properties_enabled() => {
            let roles: Vec<Role> =
                ax.0.iter()
                    .map(|dp| Role::named(vocab.intern_role(dp.0.as_ref())))
                    .collect();
            Ok(Some(Axiom::EquivalentObjectProperties(roles)))
        }

        // ── DisjointDataProperties: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // Disjoint(dp,dq) → DisjointObjectProperties on the dp-roles (gate-ON).
        // When the gate is OFF, falls through to the catch-all Ok(None).
        C::DisjointDataProperties(ax) if data_properties_enabled() => {
            let roles: Vec<Role> =
                ax.0.iter()
                    .map(|dp| Role::named(vocab.intern_role(dp.0.as_ref())))
                    .collect();
            Ok(Some(Axiom::DisjointObjectProperties(roles)))
        }
        // ── FunctionalDataProperty: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // Functional(dp) → FunctionalRole(dp-role): ≤1 value via functional-merge (gate-ON).
        // When the gate is OFF, falls through to the catch-all Ok(None).
        C::FunctionalDataProperty(ax) if data_properties_enabled() => {
            let dp = &ax.0;
            let role = Role::named(vocab.intern_role(dp.0.as_ref()));
            Ok(Some(Axiom::FunctionalRole(role)))
        }
        // ── DataPropertyDomain: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // DataPropertyDomain(dp,C) → ObjectPropertyDomain on the dp-role (gate-ON).
        // When the gate is OFF, falls through to the catch-all Ok(None).
        C::DataPropertyDomain(ax) if data_properties_enabled() => {
            let role = Role::named(vocab.intern_role(ax.dp.0.as_ref()));
            let domain = ce_or_skip!(convert_class_expression(&ax.ce, vocab, pool));
            Ok(Some(Axiom::ObjectPropertyDomain { role, domain }))
        }
        // ── DataPropertyRange: gated lowering (RUSTDL_DATA_PROPERTIES) ──
        // DataPropertyRange(dp,R) → ObjectPropertyRange with DKey(R) filler (gate-ON).
        // When the gate is OFF, or the range is unrecognized, drop silently (sound).
        C::DataPropertyRange(ax) if data_properties_enabled() => {
            // xsd:float is now handled with f32-precision parsing (separate
            // `f:` bucket) so the drop guard is no longer needed.
            //
            // DataIntersectionOf: fold to a single range. Empty intersection →
            // DROP (sound: `∀p.⊥`-semantics for PropertyRange needs careful
            // tableau support; conservative drop avoids FP risk).
            if let Some(intersection) = parse_data_intersection_dkey(&ax.dr) {
                return match intersection {
                    DataIntersectionDkey::Bucket(b) => {
                        let iri = bucket_to_dkey_iri(b);
                        let role = Role::named(vocab.intern_role(ax.dp.0.as_ref()));
                        let range = pool.atomic(vocab.intern_class(&iri));
                        Ok(Some(Axiom::ObjectPropertyRange { role, range }))
                    }
                    // empty range → drop (sound), but this is CONTENT (a
                    // DataPropertyRange axiom), so record it (issue #43 review).
                    DataIntersectionDkey::Empty => Err(ConversionError::UnsupportedDataRange),
                };
            }
            // DataComplementOf: DataPropertyRange(p, DataComplementOf(r)) →
            // ObjectPropertyRange(role, ¬DKey(r)). Semantics: every value on
            // the role must lie outside r. Drop on composite inner (sound).
            if let DataRange::DataComplementOf(inner) = &ax.dr {
                return match data_range_dkey(inner, ax.dp.0.as_ref(), vocab, pool) {
                    Some((role, range)) => Ok(Some(Axiom::ObjectPropertyRange {
                        role,
                        range: pool.not(range),
                    })),
                    // unrecognized inner → drop (sound), but CONTENT — record it.
                    None => Err(ConversionError::UnsupportedDataRange),
                };
            }
            match data_range_dkey(&ax.dr, ax.dp.0.as_ref(), vocab, pool) {
                Some((role, range)) => Ok(Some(Axiom::ObjectPropertyRange { role, range })),
                // unrecognized range → drop (sound), but CONTENT — record it.
                None => Err(ConversionError::UnsupportedDataRange),
            }
        }

        // ── Data property / datatype CONTENT axioms: dropped when the
        // `RUSTDL_DATA_PROPERTIES` gate is OFF (or, for `DatatypeDefinition`,
        // unconditionally — there is no gated lowering for it). Sound
        // under-approximation (see the DeclareDataProperty / DeclareDatatype
        // block above for the rationale), but unlike a bare declaration these
        // are reasoning-load-bearing axioms, so issue #43's review requires
        // recording the drop rather than silently swallowing it.
        #[allow(clippy::match_same_arms)]
        C::SubDataPropertyOf(_)
        | C::EquivalentDataProperties(_)
        | C::DisjointDataProperties(_)
        | C::DataPropertyDomain(_)
        | C::DataPropertyRange(_)
        | C::FunctionalDataProperty(_)
        | C::DatatypeDefinition(_)
        | C::DataPropertyAssertion(_)
        | C::NegativeDataPropertyAssertion(_) => Err(ConversionError::UnsupportedDataRange),

        // ── HasKey: advanced feature, deferred ──────────────────────────
        C::HasKey(_) => Err(ConversionError::UnsupportedAxiom { kind: "HasKey" }),

        // ── SWRL rules: silently skipped ────────────────────────────────
        // DL-safe `Rule` axioms are FOL-style entailment rules over
        // individuals; on real workloads (e.g. RO with 25 such rules)
        // they encode ABox-level inferences (`if x has property P
        // and y holds, then ...`). They don't enter class-side
        // classification — no class definition references their head
        // predicates — so silently dropping them is sound for the
        // `classify` use case. A future `swrl` feature gate could
        // materialise them via tableau extensions if needed.
        #[allow(clippy::match_same_arms)]
        C::Rule(_) => Ok(None),
    }
}

/// Stable discriminant name for the axiom-carrying `Component` variants that
/// can be dropped (i.e. can make [`convert_component`] return `Err`). Used
/// only to build the [`drop_label`] diagnostic string — never affects
/// reasoning. The `_ => "Other"` fallback keeps this sound (just a coarser
/// label) for any variant not spelled out here.
fn component_kind<A: ForIRI>(c: &Component<A>) -> &'static str {
    use Component as C;
    match c {
        C::SubClassOf(_) => "SubClassOf",
        C::EquivalentClasses(_) => "EquivalentClasses",
        C::DisjointClasses(_) => "DisjointClasses",
        C::DisjointUnion(_) => "DisjointUnion",
        C::ClassAssertion(_) => "ClassAssertion",
        C::ObjectPropertyAssertion(_) => "ObjectPropertyAssertion",
        C::NegativeObjectPropertyAssertion(_) => "NegativeObjectPropertyAssertion",
        C::ObjectPropertyDomain(_) => "ObjectPropertyDomain",
        C::ObjectPropertyRange(_) => "ObjectPropertyRange",
        C::SubObjectPropertyOf(_) => "SubObjectPropertyOf",
        C::EquivalentObjectProperties(_) => "EquivalentObjectProperties",
        C::DisjointObjectProperties(_) => "DisjointObjectProperties",
        C::HasKey(_) => "HasKey",
        C::DataPropertyDomain(_) => "DataPropertyDomain",
        C::DataPropertyRange(_) => "DataPropertyRange",
        C::DataPropertyAssertion(_) => "DataPropertyAssertion",
        C::NegativeDataPropertyAssertion(_) => "NegativeDataPropertyAssertion",
        C::SubDataPropertyOf(_) => "SubDataPropertyOf",
        C::EquivalentDataProperties(_) => "EquivalentDataProperties",
        C::DisjointDataProperties(_) => "DisjointDataProperties",
        C::FunctionalDataProperty(_) => "FunctionalDataProperty",
        C::DatatypeDefinition(_) => "DatatypeDefinition",
        _ => "Other",
    }
}

/// `"<component>: <reason>"` — the diagnostic kind label recorded in
/// [`crate::DroppedAxioms`] for a component that [`convert_component`]
/// dropped (returned `Err` for).
fn drop_label<A: ForIRI>(c: &Component<A>, e: &ConversionError) -> String {
    let comp = component_kind(c);
    match e {
        ConversionError::UnsupportedDataRange => format!("{comp}: unsupported data range"),
        ConversionError::AnonymousIndividual => format!("{comp}: anonymous individual"),
        ConversionError::UnsupportedConcept { kind } => {
            format!("{comp}: unsupported concept ({kind})")
        }
        ConversionError::UnsupportedAxiom { kind } => format!("{comp}: unsupported axiom ({kind})"),
    }
}

/// Convert an entire horned-owl [`SetOntology`] into an [`InternalOntology`].
///
/// Issue #43: a component that `convert_component` can't lower no longer
/// aborts the whole conversion. Its error is recorded (via [`drop_label`])
/// into the returned ontology's [`InternalOntology::dropped`] tally instead,
/// and conversion continues with the remaining components — a sound
/// under-approximation (a dropped axiom can only make the resulting KB
/// weaker, never introduce a false entailment). horned-owl iterates a
/// `HashSet`, so the components arrive in HashMap-iteration order (different
/// between processes). Two stabilizations make every downstream pass —
/// vocabulary interning, absorption, saturation, the tableau search —
/// deterministic across runs:
///
/// 1. Sort components by their derived `Ord` *before* lowering, so the
///    sequence of `intern_class` / `intern_role` / `intern_individual`
///    calls is reproducible. This pins `ClassId` / `RoleId` /
///    `IndividualId` assignment (and therefore every `ConceptId` derived
///    from them) to a single canonical order across runs.
/// 2. Sort the lowered axiom list afterwards. Step 1 already guarantees
///    a deterministic sequence given a stable component order, but
///    sorting the output too keeps the contract explicit and survives
///    any future change to lowering that might shuffle ordering.
///
/// Same input → same axiom vector → reproducible reasoning behaviour and
/// timings.
pub fn convert_ontology<A: ForIRI>(
    src: &SetOntology<A>,
) -> Result<InternalOntology, ConversionError> {
    let mut components: Vec<&AnnotatedComponent<A>> = src.iter().collect();
    components.sort();
    let mut out = InternalOntology::new();
    for ac in components {
        match convert_component(&ac.component, &mut out.vocabulary, &mut out.concepts) {
            Ok(Some(axiom)) => out.axioms.push(axiom),
            Ok(None) => {} // benign: metadata / annotation / declaration — no reasoning content
            Err(e) => out.dropped.record(drop_label(&ac.component, &e)),
        }
    }
    // Phase D4 (2026-06-03): scan for data-axiom patterns the main
    // conversion dropped (DeclareDataProperty, DataMin/Max, Functional,
    // DataPropertyDomain, SubDataPropertyOf, DataSome) and emit derived
    // class-subsumption / unsat axioms. The vocabulary is now fully
    // populated so class IRIs resolve. Sound under-approximation:
    // patterns we don't recognize stay dropped; recognized patterns
    // contribute additional axioms that close specific completeness
    // gaps without changing any other behavior. See
    // crates/owl-dl-core/src/data_axioms.rs for the pattern docs +
    // crates/owl-dl-reasoner/tests/datatype_completeness.rs for the
    // TDD harness.
    let bot_id = out.concepts.bot();
    let top_id = out.concepts.top();
    // We intern atomic concept lookups inside the closure so the pool
    // gets all referenced atomic classes (some may not have been
    // referenced by any axiom that survived ce_or_skip!).
    // RefCell scoped tightly so its borrow on out.concepts ends before
    // out.axioms.extend (which doesn't need it but reads cleaner).
    let derived = {
        let concepts_cell = std::cell::RefCell::new(&mut out.concepts);
        crate::data_axioms::derive_data_axioms(src, &out.vocabulary, top_id, bot_id, |cid| {
            concepts_cell.borrow_mut().atomic(cid)
        })
    };
    out.axioms.extend(derived);
    // Seed told-subsumptions between the synthetic `DKey(range)` filler
    // classes introduced by the integer-facet data lowering above:
    // `DKey(r1) ⊑ DKey(r2)` iff `r1 ⊆ r2` (and `r1` is the sub).
    // This is the ONE missing inference that makes
    // `DataHasValue(p, v) ⊑ DataSomeValuesFrom(p, range)` hold when
    // `v ∈ range` — the property match is handled by CR5 (both lower to
    // `∃p.DKey(...)` on the same role). Sound by construction: every
    // emitted edge is a genuine value-space containment. The `DKey`
    // classes never appear in the reported class list (filtered by
    // `DKEY_IRI_PREFIX` in the reasoner), so these edges add no output
    // noise — they only relay through the existential machinery.
    seed_dkey_subsumptions(&mut out);
    // Disjunctive data-property domains: for `DataPropertyDomain(dp,
    // D₁ ⊔ … ⊔ Dₙ)` (all atomic) and each class `C` using `dp`, emit the
    // bare disjunctive GCI `C ⊑ (D₁ ⊔ … ⊔ Dₙ)`. Sound (`C ⊑ ∃dp.⊤ ⊑ ⊔Dᵢ`).
    // We build the union here because the `ConceptPool` lives on `out`;
    // `data_axioms` only returns the resolved class ids. The bare GCI is
    // then folded to `C ⊑ E` by `derive_disjunction_existentials` below
    // (common told-subsumer) and also case-split natively by the tableau.
    // Closes the SAO/BFO cross-ontology cluster — see
    // `docs/sao-bfo-chain-2026-06-10.md`.
    for (c_id, disjunct_ids) in crate::data_axioms::derive_data_domain_unions(src, &out.vocabulary)
    {
        let sub = out.concepts.atomic(c_id);
        let members: Vec<_> = disjunct_ids
            .into_iter()
            .map(|d| out.concepts.atomic(d))
            .collect();
        let sup = out.concepts.or(members);
        if sub != sup {
            out.axioms.push(Axiom::SubClassOf { sub, sup });
        }
    }
    // Split a union on the LHS of a subclass axiom into one axiom per disjunct
    // — `(D₁ ⊔ … ⊔ Dₙ) ⊑ C ≡ ⋀ Dᵢ ⊑ C` (sound equivalence). Runs first so
    // both the EL saturator (which drops union-LHS) and the disjunction-
    // existential / told-table passes below see the atomic-LHS form.
    crate::disjunctive_antecedent::split_disjunctive_antecedents(&mut out);
    // Derive `X ⊑ ∃R.C` from `X ⊑ ∃R.(D₁ ⊔ … ⊔ Dₙ)` when the disjuncts
    // share a told-subsumer C (sound under-approximation; feeds the EL
    // saturator a case-split it otherwise drops). Runs on the fully
    // populated IR.
    crate::disjunction_existential::derive_disjunction_existentials(&mut out);
    // SP-A: forced-disjunct over atomic disjunctions. Runs AFTER
    // derive_disjunction_existentials so it sees the common-subsumer axioms that
    // pass adds (richer told tables ⟹ more forcings). Sound; atomic-only.
    crate::approx_saturation::derive_forced_disjuncts(&mut out);
    // Canonicalize `X ⊑ ¬Y` into `X ⊓ Y ⊑ ⊥` (a logical equivalence). The gates
    // reject `ConceptExpr::Not` outright, so one negated GCI routes an
    // otherwise-EL ontology onto the O(n²) hybrid path; the lowered-⊥ form is
    // in-fragment (Lever 1b) and completely reasoned over by the saturator's
    // ConjunctiveUnsat rule. Runs on the fully populated IR, and BEFORE any NNF
    // view is taken — `nnf_axioms` would already have turned `¬(A ⊓ B)` into an
    // `Or`. Gated RUSTDL_NEG_TO_BOT_GCI (default ON).
    let _ = crate::negation_gci::rewrite_negated_supers(&mut out);
    // HF3: decompose role chains longer than 2 legs into a cascade of
    // 2-leg chains using fresh auxiliary roles, so both the wedge
    // clausifier (which only encodes 2-leg chains) and the main tableau
    // (`collect_chain_axioms`, len==2 only) pick them up. Runs late so the
    // vocabulary is fully populated; allocates aux roles IN the vocabulary
    // so `num_roles()` grows and `build_role_hierarchy` / the engine stay
    // consistent. Sound + additive — see `decompose_long_chains`.
    decompose_long_chains(&mut out);
    // Functional object-property ENFORCEMENT for the tableau + hypertableau
    // wedge. `FunctionalRole(R)` is read by the EL saturator's bitset
    // machinery (classify), but the wedge clausifier DROPS it and the main
    // tableau never sees a `≤1 R` constraint — so consistency / ABox-merge /
    // non-EL paths miss functional-merge clashes. We emit a derived
    // ROLE-TRIGGERED GCI `∃R.⊤ ⊑ ≤1 R` that both engines pick up through
    // existing machinery (the wedge's `Some`-antecedent + `Max`-consequent
    // clausifier; the tableau's `apply_max`). The `FunctionalRole` axiom is
    // KEPT (the saturator still reads it).
    //
    // FORWARD ONLY. Inverse-functional (`InverseFunctionalRole(R)`) is NOT
    // translated: the engine does not perform `≤1 R⁻` predecessor-merge even
    // for an EXPLICIT `ObjectMaxCardinality(1, R⁻)` (proven — that case still
    // reports `consistent`), so emitting `∃R⁻.⊤ ⊑ ≤1 R⁻` would be a silent
    // no-op. Inverse-functional enforcement is a documented sound MISS pending
    // an engine fix to inverse-role predecessor merging. See
    // `docs/superpowers/specs/2026-06-15-functional-role-enforcement-design.md`.
    // Propagate functionality ACROSS a declared inverse pair. Runs **AFTER**
    // `derive_functional_max_cardinality`, and the order is load-bearing.
    //
    // It originally ran before, so that a newly-derived functional role also picked up
    // that pass's `∃R.⊤ ⊑ ≤1 R` enforcement GCI. That was measured to be the wrong
    // choice: a 1,920-ontology sweep found **4 ontologies going from 1–5 s to
    // non-terminating** (`ore_ont_16372`, `7532`, `9662`, `9786`), each carrying 76–118
    // inverse pairs and ~21 functional roles, so the derivation marks many roles
    // functional and every one adds a `≤1` constraint for `apply_max` to police.
    // Diagnostic: `ore_ont_7532`'s `role_rules_unguarded` went 80 → 81, and none of the
    // four has a single `ObjectPropertyAssertion`, which rules out the materialisation
    // half as the cause.
    //
    // Running after keeps the cheap win and drops the expensive one: the derived
    // characteristics still reach `abox_check`'s P5 (which reads axioms, not GCIs), while
    // no derived-functional role gains an enforcement GCI. The materialisation half is
    // unaffected because it fires on a role whose functionality is DECLARED, so the `≤1`
    // GCI it needs already exists.
    derive_functional_max_cardinality(&mut out);
    derive_inverse_pair_functionality(&mut out);
    out.axioms.sort();
    Ok(out)
}

/// `RUSTDL_INVERSE_PAIR_FUNC` — derive functionality across a declared inverse
/// pair. **Default OFF**; `=1` enables.
#[must_use]
pub fn inverse_pair_functionality_enabled() -> bool {
    std::env::var_os("RUSTDL_INVERSE_PAIR_FUNC").is_some_and(|v| v == "1")
}

/// From `InverseObjectProperties(A, B)` — i.e. `B ≡ A⁻` — derive the functionality
/// characteristic of each partner from the other:
///
/// ```text
/// Functional(A)        ⟹ InverseFunctional(B)
/// Functional(B)        ⟹ InverseFunctional(A)
/// InverseFunctional(A) ⟹ Functional(B)
/// InverseFunctional(B) ⟹ Functional(A)
/// ```
///
/// **Valid, not heuristic.** `Functional(A)` says every `x` has at most one
/// `A`-successor. With `B(x,y) ⟺ A(y,x)`, that is precisely "every `y` has at most
/// one `B`-predecessor", which is what `InverseFunctional(B)` means. The other three
/// follow by the symmetry of `InverseObjectProperties` and `A ≡ B⁻`.
///
/// **Why this is needed** (`docs/known-limitations/inverse-pair-functionality-not-derived.md`):
/// `abox_check`'s P5 handles a *declared* `InverseFunctionalRole` correctly, but nothing
/// derived one, so a 5-axiom `ABox` that `Konclude` and `HermiT` both call inconsistent was
/// reported `consistent`. Delta-debugging `ore_ont_4141` (67,143 axioms → a 7-axiom
/// core) landed on exactly this.
///
/// **Deliberately conservative:** only axioms whose role is `Role::Named` participate.
/// `Functional(ObjectInverseOf(p))` is *equivalent* to `InverseFunctional(p)` and could
/// be folded in, but normalising polarity here would widen the change for a shape that
/// does not occur in the motivating corpus; it stays a documented residual.
///
/// **The risk direction is INVERTED from most passes here.** This ADDS characteristics,
/// making the KB stronger and more clashes derivable, so the failure mode is a FALSE
/// POSITIVE rather than a miss. It also feeds the `merge_inducing` / `collapse` sets at
/// the `DKey` gate (`:3066`, `:3169`), where marking extra roles merge-inducing can
/// re-inflate the O(k²) disjointness seeding that gate exists to suppress — so
/// `ore_ont_9347` (113 concept rules) and `ore_ont_5368` (18,620,251) are load-bearing
/// gates for any default flip, not optional extras.
fn derive_inverse_pair_functionality(out: &mut InternalOntology) {
    if !inverse_pair_functionality_enabled() {
        return;
    }
    let mut functional: std::collections::HashSet<crate::ir::RoleId> =
        std::collections::HashSet::new();
    let mut inverse_functional: std::collections::HashSet<crate::ir::RoleId> =
        std::collections::HashSet::new();
    let mut pairs: Vec<(crate::ir::RoleId, crate::ir::RoleId)> = Vec::new();
    for axiom in &out.axioms {
        match axiom {
            Axiom::FunctionalRole(r) if !r.is_inverse() => {
                functional.insert(r.role_id());
            }
            Axiom::InverseFunctionalRole(r) if !r.is_inverse() => {
                inverse_functional.insert(r.role_id());
            }
            Axiom::InverseObjectProperties(a, b) if !a.is_inverse() && !b.is_inverse() => {
                pairs.push((a.role_id(), b.role_id()));
            }
            _ => {}
        }
    }
    // Fixpoint: a derived characteristic can feed another inverse pair (`p⁻ = q`,
    // `q⁻ = r`), so iterate until nothing new appears rather than making one pass.
    // Bounded by 2 × |roles| insertions, so it always terminates.
    let mut added: Vec<Axiom> = Vec::new();
    loop {
        let mut grew = false;
        for &(a, b) in &pairs {
            for (src, dst) in [(a, b), (b, a)] {
                if functional.contains(&src) && inverse_functional.insert(dst) {
                    added.push(Axiom::InverseFunctionalRole(Role::named(dst)));
                    grew = true;
                }
                if inverse_functional.contains(&src) && functional.insert(dst) {
                    added.push(Axiom::FunctionalRole(Role::named(dst)));
                    grew = true;
                }
            }
        }
        if !grew {
            break;
        }
    }
    out.axioms.extend(added);

    // ---- Part 2: materialise the inverse ABox edges for FUNCTIONAL partners ----
    //
    // Deriving the characteristic is necessary but NOT sufficient. The engine cannot
    // merge PREDECESSORS: `derive_functional_max_cardinality` is deliberately
    // forward-only because `∃R⁻.⊤ ⊑ ≤1 R⁻` is a measured no-op (see its comment). So
    // `InverseFunctionalRole(S)` alone still leaves the clash undetected on the
    // tableau path, which is where the *direct* analogue is actually caught — probes
    // confirm the direct case is decided even with BOTH ABox pre-checks disabled.
    //
    // The fix reuses that proven forward path instead of building predecessor merge:
    // for a declared pair `(R, S)` where `R` is functional, every asserted `S(a, b)`
    // entails `R(b, a)`, so materialising it lets the existing
    // `∃R.⊤ ⊑ ≤1 R` + `apply_max` fire at `b`. Verified by hand-adding the edges to
    // the reproducer: `consistent` → `inconsistent`.
    //
    // Sound: `InverseObjectProperties(R, S)` makes `R(b, a)` an ENTAILED ground fact,
    // so this adds no model. **Bounded on purpose:** materialisation happens only when
    // the target role is functional (declared or derived above), so edge growth is
    // proportional to assertions on the partner of a functional role rather than to the
    // whole ABox. An unbounded version would double the edge count of every ontology
    // declaring an inverse pair.
    let mut inverse_of: std::collections::HashMap<crate::ir::RoleId, Vec<crate::ir::RoleId>> =
        std::collections::HashMap::new();
    for &(a, b) in &pairs {
        inverse_of.entry(a).or_default().push(b);
        inverse_of.entry(b).or_default().push(a);
    }
    let existing: std::collections::HashSet<(crate::ir::RoleId, IndividualId, IndividualId)> = out
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::ObjectPropertyAssertion {
                role,
                subject,
                object,
            } if !role.is_inverse() => Some((role.role_id(), *subject, *object)),
            _ => None,
        })
        .collect();
    let mut edges: Vec<Axiom> = Vec::new();
    let mut seen = existing.clone();
    for ax in &out.axioms {
        let Axiom::ObjectPropertyAssertion {
            role,
            subject,
            object,
        } = ax
        else {
            continue;
        };
        if role.is_inverse() {
            continue;
        }
        for &partner in inverse_of.get(&role.role_id()).into_iter().flatten() {
            // Only where it can matter: the partner must be functional, so that the
            // forward `≤1` constraint exists to fire on the materialised edge.
            if !functional.contains(&partner) {
                continue;
            }
            if seen.insert((partner, *object, *subject)) {
                edges.push(Axiom::ObjectPropertyAssertion {
                    role: Role::named(partner),
                    subject: *object,
                    object: *subject,
                });
            }
        }
    }
    out.axioms.extend(edges);
}

/// Whether `InverseFunctionalRole(r)` also derives `∃r⁻.⊤ ⊑ ≤1 r⁻.⊤`
/// (`RUSTDL_INVERSE_FUNC_MAX`, **default OFF**).
///
/// **Logically equivalent, so FP-safe:** `InverseFunctional(r)` ⟺ `Functional(r⁻)` ⟺
/// `⊤ ⊑ ≤1 r⁻`, and the `∃r⁻.⊤` guard only makes it vacuous where there are no
/// `r⁻`-successors. The GCI is entailed, so it can add entailments but never
/// manufacture one.
///
/// **Default OFF pending measurement, not because of doubt about the semantics.** It
/// emits axioms into every ontology carrying an inverse-functional role, which (a) can
/// change classify output — soundly, by finding more — and (b) interacts with the
/// fragment gates, since a `Max` is normally disqualifying and only the exact derived
/// shape is whitelisted (`classify::is_derived_functional_max`). Both need a corpus
/// sweep before this becomes a default.
#[must_use]
pub fn inverse_functional_max_enabled() -> bool {
    std::env::var_os("RUSTDL_INVERSE_FUNC_MAX").is_some_and(|v| v == "1")
}

/// Skip a `DKey` disjointness component wholesale when every one of its pairs is provably
/// droppable (`RUSTDL_DKEY_GROUP_SKIP`, **default OFF** pending measurement).
///
/// The drop condition is component-level, not per-pair, so the O(k²) walk can be replaced
/// by an O(k) test. Measured waste it ENUMERATES AWAY: `ore_ont_10929` 248,465,112
/// pair-visits at a 100% drop rate, `ore_ont_15635` 294,744,041.
///
/// **PARTIAL — it does not remove those ontologies' conversion cost.** Measured
/// `ore_ont_10929` 96.5 s → 77.5 s (1.24×) and `ore_ont_15635` 92.2 s → 67.4 s (1.37×),
/// against the 2.6 s that `RUSTDL_DATA_PROPERTIES=0` reaches — so ~77 s of 96 s remains.
/// This touches only the DISJOINTNESS loop; the leading suspect for the residual is
/// `seed_bucket`'s SUBSUMPTION seeding, which walks k² ORDERED pairs (~3.6 × 10⁹ at that
/// ontology's 60,323 distinct string keys). That attribution is arithmetic, not a
/// measurement — probe the two seeding calls before touching the loop.
///
/// See `docs/benchmarks/2026-08-20-dkey-residual-class-unpark-case.md`.
#[must_use]
pub fn dkey_group_skip_enabled() -> bool {
    std::env::var_os("RUSTDL_DKEY_GROUP_SKIP").is_some_and(|v| v == "1")
}

/// Emit a derived role-triggered `≤1` GCI for every (forward) functional
/// object property.
///
/// For `Axiom::FunctionalRole(R)` emit `SubClassOf(∃R.⊤, ≤1 R)`. The `≤1` is
/// UNQUALIFIED (`Max(1, role, ⊤)`).
///
/// SOUNDNESS: the emitted GCI is EXACTLY the axiom's meaning —
/// `FunctionalRole(R) ≡ ⊤ ⊑ ≤1R`, and the role-triggered `∃R.⊤ ⊑ ≤1R` is
/// satisfiability-equivalent (a node with no `R`-successor trivially satisfies
/// `≤1R`). Additive: it can only enable genuine `≤1`-merge clashes, never
/// spurious ones (the engines' merge is sound). The original `FunctionalRole`
/// axiom is left in place so the saturator's bitset handling is untouched.
///
/// Translates `Axiom::FunctionalRole(R)` always, and
/// `Axiom::InverseFunctionalRole(R)` under `RUSTDL_INVERSE_FUNC_MAX` (default OFF).
///
/// **CORRECTED 2026-08-18.** This comment used to say inverse-functionality was
/// "deliberately NOT translated: the engine does not perform `≤1 R⁻` predecessor
/// merges (even explicit ones — verified), so the GCI would be a silent no-op."
/// **That is false, and it was load-bearing** — it is why the inverse GCI went
/// unwritten for months. `HyperEngine` DOES perform them: `hyper.rs` walks a node's
/// `preds` and merges `R`-predecessors, guarded by `inverse_func_merge`
/// (`RUSTDL_INVERSE_FUNC_MERGE`, **default ON** since 2026-07-11). That merge is
/// triggered by an explicit `≤1` constraint on the node — so the GCI is not a no-op,
/// it is the *only* thing that supplies the trigger. Its absence is what made
/// inverse-functional-forced individual equality invisible to `realize`
/// (`docs/known-limitations/realize-drops-derived-individual-equality.md`).
///
/// NOTE: a `FunctionalObjectProperty(ObjectInverseOf(r))`
/// — inverse-functionality written the other way — converts to
/// `Axiom::FunctionalRole(R⁻)` and DOES get `∃R⁻.⊤ ⊑ ≤1 R⁻` emitted; that is
/// sound (correct functional semantics on the inverse role) and routes to the
/// hybrid path (the fast-path gate rejects inverse roles), where it is harmless.
///
/// PERF: role-triggered (`∃R.⊤ ⊑ ≤1R`), NOT global (`⊤ ⊑ ≤1R`) — fires merge
/// work only on nodes that already have an `R`-successor.
///
/// The derived `SubClassOf{Some(R,⊤), Max(1,R,⊤)}` shape is recognized by
/// `saturator_complete_fragment` (classify.rs) so EL+functional ontologies
/// (GALEN/notgalen) stay on the saturation fast path — the derived `≤n` must
/// not kick them onto the slower hybrid path.
fn derive_functional_max_cardinality(out: &mut InternalOntology) {
    // Collect the forward functional roles to constrain. Duplicate GCIs are
    // harmless — the pool interns the shape and `out.axioms.sort()` keeps the
    // axiom list canonical — but dedup the source roles to avoid emitting the
    // same GCI twice.
    let mut roles: Vec<Role> = out
        .axioms
        .iter()
        .filter_map(|ax| match ax {
            Axiom::FunctionalRole(r) => Some(*r),
            // `InverseFunctional(r)` ⟺ `Functional(r⁻)`, so the analogous derived GCI
            // is the same bound on the INVERSE role. Without it the wedge has no
            // `at_most` constraint to enforce, and `HyperEngine`'s
            // predecessor-walking merge (`hyper.rs`, guarded by
            // `inverse_func_merge`) is never triggered — which is why
            // inverse-functional-forced individual equality was invisible to the
            // `ABox`-seeded witness while the functional case worked.
            // See `docs/known-limitations/realize-drops-derived-individual-equality.md`.
            Axiom::InverseFunctionalRole(r) if inverse_functional_max_enabled() => Some(r.flip()),
            _ => None,
        })
        .collect();
    if roles.is_empty() {
        return;
    }
    roles.sort_by_key(|r| (r.role_id().index(), r.is_inverse()));
    roles.dedup();
    let top = out.concepts.top();
    for role in roles {
        let sub = out.concepts.some(role, top);
        let sup = out.concepts.max(1, role, top);
        out.axioms.push(Axiom::SubClassOf { sub, sup });
    }
}

/// Decompose every `SubObjectPropertyOf{ Chain(parts), sup }` with
/// `parts.len() > 2` into a left-associative cascade of length-2 chains
/// joined by FRESH auxiliary roles, replacing the original axiom:
///
/// ```text
///   R₁∘R₂∘R₃ ⊑ S   ≡   R₁∘R₂ ⊑ aux₀ ,  aux₀∘R₃ ⊑ S
///   R₁∘R₂∘R₃∘R₄ ⊑ S ≡  R₁∘R₂ ⊑ aux₀ ,  aux₀∘R₃ ⊑ aux₁ ,  aux₁∘R₄ ⊑ S
/// ```
///
/// SOUNDNESS:
/// - The decomposition is an EXACT equivalence for any associativity
///   (each aux denotes precisely its prefix composition).
/// - **Aux IRIs are unique per decomposition site**
///   (`urn:rustdl-aux-role:<axiom-idx>:<leg-idx>`), so an aux role is
///   PRODUCED only by its prefix chain and CONSUMED only by its suffix
///   chain — never shared across two different compositions (which would
///   be a silent FP). No common-prefix CSE (sound only if the shared aux
///   denotes the identical leg-prefix; not worth the FP risk).
/// - Allocating aux roles via `intern_role` grows `vocabulary.num_roles()`,
///   keeping the clausifier, `build_role_hierarchy`, and the engine's
///   role indices mutually consistent (the role hierarchy is sized to
///   `num_roles()` and panics on out-of-range ids).
/// - Aux roles appear ONLY in the two decomposed chain axioms — they have
///   no hierarchy edges of their own and no class-level use, so they are
///   opaque connectors. They are interned by IRI in the reserved
///   `urn:rustdl-aux-role:` namespace, unique per `(axiom-idx, leg-idx)`, so
///   they collide only with an adversarially-named declared role in that
///   reserved namespace (the same risk profile as the `urn:rustdl-dkey:`
///   IRIs) — never with an ordinary declared role.
///
/// Length-≤2 chains and `TransitiveRole` are untouched.
fn decompose_long_chains(out: &mut InternalOntology) {
    // Collect the long chains with their original index (for unique aux
    // IRIs), then rebuild the axiom list.
    let long: Vec<(usize, Vec<Role>, Role)> = out
        .axioms
        .iter()
        .enumerate()
        .filter_map(|(i, ax)| match ax {
            Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(parts),
                sup,
            } if parts.len() > 2 => Some((i, parts.clone(), *sup)),
            _ => None,
        })
        .collect();
    if long.is_empty() {
        return;
    }
    // Remove the originals (mark by index), then append the decomposition.
    let drop_idx: std::collections::HashSet<usize> = long.iter().map(|(i, _, _)| *i).collect();
    let kept: Vec<Axiom> = out
        .axioms
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop_idx.contains(i))
        .map(|(_, ax)| ax.clone())
        .collect();
    out.axioms = kept;
    for (axiom_idx, parts, sup) in long {
        // Left-fold: cur ∘ parts[i] ⊑ (aux | sup).
        let mut cur = parts[0];
        let last = parts.len() - 1;
        for (leg_idx, &leg) in parts.iter().enumerate().skip(1) {
            let target = if leg_idx == last {
                sup
            } else {
                let iri = format!("urn:rustdl-aux-role:{axiom_idx}:{leg_idx}");
                Role::Named(out.vocabulary.intern_role(&iri))
            };
            out.axioms.push(Axiom::SubObjectPropertyOf {
                sub: SubRolePath::Chain(vec![cur, leg]),
                sup: target,
            });
            cur = target;
        }
    }
}

/// Emit `SubClassOf(DKey(r1), DKey(r2))` for every ordered pair of
/// distinct synthetic `DKey` filler classes where `r1 ⊆ r2`. O(k²) in
/// the number of distinct integer ranges (small in practice — bounded
/// by the count of distinct facet/value combinations in the ontology).
fn seed_dkey_subsumptions(out: &mut InternalOntology) {
    // Bucket DKeys BY DATATYPE so edges are only ever seeded WITHIN a
    // bucket — never across datatypes. Cross-datatype subsumption would be
    // unsound (integer / real / decimal / temporal value spaces are
    // disjoint for our purposes; deliberate conservative under-approx).
    // The decoders are pairwise mutually exclusive on IRIs (the
    // `parser_matrix_*` canaries pin this), so a given `DKey` IRI lands in
    // exactly one bucket.
    let int_dkeys: Vec<(ClassId, IntegerRange)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    // xsd:float (f:) and xsd:double (db:) are SEPARATE buckets — they have
    // disjoint OWL value spaces (float is f32, double is f64); cross-bucket
    // subsumption would be unsound.
    let float_dkeys: Vec<(ClassId, FloatRange)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_float_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    let double_dkeys: Vec<(ClassId, FloatRange)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_double_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    let dec_dkeys: Vec<(ClassId, OrdRange<Decimal>)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_decimal_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    let date_dkeys: Vec<(ClassId, OrdRange<DateKey>)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_date_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    let dt_dkeys: Vec<(ClassId, OrdRange<DateTimeKey>)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_datetime_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    let str_dkeys: Vec<(ClassId, StrSet)> = out
        .vocabulary
        .classes()
        .filter_map(|(cid, iri)| parse_string_dkey_iri(iri).map(|r| (cid, r)))
        .collect();
    // ── The six NUMERIC `DataOneOf` buckets (`io:` / `fo:` / `dbo:` / `deo:` /
    // `dao:` / `dto:`) ────────────────────────────────────────────────────────
    // These were minted by `data_range_dkey` but NEVER collected here, so they
    // got neither told `DKey ⊑ DKey` edges nor `DisjointClasses(DKey, DKey)`
    // entries — while `is_pure_el` still certified the saturator-only closure
    // COMPLETE (`incomplete: false`). That is the D10 failure class: the gate
    // says complete, the engine drops the axiom. The `str:` bucket (the seventh
    // enumeration bucket) was always seeded; these five/six were the asymmetry.
    //
    // Each is a `BTreeSet<T>` over an EXACTLY-keyed value domain, so set
    // inclusion IS `⊑` and set intersection-emptiness IS provable disjointness —
    // no boundary algebra, unlike the interval buckets. Same `seed_bucket` /
    // `seed_disjoint_bucket` route as every other bucket; strictly WITHIN a
    // bucket, so int / float / double / decimal / date / dateTime never
    // cross-subsume (the `numeric_oneof_parser_matrix_exclusivity` canary pins
    // the decoders pairwise-exclusive).
    //
    // Gated `RUSTDL_DKEY_ONEOF_SEED` (default ON since 2026-08-03; `=0` reverts).
    let oneof_seed = dkey_oneof_seed_enabled();
    let int_oneof_dkeys: OneofBucket<i64> =
        collect_oneof_dkeys(out, oneof_seed, parse_int_oneof_iri);
    let float_oneof_dkeys: OneofBucket<crate::data_axioms::OrdF64> =
        collect_oneof_dkeys(out, oneof_seed, parse_float_oneof_iri);
    let double_oneof_dkeys: OneofBucket<crate::data_axioms::OrdF64> =
        collect_oneof_dkeys(out, oneof_seed, parse_double_oneof_iri);
    let dec_oneof_dkeys: OneofBucket<Decimal> =
        collect_oneof_dkeys(out, oneof_seed, parse_decimal_oneof_iri);
    let date_oneof_dkeys: OneofBucket<DateKey> =
        collect_oneof_dkeys(out, oneof_seed, parse_date_oneof_iri);
    let dt_oneof_dkeys: OneofBucket<DateTimeKey> =
        collect_oneof_dkeys(out, oneof_seed, parse_datetime_oneof_iri);
    // Bounded DKey-disjointness seeding (2026-07-20): compute the merge-aware
    // role-component map NOW — before `seed_bucket` pushes the told
    // `DKey ⊑ DKey` edges, whose bare-atomic DKey operands would otherwise be
    // conservatively classified as "unanchored" (= pair with everything) by
    // the axiom scan, re-inflating the pair count.
    let bounded_components = bounded_dkey_disjoint_enabled().then(|| dkey_components(out));
    // `DKey(r1) ⊑ DKey(r2)` iff `r1 ⊆ r2` (distinct keys ⟹ strict subset,
    // since equal ranges share one ClassId). Integer/float ranges are
    // `Copy`; the ordered ranges compare by reference.
    seed_bucket(out, &int_dkeys, |a, b| a.subset(*b));
    seed_bucket(out, &float_dkeys, |a, b| a.subset(*b));
    seed_bucket(out, &double_dkeys, |a, b| a.subset(*b));
    seed_bucket(out, &dec_dkeys, OrdRange::subset);
    seed_bucket(out, &date_dkeys, OrdRange::subset);
    seed_bucket(out, &dt_dkeys, OrdRange::subset);
    seed_bucket(out, &str_dkeys, StrSet::subset);
    // Numeric-oneof `⊑`: `DKey(S1) ⊑ DKey(S2)` iff `S1 ⊆ S2` (exact set
    // inclusion — every member of `S1` is a member of `S2`, so any value in
    // `S1` is in `S2`). Empty when the flag is off ⟹ these are no-ops.
    seed_bucket(out, &int_oneof_dkeys, std::collections::BTreeSet::is_subset);
    seed_bucket(
        out,
        &float_oneof_dkeys,
        std::collections::BTreeSet::is_subset,
    );
    seed_bucket(
        out,
        &double_oneof_dkeys,
        std::collections::BTreeSet::is_subset,
    );
    seed_bucket(out, &dec_oneof_dkeys, std::collections::BTreeSet::is_subset);
    seed_bucket(
        out,
        &date_oneof_dkeys,
        std::collections::BTreeSet::is_subset,
    );
    seed_bucket(out, &dt_oneof_dkeys, std::collections::BTreeSet::is_subset);

    // Phase D11b: `DisjointClasses(DKey(ra), DKey(rb))` for every PROVABLY
    // disjoint pair within a bucket — the basis of the `∃p.DKey(v) ⊓
    // ∀p.DKey(r)` membership clash (v ∉ r) AND the functional / `≤1`-data-
    // property MERGE clash (`∃p.DKey(v1) ⊓ ∃p.DKey(v2)` at one node, v1≠v2).
    // `disjoint` is conservative (true only when no value is shared), so a
    // wrong "disjoint" — which would spuriously make a class ⊥ = FP — cannot
    // arise from overlapping ranges. Same datatype bucketing: int / float /
    // double / decimal / date / dateTime / string never cross-seed.
    //
    // BOUNDED (2026-07-20, `RUSTDL_BOUNDED_DKEY_DISJOINT`, default ON): the
    // unconditional all-pairs seeding is O(k²) in the number of distinct data
    // values k, which is LARGE when DKeys come from ABox
    // `DataPropertyAssertion`s (ore_ont_10425: 5261 values → ~14M axioms →
    // front-end conversion DNF). A disjointness axiom is only ever CONSUMED
    // when both DKeys land in ONE node label, which requires their data roles
    // to be connected through a merge-inducing super-role (functional /
    // inverse-functional / in a `≤n` / carrying a `∀role.DKey` or a
    // DKey-range) — see `dkey_components`. Seeding only within those
    // merge-aware role components drops zero consumable clash (NOT
    // co-occurrence guessing; cross-component DKeys provably never share a
    // label) and cuts the volume to O(Σ_component values²). Two prior
    // dead-ends baked in here: the pairs are GROUPED by component first (a
    // per-pair filter over the O(k²) walk still stalls), and the union is
    // gated on merge-inducing supers ONLY (unioning on every `SubProperty`
    // collapses everything under an `owl:topDataProperty`-style root back to
    // one O(k²) component). `=0` restores the unconditional all-pairs path.
    // Spec: docs/superpowers/specs/2026-07-20-dkey-disjointness-bounded-seeding-spec.md
    let comp = bounded_components.as_ref();
    seed_disjoint_bucket(out, &int_dkeys, |a, b| a.disjoint(*b), comp);
    seed_disjoint_bucket(out, &float_dkeys, |a, b| a.disjoint(*b), comp);
    seed_disjoint_bucket(out, &double_dkeys, |a, b| a.disjoint(*b), comp);
    seed_disjoint_bucket(out, &dec_dkeys, OrdRange::disjoint, comp);
    seed_disjoint_bucket(out, &date_dkeys, OrdRange::disjoint, comp);
    seed_disjoint_bucket(out, &dt_dkeys, OrdRange::disjoint, comp);
    seed_disjoint_bucket(out, &str_dkeys, StrSet::disjoint, comp);
    // Numeric-oneof disjointness. FP-CRITICAL DIRECTION: emitting a
    // `DisjointClasses` ADDS clashes, so a wrong "disjoint" is a false UNSAT,
    // not a miss. `BTreeSet::is_disjoint` is exact here because every bucket's
    // key type is an EXACT representative of its OWL value (i64 for
    // `xsd:integer`; the normalized-lexical `Decimal` — never an `f64`, whose
    // rounding would make two distinct decimals collide; timezone-free
    // component tuples for `date`/`dateTime`; and, for the two IEEE buckets,
    // signed-zero-normalized `OrdF64` at the bucket's OWN precision — `fo:`
    // f32-rounded, `dbo:` f64 — kept in SEPARATE buckets because OWL 2 gives
    // `xsd:float` and `xsd:double` disjoint value spaces). So distinct keys
    // ⟹ distinct values ⟹ the sets really are disjoint.
    seed_disjoint_bucket(
        out,
        &int_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
    seed_disjoint_bucket(
        out,
        &float_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
    seed_disjoint_bucket(
        out,
        &double_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
    seed_disjoint_bucket(
        out,
        &dec_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
    seed_disjoint_bucket(
        out,
        &date_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
    seed_disjoint_bucket(
        out,
        &dt_oneof_dkeys,
        std::collections::BTreeSet::is_disjoint,
        comp,
    );
}

/// One numeric-`DataOneOf` bucket: the `DKey` classes of that datatype paired
/// with their decoded value SETS. Empty when `RUSTDL_DKEY_ONEOF_SEED` is off.
type OneofBucket<T> = Vec<(ClassId, std::collections::BTreeSet<T>)>;

/// Collect every numeric-`DataOneOf` `DKey` class whose IRI `decode` recognizes,
/// paired with its decoded value set. Returns EMPTY when `enabled` is false, so
/// the flag-off path feeds `seed_bucket` / `seed_disjoint_bucket` nothing and is
/// byte-identical to not calling them at all.
fn collect_oneof_dkeys<T: Ord>(
    out: &InternalOntology,
    enabled: bool,
    decode: impl Fn(&str) -> Option<std::collections::BTreeSet<T>>,
) -> OneofBucket<T> {
    if !enabled {
        return Vec::new();
    }
    out.vocabulary
        .classes()
        .filter_map(|(cid, iri)| decode(iri).map(|r| (cid, r)))
        .collect()
}

/// Numeric-`DataOneOf` `DKey` seeding (2026-08-01). **Default ON since
/// 2026-08-03** (`=0` reverts) — seeds told `DKey ⊑ DKey` edges and
/// `DisjointClasses(DKey, DKey)` entries for the six numeric enumeration
/// buckets (`io:` / `fo:` / `dbo:` / `deo:` / `dao:` / `dto:`), which were
/// minted but never collected into `seed_dkey_subsumptions` — the sixth
/// D10-class bug (the gate certifies the closure complete while the engine
/// drops the axiom).
///
/// # Why it shipped OFF, and what changed
///
/// It was held back solely for a volume measurement: the disjointness half is
/// `O(k²)` per component, the shape that caused the v0.3.29 conversion DNFs.
/// That scan is now done — `rustdl tbox-stats` over all 1,920 ORE ontologies in
/// four arms (neither / `EMIT_ORDER` / `ONEOF_SEED` / both), recording
/// `concept_rules`, told-subsumer edges, told-disjoint pairs AND conversion
/// wall, and counting timeouts per arm so a flag-induced one could not be
/// dropped as an unparseable row.
///
/// **Result: this flag moves NOTHING on the ORE corpus** — zero ontologies
/// change any of the three counts, zero gain a conversion timeout, and the
/// `ore_ont_5368` discriminator is unmoved at 18,620,251. The numeric
/// `DataOneOf` pattern simply does not occur there. So the corpus establishes
/// only that the flip is free; the evidence that it is *right* is its canaries
/// plus a Konclude ∪ `HermiT` adjudication of the fixture where it does fire
/// (rustdl ON reproduces the oracle union exactly; OFF misses three
/// subsumptions).
///
/// See `docs/2026-08-03-dkey-volume-scan.md`.
fn dkey_oneof_seed_enabled() -> bool {
    std::env::var_os("RUSTDL_DKEY_ONEOF_SEED").is_none_or(|v| v != "0")
}

/// Bounded DKey-disjointness seeding (2026-07-20). **Default ON** — set
/// `RUSTDL_BOUNDED_DKEY_DISJOINT=0` to restore the unconditional all-pairs
/// seeding (the pre-fix O(k²) behaviour). Read per call so tests can toggle.
fn bounded_dkey_disjoint_enabled() -> bool {
    std::env::var("RUSTDL_BOUNDED_DKEY_DISJOINT").map_or(true, |v| v != "0")
}

/// Non-merging-component gate (2026-07-30). **Default ON** — set
/// `RUSTDL_DKEY_MERGING_GATE=0` to seed disjointness for every role component,
/// including those that contain no merge-inducing role (the pre-2026-07-30
/// behaviour). Read per call so tests can toggle it.
///
/// A component with no merge-inducing role can never force two `DKey`s into one
/// node label, so its pairwise disjointness is unusable — see
/// `docs/superpowers/specs/2026-07-30-dkey-nonmerging-component-gate-design.md`.
/// MEASUREMENT gate for the collapse/broadcast split study. Report-only: when set,
/// conversion counts how many `DKey` disjointness pairs the split WOULD drop, without
/// changing which axioms are emitted.
fn dkey_split_stats_enabled() -> bool {
    std::env::var("RUSTDL_DKEY_SPLIT_STATS").is_ok_and(|v| v != "0")
}

/// Collapse/broadcast split (2026-07-30). **Default ON** since the gates in
/// `docs/superpowers/specs/2026-07-30-dkey-collapse-vs-broadcast-design.md` passed; set
/// `RUSTDL_DKEY_COLLAPSE_SPLIT=0` to revert. Recovered `ore_ont_7607` and `ore_ont_1685`
/// from DNF; answers byte-identical wherever both settings complete.
///
/// Omits a `DKey`-disjointness pair when its component has no COLLAPSE role and BOTH
/// keys are value-only there: a BROADCAST source puts one key on EVERY successor, so a
/// value meets the broadcast key but never another value. Subtractive only — it can
/// never create a false positive; the exposure is a lost clash.
fn dkey_collapse_split_enabled() -> bool {
    std::env::var("RUSTDL_DKEY_COLLAPSE_SPLIT").map_or(true, |v| v != "0")
}

fn dkey_merging_gate_enabled() -> bool {
    std::env::var("RUSTDL_DKEY_MERGING_GATE").map_or(true, |v| v != "0")
}

/// Emit-ordering fix for [`seed_disjoint_bucket`] (2026-08-01). **Default ON
/// since 2026-08-03** (`=0` reverts to the defect).
///
/// # The defect
///
/// A `DKey` pair can belong to SEVERAL role components, and the collapse/broadcast
/// split ([`dkey_collapse_split_enabled`]) is a PER-COMPONENT judgement: the same
/// pair can be unusable in one component (both keys value-only, no collapse role)
/// and genuinely consumable in another (one key broadcast by a `∀`). `try_emit`
/// nevertheless claimed the pair in its `emitted` dedup set BEFORE consulting the
/// split, so the first component the `BTreeMap` reached spent the pair for good; if
/// that component declined it, the component that could have used it was never asked
/// and the `DisjointClasses` axiom was silently never emitted.
///
/// The symptom is NON-MONOTONIC: on `∀p.[0,5] ⊓ ∃p.{9}` rustdl derives `⊥`, but
/// adding an UNRELATED second data property `q` that merely mentions the same two
/// keys in value position makes the class satisfiable again. It is also what made
/// `RUSTDL_DKEY_MERGING_GATE=0` report FEWER entailments than the default — a purely
/// restrictive gate cannot lose entailments by being switched OFF, and it was only
/// "winning" because gating `q`'s component out kept it from eating the pair.
///
/// # Direction of risk
///
/// This lever makes conversion emit MORE `DisjointClasses(DKey, DKey)`, so unlike the
/// surrounding gates its failure mode would be a FALSE POSITIVE. Three properties bound
/// it: every emitted pair still passes the per-pair `disjoint()` value-space test (only
/// provably disjoint ranges); [`seed_disjoint_bucket`] is called once per DATATYPE
/// bucket with only that bucket's keys, so no cross-datatype pair is constructible; and
/// the emitted set is a SUBSET of what `RUSTDL_DKEY_COLLAPSE_SPLIT=0` already emits, so
/// it introduces no axiom the pre-split code did not.
///
/// # Why it shipped OFF, and what changed (2026-08-03)
///
/// It was held back for a volume measurement — it emits MORE axioms, the shape
/// that caused the v0.3.29 conversion DNFs. The scan (`rustdl tbox-stats` over
/// all 1,920 ORE ontologies, four arms, recording `concept_rules`, told-subsumer
/// edges, told-disjoint pairs and conversion wall, with timeouts counted per
/// arm) found **exactly one ontology in 1,920 whose numbers move at all**:
/// `ore_ont_9303`, `concept_rules` 8886 → 8887 and told-disjoint pairs
/// 6669 → 6670. One extra axiom. No ontology grew past the >2× / >100k
/// threshold, none gained a conversion timeout, none slowed >2×, and the
/// `ore_ont_5368` discriminator is unmoved at 18,620,251.
///
/// Because the risk direction here is a FALSE POSITIVE, the mover was
/// adjudicated rather than merely counted: `ore_ont_9303`'s classify output is
/// **byte-identical** ON vs OFF (the pair is emitted but never consumed), and
/// its verdict — inconsistent, all 726 classes unsatisfiable — is confirmed by
/// **both** Konclude (727 of 728 classes ≡ `owl:Nothing`) and `HermiT`
/// (`InconsistentOntologyException`). FP=0.
///
/// Caveat worth carrying: the curated corpus is inert for the `DKey` area by
/// `datatype_value_membership.rs`'s own admission, so the FP=0 net shows
/// non-regression only. The positive evidence is the canaries plus the
/// Konclude ∪ `HermiT` adjudication above.
///
/// See `docs/2026-08-03-dkey-volume-scan.md`.
fn dkey_emit_order_enabled() -> bool {
    std::env::var_os("RUSTDL_DKEY_EMIT_ORDER").is_none_or(|v| v != "0")
}

/// See a role restriction that only exists **after NNF** when classifying roles for
/// the bounded `DKey`-disjointness seeding. **Default ON since 0.4.8**;
/// `RUSTDL_DKEY_POST_NNF=0` reverts. (This header said "Default OFF" until
/// 2026-08-03 — stale since the 0.4.8 flip; the predicate below has been the
/// default-ON idiom throughout, so the doc was wrong, not the code.)
///
/// # The D10 bug this closes
///
/// [`seed_dkey_subsumptions`] — hence [`dkey_components`] — runs inside
/// `convert_ontology`, i.e. **before NNF**, and its three role classifications
/// (`merge_inducing`, `collapse`, `broadcast_in`) match only syntactic `All` / `Max`
/// pool entries. A universal restriction that comes into existence *only* through
/// NNF is therefore invisible. The reachable OWL 2 DL shape is the double negation
///
/// ```text
/// ObjectComplementOf(DataSomeValuesFrom(q, DataComplementOf(r)))   -- NNF -->   ∀q.DKey(r)
/// ```
///
/// Pre-NNF the pool holds `Not(Some(q, Not(DKey)))`, so `q` is marked neither
/// merge-inducing nor collapse/broadcast, its component is gated out, and the
/// disjointness pair `DKey(9) ⟂ DKey([0,5])` is never emitted — after which the
/// post-NNF `∀q.DKey([0,5])` has nothing to clash against. rustdl reports the class
/// satisfiable under **every** engine flag while the banner certifies
/// `Horn (hyper Horn fixpoint is complete)`; Konclude and `HermiT` both report it
/// `≡ owl:Nothing`. The directly-written control `∀p.[0,5] ⊓ ∃p.{9}` is caught at
/// every setting, so this is the gate's role classification failing, not the calculus.
///
/// This is a completeness **REGRESSION** introduced by the two `DKey` gates:
/// `RUSTDL_BOUNDED_DKEY_DISJOINT=0` (pre-v0.3.29 behaviour) still catches it, and
/// **either gate alone is enough to lose it** — so it is not attributable to one of
/// the 2026-07-20 / 2026-07-30 changes.
///
/// # Why NOT simply run the pass post-NNF
///
/// The sibling `RUSTDL_NEG_TO_BOT_GCI` pass runs pre-NNF *deliberately*, because it
/// needs to see `¬Y` before NNF turns `¬∃R.C` into `∀R.¬C`. This pass wants the
/// opposite, and the two must coexist in one `convert_ontology`. Moving
/// `seed_dkey_subsumptions` after NNF would also mean its emitted `DisjointClasses`
/// axioms bypass normalization, and NNF lives in a later pass in a different module.
/// So instead the SCAN is made NNF-aware: `Not(…)` pool entries are walked at
/// negative polarity and each `∃`/`≥n` found there is recorded as the `∀`/`≤n` it
/// will become. The dual direction needs nothing — a negative-polarity `All`/`Max`
/// weakens to `Some`/`Min`, and the existing positive scan already treats every
/// `All`/`Max` pool entry as merge-inducing regardless of polarity.
///
/// **Purely additive, so it can never lose a pair that is emitted today**: it only
/// ever sets more `merge_inducing` / `collapse` / `broadcast_in` bits, which only
/// ever admits more components and drops fewer pairs. FP-safety is untouched — the
/// per-pair `disjoint()` value-space check is the entire FP surface and is unchanged.
fn dkey_post_nnf_enabled() -> bool {
    // DEFAULT ON since 0.4.8 (`=0` reverts). Closes a completeness REGRESSION from the
    // 07-20/07-30 DKey gates: a `∀p.DKey` that only exists post-NNF was invisible here.
    std::env::var_os("RUSTDL_DKEY_POST_NNF").is_none_or(|v| v != "0")
}

/// A role restriction that will exist after NNF but does not exist in the pool yet.
#[derive(Clone, Copy)]
enum NnfDual {
    /// A negative-polarity `∃r.f`, which NNF turns into `∀r.¬f` — a BROADCAST
    /// position, merge-inducing, and COLLAPSE unless the filler is pure-`DKey`.
    Forall,
    /// A negative-polarity `≥n r.f`, which NNF turns into `≤(n-1) r.¬f` — a VALUE
    /// position, merge-inducing, and unconditionally COLLAPSE (like any `Max`).
    Max,
}

/// Collect the [`NnfDual`] occurrences: every `∃`/`≥n` reachable at NEGATIVE
/// polarity from a `Not(…)` pool entry.
///
/// Enumerating `Not` entries via `iter_exprs` (rather than walking axiom roots)
/// means no axiom kind can be forgotten — interning guarantees every `Not` node in
/// the ontology is its own pool entry. A nested `Not` flips polarity back to
/// positive, and that region is already covered by the caller's plain positive
/// scans, so the walk stops there.
///
/// `filler` is returned UN-negated. Both consumers see through `Not`
/// ([`filler_is_pure_dkey`] and [`collect_direct_dkeys`] both recurse into it), so
/// `¬f` and `f` yield the same `DKey` set and the same purity verdict.
fn collect_nnf_duals(pool: &ConceptPool) -> Vec<(NnfDual, Role, ConceptId)> {
    fn walk(
        pool: &ConceptPool,
        cid: ConceptId,
        seen: &mut std::collections::HashSet<ConceptId>,
        out: &mut Vec<(NnfDual, Role, ConceptId)>,
    ) {
        if !seen.insert(cid) {
            return;
        }
        match pool.get(cid) {
            ConceptExpr::Some(r, f) => {
                out.push((NnfDual::Forall, *r, *f));
                walk(pool, *f, seen, out);
            }
            ConceptExpr::Min(_, r, f) => {
                out.push((NnfDual::Max, *r, *f));
                walk(pool, *f, seen, out);
            }
            // Dual is `∃`/`≥n`, strictly weaker than what the positive scan
            // already asserts for these entries. Only recurse.
            ConceptExpr::All(_, f) | ConceptExpr::Max(_, _, f) => walk(pool, *f, seen, out),
            // De Morgan: polarity carries through both, and `⊓`/`⊔` swap.
            ConceptExpr::And(items) | ConceptExpr::Or(items) => {
                for &i in items {
                    walk(pool, i, seen, out);
                }
            }
            // Back to positive polarity — the plain scans cover it.
            _ => {}
        }
    }
    let mut out = Vec::new();
    if !dkey_post_nnf_enabled() {
        return out;
    }
    let mut seen = std::collections::HashSet::new();
    for expr in pool.iter_exprs() {
        if let ConceptExpr::Not(inner) = expr {
            walk(pool, *inner, &mut seen, &mut out);
        }
    }
    out
}

/// Merge-aware role-component map for bounded DKey-disjointness seeding.
///
/// `components[dkey_class]` = the set of role-component roots the `DKey` is
/// reachable under (a `DKey` may appear under several roles). `unanchored` =
/// `DKeys` that occur in a class position OUTSIDE any role restriction /
/// `ObjectPropertyRange` (cannot arise from the data lowering — every lowered
/// `DKey` is a restriction filler or a range — but handled conservatively:
/// an unanchored `DKey` pairs with every key in its bucket).
struct DkeyComponents {
    components: std::collections::HashMap<ClassId, Vec<usize>>,
    unanchored: std::collections::HashSet<ClassId>,
    /// MEASUREMENT: component ids containing a COLLAPSE role (spec R4-closed).
    collapse_comps: std::collections::HashSet<usize>,
    /// MEASUREMENT: per key, the component ids where it occurs in a BROADCAST
    /// position (a range or `∀` filler). A key may be both value and broadcast
    /// in one component; if broadcast here, it is not "value-only" here (spec R2).
    broadcast_in: std::collections::HashMap<ClassId, Vec<usize>>,
}

/// MEASUREMENT (2026-07-30, report-only): is `cid` provably incapable of collapsing
/// two successors onto one node? True only when the filler consists EXCLUSIVELY of
/// `DKey` atomics under `And`/`Or`/`Not`. Anything else may be, or be subsumed by, a
/// nominal and therefore collapse via the o-rule (spec R5) — so it must be treated as
/// a COLLAPSE source. Note the polarity: this asks ALL, whereas the deleted
/// `filler_mentions_dkey` asked ANY.
fn filler_is_pure_dkey(
    pool: &ConceptPool,
    cid: ConceptId,
    dkeys: &std::collections::HashSet<ClassId>,
) -> bool {
    match pool.get(cid) {
        ConceptExpr::Atomic(c) => dkeys.contains(c),
        ConceptExpr::Not(inner) => filler_is_pure_dkey(pool, *inner, dkeys),
        ConceptExpr::And(items) | ConceptExpr::Or(items) => {
            !items.is_empty() && items.iter().all(|&i| filler_is_pure_dkey(pool, i, dkeys))
        }
        _ => false,
    }
}

/// MEASUREMENT counters for the collapse/broadcast split (`RUSTDL_DKEY_SPLIT_STATS=1`).
/// Report-only: nothing here changes which axioms are emitted.
pub static DKEY_SPLIT_TOTAL: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
pub static DKEY_SPLIT_WOULD_DROP: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

// REMOVED 2026-07-30: `filler_mentions_dkey`. It gated whether an
// `ObjectPropertyRange` / `∀` marked its role merge-inducing, on the theory that
// only a `DKey`-bearing filler can put a key into every successor label. Adversarial
// review found the counterexample: a filler that forces successors to be the SAME
// individual (`ObjectOneOf(o)`, or any class subsumed by one) collapses them via the
// o-rule, so two distinct VALUE keys share a label without the filler mentioning a
// `DKey` at all. That is not syntactically detectable, so the test was replaced by
// treating ANY range / `∀` as merge-inducing. Regression:
// `crates/owl-dl-reasoner/tests/dkey_nominal_range_merge.rs`.

/// Invoke `f` on every `DKey` atomic reachable from `cid` through
/// `Not`/`And`/`Or` only (stopping at nested role restrictions — those are
/// anchored by their own pool entry).
fn collect_direct_dkeys(
    pool: &ConceptPool,
    cid: ConceptId,
    dkeys: &std::collections::HashSet<ClassId>,
    f: &mut impl FnMut(ClassId),
) {
    match pool.get(cid) {
        ConceptExpr::Atomic(c) if dkeys.contains(c) => {
            f(*c);
        }
        ConceptExpr::Not(inner) => collect_direct_dkeys(pool, *inner, dkeys, f),
        ConceptExpr::And(items) | ConceptExpr::Or(items) => {
            for &i in items {
                collect_direct_dkeys(pool, i, dkeys, f);
            }
        }
        _ => {}
    }
}

/// Build the merge-aware role components + `DKey` anchoring map.
///
/// Soundness/completeness fact (the spec's §2.1): a
/// `DisjointClasses(DKey_a, DKey_b)` axiom is only ever CONSUMED when both
/// `DKeys` land in one node's label, which requires their carrier roles to be
/// (transitive) sub-roles of a common **merge-inducing** role `f`:
/// functional / inverse-functional / occurring in a `≤n` restriction /
/// carrying a `∀f.DKey` filler or a `DKey` `ObjectPropertyRange` (a range acts
/// as a global `∀`). Two `DKeys` under roles NOT so connected can never share
/// a label, so their disjointness is dead weight. The component bound is
/// deliberately COARSE in the safe direction (over-union / over-anchor only
/// reduces the perf win; it can never drop a consumable pair):
/// - `M*` closes merge-inducing-ness DOWNWARD through the role hierarchy
///   (a sub-role of a functional role relays the merge to its fillers);
/// - `EquivalentObjectProperties` / `InverseObjectProperties` are treated as
///   mutual sub-edges;
/// - `DKeys` in non-restriction positions (can't arise from the lowering) are
///   returned `unanchored` and pair with everything.
///
/// Load-bearing (dead-end #3): the union is gated on `M*` supers ONLY —
/// unioning on every `SubObjectPropertyOf` collapses all data properties
/// under an `owl:topDataProperty`-style root into one O(k²) component.
fn dkey_components(out: &InternalOntology) -> DkeyComponents {
    use std::collections::{HashMap, HashSet};

    use crate::locality::UnionFind;

    let dkeys: HashSet<ClassId> = out
        .vocabulary
        .classes()
        .filter(|(_, iri)| is_dkey_iri(iri))
        .map(|(cid, _)| cid)
        .collect();
    let num_roles = out.vocabulary.num_roles();
    if dkeys.is_empty() || num_roles == 0 {
        return DkeyComponents {
            components: HashMap::new(),
            unanchored: HashSet::new(),
            collapse_comps: HashSet::new(),
            broadcast_in: HashMap::new(),
        };
    }

    // (a) merge-inducing roles M. `role_id()` ignores inverse polarity —
    // a `≤n r⁻` merge is anchored on the same named role (over-approx, safe).
    let mut merge_inducing = vec![false; num_roles];
    for axiom in &out.axioms {
        match axiom {
            Axiom::FunctionalRole(r) | Axiom::InverseFunctionalRole(r) => {
                merge_inducing[r.role_id().index() as usize] = true;
            }
            // ANY range makes the role merge-inducing — deliberately NOT gated on
            // `filler_mentions_dkey`. Two reasons, the second discovered by
            // adversarial review on 2026-07-30:
            //  1. a `DKey` range puts the range key into EVERY successor label of
            //     the role — the same consumption shape as `∀role.DKey`;
            //  2. a range whose filler forces successors to be the SAME individual
            //     collapses them via the o-rule, so two distinct value keys share
            //     one label. `ObjectPropertyRange(p, ObjectOneOf(o))` does this, and
            //     so does `ObjectPropertyRange(p, C)` with `C ⊑ ObjectOneOf(o)` —
            //     which is NOT syntactically local, so no filler test can catch it.
            //     Gating on `filler_mentions_dkey` here made
            //     `tests/dkey_nominal_range_merge.rs`'s fixture a MISS.
            Axiom::ObjectPropertyRange { role, .. } => {
                merge_inducing[role.role_id().index() as usize] = true;
            }
            _ => {}
        }
    }
    for expr in out.concepts.iter_exprs() {
        match expr {
            // `Max` collapses two successors onto one node. `All` is merge-inducing
            // for the same two reasons as `ObjectPropertyRange` above, and is NOT
            // gated on the filler mentioning a `DKey`: a filler that forces
            // successors to coincide (a nominal, or any class subsumed by one)
            // collapses them via the o-rule, and that is not syntactically
            // detectable. Same body, so one arm.
            ConceptExpr::Max(_, r, _) | ConceptExpr::All(r, _) => {
                merge_inducing[r.role_id().index() as usize] = true;
            }
            _ => {}
        }
    }
    // `RUSTDL_DKEY_POST_NNF` (default ON): the `∀`/`≤n` restrictions that only
    // come into existence at NNF. Both duals are merge-inducing for exactly the
    // reasons the syntactic arm above lists. See [`dkey_post_nnf_enabled`].
    let nnf_duals = collect_nnf_duals(&out.concepts);
    for &(_, r, _) in &nnf_duals {
        merge_inducing[r.role_id().index() as usize] = true;
    }

    // (b) role-hierarchy edges (sub ⊑ sup), incl. chain parts and
    // equivalence / declared-inverse pairs (both directions).
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for axiom in &out.axioms {
        match axiom {
            Axiom::SubObjectPropertyOf { sub, sup } => {
                let sup_id = sup.role_id().index() as usize;
                match sub {
                    SubRolePath::Role(r) => edges.push((r.role_id().index() as usize, sup_id)),
                    SubRolePath::Chain(parts) => {
                        for p in parts {
                            edges.push((p.role_id().index() as usize, sup_id));
                        }
                    }
                }
            }
            Axiom::EquivalentObjectProperties(roles) => {
                for w in roles.windows(2) {
                    let a = w[0].role_id().index() as usize;
                    let b = w[1].role_id().index() as usize;
                    edges.push((a, b));
                    edges.push((b, a));
                }
            }
            Axiom::InverseObjectProperties(p, q) => {
                let a = p.role_id().index() as usize;
                let b = q.role_id().index() as usize;
                edges.push((a, b));
                edges.push((b, a));
            }
            _ => {}
        }
    }

    // (c) M* = downward closure of M along sub-edges: a role with a
    // (transitive) merge-inducing super relays the merge/∀ to its fillers.
    let mut subs_of: Vec<Vec<usize>> = vec![Vec::new(); num_roles];
    for &(sub, sup) in &edges {
        subs_of[sup].push(sub);
    }
    let mut m_star = merge_inducing;
    let mut queue: Vec<usize> = (0..num_roles).filter(|&r| m_star[r]).collect();
    while let Some(sup) = queue.pop() {
        for &sub in &subs_of[sup] {
            if !m_star[sub] {
                m_star[sub] = true;
                queue.push(sub);
            }
        }
    }

    // MEASUREMENT (report-only): the COLLAPSE subset — sources that force two
    // DISTINCT successors onto ONE node, as opposed to BROADCAST sources which put
    // one key on EVERY successor. Per spec R5 a range/`∀` counts as COLLAPSE unless
    // its filler is provably pure-`DKey`, because a nominal-forcing filler collapses
    // via the o-rule and that is not syntactically detectable. Closed DOWNWARD
    // through the role hierarchy exactly as `m_star` is (spec R4).
    let mut collapse = vec![false; num_roles];
    for axiom in &out.axioms {
        match axiom {
            Axiom::FunctionalRole(r) | Axiom::InverseFunctionalRole(r) => {
                collapse[r.role_id().index() as usize] = true;
            }
            Axiom::ObjectPropertyRange { role, range }
                if !filler_is_pure_dkey(&out.concepts, *range, &dkeys) =>
            {
                collapse[role.role_id().index() as usize] = true;
            }
            _ => {}
        }
    }
    for expr in out.concepts.iter_exprs() {
        match expr {
            ConceptExpr::Max(_, r, _) => {
                collapse[r.role_id().index() as usize] = true;
            }
            ConceptExpr::All(r, f) if !filler_is_pure_dkey(&out.concepts, *f, &dkeys) => {
                collapse[r.role_id().index() as usize] = true;
            }
            _ => {}
        }
    }
    // NNF duals, matching the arms just above: a `∀` counts as COLLAPSE unless its
    // filler is provably pure-`DKey`; a `≤n` always does.
    for &(kind, r, f) in &nnf_duals {
        let is_collapse = match kind {
            NnfDual::Forall => !filler_is_pure_dkey(&out.concepts, f, &dkeys),
            NnfDual::Max => true,
        };
        if is_collapse {
            collapse[r.role_id().index() as usize] = true;
        }
    }
    let mut cqueue: Vec<usize> = (0..num_roles).filter(|&r| collapse[r]).collect();
    while let Some(sup) = cqueue.pop() {
        for &sub in &subs_of[sup] {
            if !collapse[sub] {
                collapse[sub] = true;
                cqueue.push(sub);
            }
        }
    }

    // (d) union roles connected via an M*-super ONLY (dead-end #3).
    let mut uf = UnionFind::new(num_roles);
    for &(sub, sup) in &edges {
        if m_star[sup] {
            uf.union(sub, sup);
        }
    }

    // Components containing at least one merge-inducing role. A component with
    // none can never force two `DKey`s into ONE node label (`∃p.A ⊓ ∃p.B` has two
    // distinct successors), so seeding its pairs is dead weight. `None` ⟹ gate
    // off ⟹ every component is treated as merging (pre-2026-07-30 behaviour).
    let merging_comps: Option<HashSet<usize>> = if dkey_merging_gate_enabled() {
        let mut s = HashSet::new();
        for (r, &is_merging) in m_star.iter().enumerate().take(num_roles) {
            if is_merging {
                s.insert(uf.find(r));
            }
        }
        Some(s)
    } else {
        None
    };

    // (e) DKey → component set, from every role-restriction pool expr plus
    // DKey-bearing `ObjectPropertyRange` axioms (range key rides the role).
    // MEASUREMENT: components holding a COLLAPSE role, computed AFTER the union so
    // it reflects merged components.
    let collapse_comps: HashSet<usize> = {
        let mut set = HashSet::new();
        for (r, &is_collapse) in collapse.iter().enumerate() {
            if is_collapse {
                set.insert(uf.find(r));
            }
        }
        set
    };
    let mut broadcast_in: HashMap<ClassId, Vec<usize>> = HashMap::new();
    let mut components: HashMap<ClassId, Vec<usize>> = HashMap::new();
    let anchor = |uf: &mut UnionFind,
                  components: &mut HashMap<ClassId, Vec<usize>>,
                  broadcast_in: &mut HashMap<ClassId, Vec<usize>>,
                  is_broadcast: bool,
                  role: Role,
                  filler: ConceptId| {
        let comp = uf.find(role.role_id().index() as usize);
        if merging_comps.as_ref().is_some_and(|m| !m.contains(&comp)) {
            // Gate ON and this component has no merge-inducing role: the keys
            // under it can never be co-labelled, so leave them unanchored-and-
            // uncomponented. `seed_disjoint_bucket` already skips such keys
            // ("can never reach a node label"); this extends that skip to
            // "can never be CO-labelled".
            return;
        }
        collect_direct_dkeys(&out.concepts, filler, &dkeys, &mut |c| {
            let v = components.entry(c).or_default();
            if !v.contains(&comp) {
                v.push(comp);
            }
            if is_broadcast {
                let b = broadcast_in.entry(c).or_default();
                if !b.contains(&comp) {
                    b.push(comp);
                }
            }
        });
    };
    for expr in out.concepts.iter_exprs() {
        match expr {
            // `Some`/`Min`/`Max` fillers are VALUE positions: the key lands on the
            // generated successor, not on every successor.
            ConceptExpr::Some(r, f) | ConceptExpr::Min(_, r, f) | ConceptExpr::Max(_, r, f) => {
                anchor(&mut uf, &mut components, &mut broadcast_in, false, *r, *f);
            }
            // `∀` is a BROADCAST position: its filler's keys land on EVERY successor.
            ConceptExpr::All(r, f) => {
                anchor(&mut uf, &mut components, &mut broadcast_in, true, *r, *f);
            }
            _ => {}
        }
    }
    // NNF duals: `∀` is a BROADCAST position, `≤n` a VALUE one — same split as the
    // syntactic arms above.
    for &(kind, r, f) in &nnf_duals {
        let is_broadcast = matches!(kind, NnfDual::Forall);
        anchor(
            &mut uf,
            &mut components,
            &mut broadcast_in,
            is_broadcast,
            r,
            f,
        );
    }
    for axiom in &out.axioms {
        // A range is a BROADCAST position, same shape as `∀`.
        if let Axiom::ObjectPropertyRange { role, range } = axiom {
            anchor(
                &mut uf,
                &mut components,
                &mut broadcast_in,
                true,
                *role,
                *range,
            );
        }
    }

    // (f) unanchored scan: DKeys reachable (through Not/And/Or only) from a
    // top-level class position of any axiom — a label placement NOT mediated
    // by a role. Cannot arise from the data lowering; conservative safety net.
    let mut unanchored: HashSet<ClassId> = HashSet::new();
    let mut tops: Vec<ConceptId> = Vec::new();
    for axiom in &out.axioms {
        match axiom {
            Axiom::SubClassOf { sub, sup } => {
                tops.push(*sub);
                tops.push(*sup);
            }
            Axiom::EquivalentClasses(cs) | Axiom::DisjointClasses(cs) => {
                tops.extend(cs.iter().copied());
            }
            Axiom::DisjointUnion { class, members } => {
                if dkeys.contains(class) {
                    unanchored.insert(*class);
                }
                tops.extend(members.iter().copied());
            }
            Axiom::ObjectPropertyDomain { domain, .. } => tops.push(*domain),
            Axiom::ClassAssertion { class, .. } => tops.push(*class),
            _ => {}
        }
    }
    for cid in tops {
        collect_direct_dkeys(&out.concepts, cid, &dkeys, &mut |c| {
            unanchored.insert(c);
        });
    }

    DkeyComponents {
        components,
        unanchored,
        collapse_comps,
        broadcast_in,
    }
}

/// Emit `DisjointClasses([DKey(r_i), DKey(r_j)])` for UNORDERED pairs of
/// distinct keys in one bucket where `disjoint(r_i, r_j)` holds. Uses the
/// native `DisjointClasses` axiom (the shape the D10 ∀-clash probe proved
/// the tableau handles), not a synthetic `And ⊑ ⊥`.
///
/// With `components == None` (`RUSTDL_BOUNDED_DKEY_DISJOINT=0`): every
/// provably-disjoint pair, O(k²). With `Some`: only pairs within one
/// merge-aware role component (see [`dkey_components`]) — the pairs are
/// GROUPED by component first (dead-end #2: a per-pair filter over the
/// O(k²) walk still stalls), so cost is `O(Σ_component` values²). The
/// per-pair `disjoint` range check is kept in both paths (FP-safety: the
/// bounded set is a subset of the sound all-pairs set by construction).
fn seed_disjoint_bucket<R>(
    out: &mut InternalOntology,
    keys: &[(ClassId, R)],
    disjoint: impl Fn(&R, &R) -> bool,
    components: Option<&DkeyComponents>,
) {
    let Some(comp) = components else {
        for (i, (a_cid, a_r)) in keys.iter().enumerate() {
            for (b_cid, b_r) in keys.iter().skip(i + 1) {
                if disjoint(a_r, b_r) {
                    let a = out.concepts.atomic(*a_cid);
                    let b = out.concepts.atomic(*b_cid);
                    out.axioms.push(Axiom::DisjointClasses(vec![a, b]));
                }
            }
        }
        return;
    };
    // Group bucket entries by role component. BTreeMap ⟹ deterministic
    // iteration ⟹ deterministic axiom order across runs (byte-identical
    // conversion output matters to the identity gates).
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    // Unanchored keys pair with every anchored/unanchored key in the bucket.
    let mut global: Vec<usize> = Vec::new();
    let mut anchored: Vec<usize> = Vec::new();
    for (idx, (cid, _)) in keys.iter().enumerate() {
        if comp.unanchored.contains(cid) {
            global.push(idx);
            continue;
        }
        if let Some(cs) = comp.components.get(cid) {
            anchored.push(idx);
            for &c in cs {
                groups.entry(c).or_default().push(idx);
            }
        }
        // Neither anchored nor unanchored: the DKey appears under no role
        // restriction at all — it can never reach a node label, so its
        // disjointness is dead weight; skip it entirely.
    }
    // A key can sit in several groups (several components) and `global`
    // overlaps every group — dedup emitted pairs.
    let mut emitted: std::collections::HashSet<(ClassId, ClassId)> =
        std::collections::HashSet::new();
    // ORDERING LEVER (2026-08-01, `RUSTDL_DKEY_EMIT_ORDER`, default ON since 2026-08-03).
    // Pairs the collapse/broadcast split DECLINED in some component. With the
    // lever on, declining no longer SPENDS the pair, so a pair recorded here may
    // still be emitted from a later component; only `deferred \ emitted` was
    // genuinely dropped. STATS-ONLY (populated when `RUSTDL_DKEY_SPLIT_STATS=1`).
    let mut deferred: std::collections::HashSet<(ClassId, ClassId)> =
        std::collections::HashSet::new();
    let emit_order = dkey_emit_order_enabled();
    let InternalOntology {
        concepts, axioms, ..
    } = out;
    // MEASUREMENT (report-only, `RUSTDL_DKEY_SPLIT_STATS=1`): `in_comp` is `Some(c)`
    // for a same-component pair and `None` for the unanchored (`global`) pairings,
    // which are unconditional (spec R6) and therefore never scored as droppable.
    let stats = dkey_split_stats_enabled();
    let split = dkey_collapse_split_enabled();
    let mut try_emit = |a_idx: usize, b_idx: usize, in_comp: Option<usize>| {
        let (a_cid, a_r) = &keys[a_idx];
        let (b_cid, b_r) = &keys[b_idx];
        if !disjoint(a_r, b_r) {
            return;
        }
        let pair = if a_cid.index() <= b_cid.index() {
            (*a_cid, *b_cid)
        } else {
            (*b_cid, *a_cid)
        };
        // DEDUP, two spellings of the same job.
        //
        // OFF (historical): claim the pair HERE, before the droppable test. A pair
        // that spans two role components is therefore spent by whichever component
        // the `BTreeMap` reaches first — and if the split declines it THERE, the
        // component where it is consumable never gets a turn. That is a silent
        // completeness defect, not a dedup: it drops an entailed axiom.
        //
        // ON (`RUSTDL_DKEY_EMIT_ORDER=1`): only LOOK here; the pair is claimed at
        // the emit site below. Dedup is preserved exactly — the `contains` guard
        // rejects any pair already emitted, and the single-threaded walk visits
        // each (pair, component) at most once, so a pair is pushed at most once.
        if emit_order {
            if emitted.contains(&pair) {
                return;
            }
        } else if !emitted.insert(pair) {
            return;
        }
        // Would the collapse/broadcast split drop this pair? Only when the component
        // has NO collapse role AND BOTH keys are value-only there. `in_comp` is
        // `None` for the unanchored `global` pairings, which are unconditional
        // (spec R6) and therefore never droppable.
        let droppable = in_comp.is_some_and(|c| {
            let value_only =
                |cid: &ClassId| !comp.broadcast_in.get(cid).is_some_and(|v| v.contains(&c));
            !comp.collapse_comps.contains(&c) && value_only(a_cid) && value_only(b_cid)
        });
        // Lever ON: a per-(pair, component) tally would double-count a multi-component
        // pair, so the counters are settled once after the walk, from the sets.
        if stats && !emit_order {
            use std::sync::atomic::Ordering;
            DKEY_SPLIT_TOTAL.fetch_add(1, Ordering::Relaxed);
            if droppable {
                DKEY_SPLIT_WOULD_DROP.fetch_add(1, Ordering::Relaxed);
            }
        }
        if split && droppable {
            if stats && emit_order {
                deferred.insert(pair);
            }
            return;
        }
        if emit_order {
            emitted.insert(pair);
        }
        let a = concepts.atomic(*a_cid);
        let b = concepts.atomic(*b_cid);
        axioms.push(Axiom::DisjointClasses(vec![a, b]));
    };
    // COMPONENT-LEVEL SKIP (`RUSTDL_DKEY_GROUP_SKIP`, default OFF).
    //
    // `droppable` above is `!collapse_comps.contains(c) && value_only(a) && value_only(b)`.
    // The first conjunct is a property of the COMPONENT, and `value_only` is a property of
    // ONE key in that component — none of it is per-PAIR. So when a component has no
    // collapse role and EVERY key in it is value-only there, every one of its C(k,2) pairs
    // is droppable, and the whole group can be skipped in O(k) instead of enumerated in
    // O(k²).
    //
    // Measured: this is the entire cost of two DNF-tail members. `ore_ont_10929` enumerates
    // **248,465,112** pairs and drops **100%** of them; `ore_ont_15635` **294,744,041**, also
    // 100%. That is ~543 M pair-visits at ~0.39 µs each — `ore_ont_10929` spends 96 s of a
    // 97.6 s `tbox-stats` inside conversion, with **12 classes** to reason about.
    // See `docs/benchmarks/2026-08-20-dkey-residual-class-unpark-case.md`.
    //
    // EQUIVALENT to the per-pair path when `split` is on: a skipped pair would have hit the
    // `split && droppable` early-return anyway. It is gated on `split` for exactly that
    // reason — with the split off nothing is droppable and the skip must not fire. Under
    // `emit_order` (default ON) a pair spanning two components is still enumerated in the
    // OTHER component, so skipping one cannot drop an axiom that was consumable elsewhere.
    let group_skip = dkey_group_skip_enabled() && split;
    for (&c, group) in &groups {
        if group_skip && !comp.collapse_comps.contains(&c) {
            // PARTITION, rather than an all-or-nothing test. `droppable` needs BOTH keys of
            // a pair value-only, so the droppable block is exactly value-only × value-only.
            // Requiring the WHOLE group to be value-only was measured to never fire (95.5 s
            // → 94.2 s on `ore_ont_10929`): a single broadcast key that forms no disjoint
            // pair defeats it while the drop rate is still 100%. So skip the vo×vo block and
            // still enumerate anything touching a broadcast key.
            let (vo, rest): (Vec<usize>, Vec<usize>) = group.iter().partition(|&&idx| {
                let (cid, _) = &keys[idx];
                !comp.broadcast_in.get(cid).is_some_and(|v| v.contains(&c))
            });
            if !vo.is_empty() {
                for (i, &a_idx) in rest.iter().enumerate() {
                    for &b_idx in &rest[i + 1..] {
                        try_emit(a_idx, b_idx, Some(c));
                    }
                    for &b_idx in &vo {
                        try_emit(a_idx, b_idx, Some(c));
                    }
                }
                // vo × vo is entirely droppable — skipped, O(|rest|·k) instead of O(k²).
                continue;
            }
            if false {
                // Every pair here is droppable; do not pay O(k²) to rediscover that.
                //
                // STATS CONSEQUENCE, deliberate: `RUSTDL_DKEY_SPLIT_STATS` counters
                // UNDERCOUNT when this fires, because tallying the skipped pairs would
                // require the very enumeration the skip exists to avoid. So compare
                // `dkey_pairs_total` only between runs with the same setting of this flag.
                continue;
            }
        }
        for (i, &a_idx) in group.iter().enumerate() {
            for &b_idx in &group[i + 1..] {
                try_emit(a_idx, b_idx, Some(c));
            }
        }
    }
    for (i, &a_idx) in global.iter().enumerate() {
        for &b_idx in &global[i + 1..] {
            try_emit(a_idx, b_idx, None);
        }
        for &b_idx in &anchored {
            try_emit(a_idx, b_idx, None);
        }
    }
    // MEASUREMENT, lever ON: settle the counters per UNIQUE pair now that every
    // component has had its turn. A pair in `deferred` that also reached `emitted`
    // was declined somewhere and emitted elsewhere — NOT dropped, so it must not
    // score as `would_drop`. `total` = |emitted ∪ deferred|.
    if stats && emit_order {
        use std::sync::atomic::Ordering;
        let dropped = deferred.difference(&emitted).count();
        DKEY_SPLIT_TOTAL.fetch_add(emitted.len() + dropped, Ordering::Relaxed);
        DKEY_SPLIT_WOULD_DROP.fetch_add(dropped, Ordering::Relaxed);
    }
}

/// Emit `SubClassOf(DKey(r_i), DKey(r_j))` for every ordered pair of
/// distinct keys in one datatype bucket where `subset(r_i, r_j)` holds.
/// O(k²) in the bucket size (small — bounded by the count of distinct
/// facet/value combinations of that datatype in the ontology).
fn seed_bucket<R>(
    out: &mut InternalOntology,
    keys: &[(ClassId, R)],
    subset: impl Fn(&R, &R) -> bool,
) {
    for (i, (sub_cid, sub_r)) in keys.iter().enumerate() {
        for (j, (sup_cid, sup_r)) in keys.iter().enumerate() {
            if i == j {
                continue;
            }
            if subset(sub_r, sup_r) {
                let sub = out.concepts.atomic(*sub_cid);
                let sup = out.concepts.atomic(*sup_cid);
                out.axioms.push(Axiom::SubClassOf { sub, sup });
            }
        }
    }
}

impl<A: ForIRI> TryFrom<&SetOntology<A>> for InternalOntology {
    type Error = ConversionError;

    fn try_from(src: &SetOntology<A>) -> Result<Self, Self::Error> {
        convert_ontology(src)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::ir::ConceptExpr;
    use horned_owl::model::{Build, RcStr};

    fn b() -> Build<RcStr> {
        Build::new_rc()
    }

    fn ctx() -> (Vocabulary, ConceptPool) {
        (Vocabulary::new(), ConceptPool::new())
    }

    #[test]
    fn class() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::Class(b().class("http://example.org/A"));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Atomic(c) = p.get(id) else {
            panic!("expected Atomic")
        };
        assert_eq!(v.class_iri(*c), "http://example.org/A");
    }

    #[test]
    fn intersection() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectIntersectionOf(vec![
            ClassExpression::Class(b().class("A")),
            ClassExpression::Class(b().class("B")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::And(args) = p.get(id) else {
            panic!("expected And")
        };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn empty_intersection_is_top() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::ObjectIntersectionOf(vec![]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Top));
    }

    #[test]
    fn union() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectUnionOf(vec![
            ClassExpression::Class(b().class("A")),
            ClassExpression::Class(b().class("B")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Or(_)));
    }

    #[test]
    fn empty_union_is_bot() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::ObjectUnionOf(vec![]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Bot));
    }

    #[test]
    fn complement() {
        let (mut v, mut p) = ctx();
        let ce =
            ClassExpression::ObjectComplementOf(Box::new(ClassExpression::Class(b().class("A"))));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Not(_)));
    }

    #[test]
    fn some_values_from() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectSomeValuesFrom {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::Some(_, _)));
    }

    #[test]
    fn all_values_from() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectAllValuesFrom {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::All(_, _)));
    }

    #[test]
    fn has_value_encodes_as_some_of_nominal() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectHasValue {
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            i: Individual::Named(b().named_individual("a")),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Some(_, inner) = p.get(id) else {
            panic!("expected Some(_, _)")
        };
        assert!(matches!(p.get(*inner), ConceptExpr::Nominal(_)));
    }

    #[test]
    fn has_self() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectHasSelf(ObjectPropertyExpression::ObjectProperty(
            b().object_property("r"),
        ));
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        assert!(matches!(p.get(id), ConceptExpr::SelfRestriction(_)));
    }

    #[test]
    fn min_cardinality() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectMinCardinality {
            n: 3,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Min(n, _, _) = p.get(id) else {
            panic!("expected Min")
        };
        assert_eq!(*n, 3);
    }

    #[test]
    fn max_cardinality() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectMaxCardinality {
            n: 5,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Max(n, _, _) = p.get(id) else {
            panic!("expected Max")
        };
        assert_eq!(*n, 5);
    }

    #[test]
    fn exact_cardinality_encodes_as_and_of_min_max() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectExactCardinality {
            n: 2,
            ope: ObjectPropertyExpression::ObjectProperty(b().object_property("r")),
            bce: Box::new(ClassExpression::Class(b().class("A"))),
        };
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::And(args) = p.get(id) else {
            panic!("expected And(Min, Max)")
        };
        assert_eq!(args.len(), 2);
        // One of the conjuncts is Min, the other Max.
        let kinds: Vec<&'static str> = args
            .iter()
            .map(|a| match p.get(*a) {
                ConceptExpr::Min(..) => "Min",
                ConceptExpr::Max(..) => "Max",
                _ => "other",
            })
            .collect();
        assert!(kinds.contains(&"Min"));
        assert!(kinds.contains(&"Max"));
    }

    #[test]
    fn one_of_encodes_as_or_of_nominals() {
        let (mut v, mut p) = ctx();
        let ce = ClassExpression::ObjectOneOf(vec![
            Individual::Named(b().named_individual("a")),
            Individual::Named(b().named_individual("b")),
        ]);
        let id = convert_class_expression(&ce, &mut v, &mut p).unwrap();
        let ConceptExpr::Or(args) = p.get(id) else {
            panic!("expected Or")
        };
        assert_eq!(args.len(), 2);
        for a in args {
            assert!(matches!(p.get(*a), ConceptExpr::Nominal(_)));
        }
    }

    #[test]
    fn inverse_object_property_lowers_to_inverse_role() {
        let mut v = Vocabulary::new();
        let ope =
            ObjectPropertyExpression::<RcStr>::InverseObjectProperty(b().object_property("r"));
        let role = convert_object_property(&ope, &mut v).unwrap();
        assert!(role.is_inverse());
        // The named id should match the forward use's id.
        let forward = b().object_property("r");
        let forward_ope = ObjectPropertyExpression::<RcStr>::ObjectProperty(forward);
        let forward_role = convert_object_property(&forward_ope, &mut v).unwrap();
        assert_eq!(role.role_id(), forward_role.role_id());
    }

    #[test]
    fn anonymous_individual_is_interned_under_reserved_prefix() {
        use horned_owl::model::AnonymousIndividual;
        use std::rc::Rc;
        let mut vocab = Vocabulary::new();
        let a: Individual<RcStr> = Individual::Anonymous(AnonymousIndividual(Rc::from("blank-0")));
        let id_a = convert_individual(&a, &mut vocab).expect("anon individual interns");
        // same label → same id (blank-node identity within a document)
        let id_a2 = convert_individual(&a, &mut vocab).expect("anon individual interns");
        assert_eq!(
            id_a, id_a2,
            "same anon label must intern to the same IndividualId"
        );
        // distinct label → distinct id
        let b: Individual<RcStr> = Individual::Anonymous(AnonymousIndividual(Rc::from("blank-1")));
        let id_b = convert_individual(&b, &mut vocab).expect("anon individual interns");
        assert_ne!(
            id_a, id_b,
            "distinct anon labels must intern to distinct IndividualIds"
        );
        // interned under the reserved prefix
        assert!(vocab.individual_iri(id_a).starts_with(ANON_IRI_PREFIX));
    }

    #[test]
    fn data_some_values_rejected() {
        let (mut v, mut p) = ctx();
        let ce: ClassExpression<RcStr> = ClassExpression::DataSomeValuesFrom {
            dp: b().data_property("dp"),
            dr: horned_owl::model::DataRange::Datatype(b().datatype("dt")),
        };
        let err = convert_class_expression(&ce, &mut v, &mut p).unwrap_err();
        assert_eq!(err, ConversionError::UnsupportedDataRange);
    }

    #[test]
    fn shared_subexpressions_share_ids() {
        let (mut v, mut p) = ctx();
        let ce1 = ClassExpression::Class(b().class("A"));
        let ce2 = ClassExpression::Class(b().class("A"));
        let id1 = convert_class_expression(&ce1, &mut v, &mut p).unwrap();
        let id2 = convert_class_expression(&ce2, &mut v, &mut p).unwrap();
        assert_eq!(id1, id2);
        assert_eq!(p.len(), 1);
        assert_eq!(v.num_classes(), 1);
    }

    // ──────────────────────────────────────────────────────────────────
    // Day 11: per-Component axiom conversion tests
    // ──────────────────────────────────────────────────────────────────

    use horned_owl::model as ho;
    use horned_owl::model::MutableOntology;

    fn ce_class(name: &str) -> ClassExpression<RcStr> {
        ClassExpression::Class(b().class(name))
    }

    fn ope(name: &str) -> ObjectPropertyExpression<RcStr> {
        ObjectPropertyExpression::ObjectProperty(b().object_property(name))
    }

    fn named_ind(name: &str) -> Individual<RcStr> {
        Individual::Named(b().named_individual(name))
    }

    fn convert_one(c: &Component<RcStr>) -> (InternalOntology, Option<Axiom>) {
        let mut o = InternalOntology::new();
        let ax = convert_component(c, &mut o.vocabulary, &mut o.concepts).unwrap();
        (o, ax)
    }

    #[test]
    fn sub_class_of_axiom() {
        let c = Component::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ce_class("B"),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::SubClassOf { .. })));
    }

    #[test]
    fn equivalent_classes_axiom_keeps_vec() {
        let c = Component::EquivalentClasses(ho::EquivalentClasses(vec![
            ce_class("A"),
            ce_class("B"),
            ce_class("C"),
        ]));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::EquivalentClasses(v)) = ax else {
            panic!()
        };
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn disjoint_classes_axiom() {
        let c = Component::DisjointClasses(ho::DisjointClasses(vec![ce_class("A"), ce_class("B")]));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::DisjointClasses(_))));
    }

    #[test]
    fn disjoint_union_axiom() {
        let c = Component::DisjointUnion(ho::DisjointUnion(
            b().class("Parent"),
            vec![ce_class("Child1"), ce_class("Child2")],
        ));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::DisjointUnion { members, .. }) = ax else {
            panic!()
        };
        assert_eq!(members.len(), 2);
    }

    #[test]
    fn sub_object_property_of_single() {
        let c = Component::SubObjectPropertyOf(ho::SubObjectPropertyOf {
            sub: SubObjectPropertyExpression::ObjectPropertyExpression(ope("r")),
            sup: ope("s"),
        });
        let (_, ax) = convert_one(&c);
        let Some(Axiom::SubObjectPropertyOf { sub, .. }) = ax else {
            panic!()
        };
        assert!(matches!(sub, SubRolePath::Role(_)));
    }

    #[test]
    fn sub_object_property_of_chain() {
        let c = Component::SubObjectPropertyOf(ho::SubObjectPropertyOf {
            sub: SubObjectPropertyExpression::ObjectPropertyChain(vec![ope("r"), ope("s")]),
            sup: ope("t"),
        });
        let (_, ax) = convert_one(&c);
        let Some(Axiom::SubObjectPropertyOf { sub, .. }) = ax else {
            panic!()
        };
        let SubRolePath::Chain(chain) = sub else {
            panic!()
        };
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn equivalent_object_properties() {
        let c = Component::EquivalentObjectProperties(ho::EquivalentObjectProperties(vec![
            ope("r"),
            ope("s"),
        ]));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::EquivalentObjectProperties(_))));
    }

    #[test]
    fn inverse_object_properties_axiom() {
        let c = Component::InverseObjectProperties(ho::InverseObjectProperties(
            b().object_property("r"),
            b().object_property("s"),
        ));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::InverseObjectProperties(_, _))));
    }

    #[test]
    fn object_property_domain_and_range() {
        let domain_c = Component::ObjectPropertyDomain(ho::ObjectPropertyDomain {
            ope: ope("r"),
            ce: ce_class("A"),
        });
        let range_c = Component::ObjectPropertyRange(ho::ObjectPropertyRange {
            ope: ope("r"),
            ce: ce_class("B"),
        });
        assert!(matches!(
            convert_one(&domain_c).1,
            Some(Axiom::ObjectPropertyDomain { .. })
        ));
        assert!(matches!(
            convert_one(&range_c).1,
            Some(Axiom::ObjectPropertyRange { .. })
        ));
    }

    #[test]
    fn role_characteristics() {
        type AxiomCheck = (Component<RcStr>, fn(&Axiom) -> bool);
        let cases: Vec<AxiomCheck> = vec![
            (
                Component::TransitiveObjectProperty(ho::TransitiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::TransitiveRole(_)),
            ),
            (
                Component::FunctionalObjectProperty(ho::FunctionalObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::FunctionalRole(_)),
            ),
            (
                Component::InverseFunctionalObjectProperty(ho::InverseFunctionalObjectProperty(
                    ope("r"),
                )),
                |a| matches!(a, Axiom::InverseFunctionalRole(_)),
            ),
            (
                Component::ReflexiveObjectProperty(ho::ReflexiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::ReflexiveRole(_)),
            ),
            (
                Component::IrreflexiveObjectProperty(ho::IrreflexiveObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::IrreflexiveRole(_)),
            ),
            (
                Component::SymmetricObjectProperty(ho::SymmetricObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::SymmetricRole(_)),
            ),
            (
                Component::AsymmetricObjectProperty(ho::AsymmetricObjectProperty(ope("r"))),
                |a| matches!(a, Axiom::AsymmetricRole(_)),
            ),
        ];
        for (c, pred) in cases {
            let (_, ax) = convert_one(&c);
            let ax = ax.expect("expected an axiom");
            assert!(pred(&ax), "wrong axiom: {ax:?}");
        }
    }

    #[test]
    fn class_assertion() {
        let c = Component::ClassAssertion(ho::ClassAssertion {
            ce: ce_class("A"),
            i: named_ind("a"),
        });
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::ClassAssertion { .. })));
    }

    #[test]
    fn object_property_assertion_positive_and_negative() {
        let pos = Component::ObjectPropertyAssertion(ho::ObjectPropertyAssertion {
            ope: ope("r"),
            from: named_ind("a"),
            to: named_ind("b"),
        });
        let neg = Component::NegativeObjectPropertyAssertion(ho::NegativeObjectPropertyAssertion {
            ope: ope("r"),
            from: named_ind("a"),
            to: named_ind("b"),
        });
        assert!(matches!(
            convert_one(&pos).1,
            Some(Axiom::ObjectPropertyAssertion { .. })
        ));
        assert!(matches!(
            convert_one(&neg).1,
            Some(Axiom::NegativeObjectPropertyAssertion { .. })
        ));
    }

    #[test]
    fn same_and_different_individuals() {
        let same =
            Component::SameIndividual(ho::SameIndividual(vec![named_ind("a"), named_ind("b")]));
        let diff = Component::DifferentIndividuals(ho::DifferentIndividuals(vec![
            named_ind("a"),
            named_ind("c"),
        ]));
        assert!(matches!(
            convert_one(&same).1,
            Some(Axiom::SameIndividual(_))
        ));
        assert!(matches!(
            convert_one(&diff).1,
            Some(Axiom::DifferentIndividuals(_))
        ));
    }

    #[test]
    fn declarations() {
        assert!(matches!(
            convert_one(&Component::DeclareClass(ho::DeclareClass(b().class("A")))).1,
            Some(Axiom::DeclareClass(_))
        ));
        assert!(matches!(
            convert_one(&Component::DeclareObjectProperty(
                ho::DeclareObjectProperty(b().object_property("r"))
            ))
            .1,
            Some(Axiom::DeclareObjectProperty(_))
        ));
        assert!(matches!(
            convert_one(&Component::DeclareNamedIndividual(
                ho::DeclareNamedIndividual(b().named_individual("a"))
            ))
            .1,
            Some(Axiom::DeclareNamedIndividual(_))
        ));
    }

    #[test]
    fn metadata_and_annotations_silently_skipped() {
        // OntologyID with no IRIs is the default.
        let id = ho::OntologyID::default();
        let (_, ax) = convert_one(&Component::<RcStr>::OntologyID(id));
        assert!(ax.is_none());
        // AnnotationProperty declaration is dropped (not reasoning-load-bearing).
        let ap = Component::<RcStr>::DeclareAnnotationProperty(ho::DeclareAnnotationProperty(
            b().annotation_property("p"),
        ));
        assert!(convert_one(&ap).1.is_none());
    }

    /// Phase D1 (2026-06-03): data-axiom declarations no longer hard-
    /// error — they're silently dropped as sound under-approximation
    /// so the 4 erroring fixtures (family, ro, sio, shoiq-knowledge)
    /// parse + classify. Phase D2 measures FP/MISSED vs Konclude to
    /// decide if real cardinality reasoning (Tier B) is needed.
    #[test]
    fn data_axiom_declarations_silently_dropped() {
        let c = Component::<RcStr>::DeclareDataProperty(ho::DeclareDataProperty(
            b().data_property("dp"),
        ));
        let mut o = InternalOntology::new();
        let result = convert_component(&c, &mut o.vocabulary, &mut o.concepts).unwrap();
        assert!(
            result.is_none(),
            "Phase D1: data-property declarations drop silently (Ok(None))"
        );
    }

    /// Phase D1 / P3: a `SubClassOf` whose SUP is data cardinality.
    /// INTEGER-qualified cardinality LOWERS (P3) to an object `Max`/`Min`
    /// over the integer `DKey` filler; STRING-qualified cardinality now also
    /// LOWERS (P3 string extension) to an object `Max` over the string `DKey`
    /// filler. Both are wired into the concrete-domain solver; only other
    /// datatype buckets still drop (`UnsupportedDataRange` → `Ok(None)`).
    #[test]
    fn subclass_with_data_cardinality_lowering() {
        use horned_owl::model::{DataProperty, DataRange, Datatype, Literal};
        let dp = DataProperty::<RcStr>(b().iri("http://t/dp"));

        // (a) integer-qualified ≤1 → lowers to a `Max` concept.
        let int_card = Component::<RcStr>::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ClassExpression::DataMaxCardinality {
                n: 1,
                dp: dp.clone(),
                dr: DataRange::Datatype(Datatype(
                    b().iri("http://www.w3.org/2001/XMLSchema#integer"),
                )),
            },
        });
        let mut o = InternalOntology::new();
        let result = convert_component(&int_card, &mut o.vocabulary, &mut o.concepts).unwrap();
        let axiom = result.expect("P3: integer data cardinality lowers (not dropped)");
        let crate::ontology::Axiom::SubClassOf { sup, .. } = axiom else {
            panic!("expected SubClassOf");
        };
        assert!(
            matches!(o.concepts.get(sup), ConceptExpr::Max(1, _, _)),
            "expected the SUP to lower to Max(1, dp, DKey(int))"
        );

        // (b) string-DataOneOf cardinality now LOWERS (string bucket wired in P3 extension).
        let str_card = Component::<RcStr>::SubClassOf(ho::SubClassOf {
            sub: ce_class("A"),
            sup: ClassExpression::DataMaxCardinality {
                n: 1,
                dp: dp.clone(),
                dr: DataRange::DataOneOf(vec![Literal::Simple {
                    literal: "x".to_string(),
                }]),
            },
        });
        let mut o2 = InternalOntology::new();
        let result2 = convert_component(&str_card, &mut o2.vocabulary, &mut o2.concepts).unwrap();
        let axiom2 = result2.expect("P3 string: string DataOneOf cardinality now lowers");
        let crate::ontology::Axiom::SubClassOf { sup: sup2, .. } = axiom2 else {
            panic!("expected SubClassOf");
        };
        assert!(
            matches!(o2.concepts.get(sup2), ConceptExpr::Max(1, _, _)),
            "expected the SUP to lower to Max(1, dp, DKey(str))"
        );

        // (c) gate-OFF: rdfs:Literal (unqualified) still drops — but as of
        // issue #43, "drop" at the `convert_component` level is now signaled
        // via `Err(UnsupportedDataRange)` (the `ce_or_skip!` macro no longer
        // intercepts it and downgrades to `Ok(None)`); `convert_ontology` is
        // what turns that `Err` into a recorded, non-aborting drop.
        {
            let _lock = DP_ENV_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _g = DpGuard::off();
            let other_card = Component::<RcStr>::SubClassOf(ho::SubClassOf {
                sub: ce_class("A"),
                sup: ClassExpression::DataMaxCardinality {
                    n: 1,
                    dp: dp.clone(),
                    dr: DataRange::Datatype(Datatype(
                        b().iri("http://www.w3.org/2000/01/rdf-schema#Literal"),
                    )),
                },
            });
            let mut o3 = InternalOntology::new();
            assert_eq!(
                convert_component(&other_card, &mut o3.vocabulary, &mut o3.concepts),
                Err(ConversionError::UnsupportedDataRange),
                "gate OFF: unqualified rdfs:Literal cardinality still drops (now via Err)"
            );
        }

        // (d) gate-ON: rdfs:Literal lowers to Max over ⊤ filler.
        {
            let _lock = DP_ENV_MUTEX
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _g = DpGuard::on();
            let unqual_card = Component::<RcStr>::SubClassOf(ho::SubClassOf {
                sub: ce_class("A"),
                sup: ClassExpression::DataMaxCardinality {
                    n: 1,
                    dp,
                    dr: DataRange::Datatype(Datatype(
                        b().iri("http://www.w3.org/2000/01/rdf-schema#Literal"),
                    )),
                },
            });
            let mut o4 = InternalOntology::new();
            let ax4 = convert_component(&unqual_card, &mut o4.vocabulary, &mut o4.concepts)
                .unwrap()
                .expect("gate ON: unqualified rdfs:Literal cardinality lowers");
            let crate::ontology::Axiom::SubClassOf { sup: sup4, .. } = ax4 else {
                panic!("expected SubClassOf");
            };
            assert!(
                matches!(o4.concepts.get(sup4), ConceptExpr::Max(1, _, _)),
                "gate ON: rdfs:Literal cardinality lowers to Max(1, dp, ⊤)"
            );
        }
    }

    #[test]
    fn convert_ontology_smoke() {
        let mut o = SetOntology::<RcStr>::new();
        o.insert(ho::AnnotatedComponent::from(Component::SubClassOf(
            ho::SubClassOf {
                sub: ce_class("A"),
                sup: ce_class("B"),
            },
        )));
        o.insert(ho::AnnotatedComponent::from(Component::DeclareClass(
            ho::DeclareClass(b().class("A")),
        )));
        let internal = convert_ontology(&o).unwrap();
        assert_eq!(internal.num_axioms(), 2);
        assert_eq!(internal.vocabulary.num_classes(), 2); // A, B
    }

    #[test]
    fn try_from_set_ontology() {
        let mut o = SetOntology::<RcStr>::new();
        o.insert(ho::AnnotatedComponent::from(Component::SubClassOf(
            ho::SubClassOf {
                sub: ce_class("A"),
                sup: ce_class("B"),
            },
        )));
        let internal = InternalOntology::try_from(&o).unwrap();
        assert_eq!(internal.num_axioms(), 1);
    }

    // ── Issue #43: graceful degradation (drop + record) ──────────────────

    /// Parse an OFN-functional-syntax string into a `SetOntology`, mirroring
    /// the `parse_str` helper in `data_axioms.rs`'s test module. The prefix
    /// mapping `read_ofn` also returns isn't needed by these tests, so it's
    /// discarded here.
    fn read_ofn_str(src: &str) -> SetOntology<RcStr> {
        use horned_owl::io::ParserConfiguration;
        use horned_owl::io::ofn::reader::read as read_ofn;
        use std::io::Cursor;
        let mut r = Cursor::new(src);
        let (onto, _prefixes) =
            read_ofn(&mut r, ParserConfiguration::default()).expect("test fixture parses");
        onto
    }

    #[test]
    fn convert_records_dropped_unsupported_axiom_and_continues() {
        // NOTE (deviation from the design brief, VERIFIED against this
        // codebase's current state): the brief's canonical "aborts today"
        // example was an anonymous-individual `ClassAssertion`
        // (`ConversionError::AnonymousIndividual`). That is no longer true
        // here — `convert_individual` (see `ANON_IRI_PREFIX` docs above)
        // already interns anonymous individuals as first-class
        // `IndividualId`s, so `AnonymousIndividual` is dead code (never
        // constructed). The one LIVE `Err(ConversionError::UnsupportedAxiom
        // { .. })` path in `convert_component` is `HasKey` (deferred
        // advanced feature — see its match arm). Before this fix, a
        // `HasKey` component aborted the whole `convert_ontology` call via
        // `?`; after the fix, it converts, the axiom is recorded as
        // dropped, and the supported axioms survive.
        let src = r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(ObjectProperty(:r))
            SubClassOf(:A :B)
            HasKey(:A (:r) ()))";
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("must not abort");
        assert!(
            internal
                .axioms
                .iter()
                .any(|a| matches!(a, Axiom::SubClassOf { .. })),
            "supported axiom survives"
        );
        assert_eq!(
            internal.dropped.total(),
            1,
            "one dropped axiom recorded, got {:?}",
            internal.dropped.by_kind()
        );
        assert!(
            internal
                .dropped
                .by_kind()
                .keys()
                .any(|k| k.contains("HasKey"))
        );
    }

    #[test]
    fn convert_records_dropped_data_range_axiom() {
        // A SubClassOf whose filler is an unsupported nested composite data
        // range: silently dropped today (ce_or_skip → Ok(None)); now recorded.
        let src = r"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(DataProperty(:p))
            SubClassOf(:A DataSomeValuesFrom(:p DataComplementOf(DataUnionOf(xsd:integer xsd:string)))))";
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("must not abort");
        assert_eq!(internal.dropped.total(), 1);
        assert!(
            internal
                .dropped
                .by_kind()
                .keys()
                .any(|k| k.contains("data range"))
        );
    }

    #[test]
    fn convert_benign_drops_not_recorded() {
        // Metadata / annotations must NOT count as dropped.
        let src = r#"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) SubClassOf(:A :B)
            AnnotationAssertion(<http://x/lbl> :A "hi"))"#;
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("ok");
        assert!(
            internal.dropped.is_empty(),
            "benign drops not recorded, got {:?}",
            internal.dropped.by_kind()
        );
    }

    // ── #43 whole-branch review: data-property CONTENT drops must be
    // RECORDED (not silently benign) ─────────────────────────────────────

    #[test]
    fn convert_records_dropped_data_property_assertion_unrecognized_literal() {
        // A DataPropertyAssertion whose literal datatype (xsd:anyURI) is not
        // DKey-recognized used to be silently dropped as `Ok(None)` — the
        // review found this is CONTENT (an ABox assertion), so it must now
        // be a RECORDED drop, and the axiom must NOT appear in `axioms`.
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let src = r#"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(DataProperty(:dp))
            Declaration(NamedIndividual(:a))
            ClassAssertion(:A :a)
            DataPropertyAssertion(:dp :a "x"^^xsd:anyURI))"#;
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("must not abort");
        assert_eq!(
            internal.dropped.total(),
            1,
            "one dropped axiom recorded, got {:?}",
            internal.dropped.by_kind()
        );
        assert!(
            internal
                .dropped
                .by_kind()
                .keys()
                .any(|k| k.starts_with("DataPropertyAssertion:")),
            "expected a DataPropertyAssertion drop kind, got {:?}",
            internal.dropped.by_kind()
        );
        // Only the supported `ClassAssertion(:A :a)` survives as a
        // ClassAssertion; the dropped DataPropertyAssertion (which would
        // otherwise also lower to a ClassAssertion) contributes none of
        // its own.
        let class_assertions = internal
            .axioms
            .iter()
            .filter(|a| matches!(a, Axiom::ClassAssertion { .. }))
            .count();
        assert_eq!(
            class_assertions, 1,
            "the unsupported data-property assertion must not surface as an axiom, got {:?}",
            internal.axioms
        );
    }

    #[test]
    fn convert_records_dropped_data_property_assertion_gate_off() {
        // RUSTDL_DATA_PROPERTIES=0: the gate-off fall-through used to drop
        // ALL data-property axioms silently (`Ok(None)`); the review found
        // this is CONTENT too, so it must now be recorded.
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::off();
        let src = r#"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(DataProperty(:dp))
            Declaration(NamedIndividual(:a))
            ClassAssertion(:A :a)
            DataPropertyAssertion(:dp :a "5"^^xsd:integer))"#;
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("must not abort");
        assert_eq!(
            internal.dropped.total(),
            1,
            "gate-off data-property assertion recorded, got {:?}",
            internal.dropped.by_kind()
        );
        assert!(
            internal
                .dropped
                .by_kind()
                .keys()
                .any(|k| k.starts_with("DataPropertyAssertion:")),
            "expected a DataPropertyAssertion drop kind, got {:?}",
            internal.dropped.by_kind()
        );
    }

    #[test]
    fn convert_declare_data_property_alone_not_recorded() {
        // Benign check: a bare DeclareDataProperty (no assertions/ranges)
        // must stay an un-recorded Ok(None) — declarations are metadata,
        // not reasoning content.
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let src = r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B))
            Declaration(DataProperty(:dp))
            SubClassOf(:A :B))";
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("ok");
        assert!(
            internal.dropped.is_empty(),
            "a bare DeclareDataProperty must not be recorded, got {:?}",
            internal.dropped.by_kind()
        );
    }

    #[test]
    fn convert_fully_supported_ontology_is_inert() {
        // A fully-supported ontology must yield an EMPTY `dropped` and the
        // expected axiom set unchanged by this refactor.
        let src = r"Prefix(:=<http://ex/#>)
          Ontology(<http://ex/>
            Declaration(Class(:A)) Declaration(Class(:B)) Declaration(Class(:C))
            SubClassOf(:A :B)
            EquivalentClasses(:B :C))";
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("ok");
        assert!(
            internal.dropped.is_empty(),
            "fully-supported ontology drops nothing, got {:?}",
            internal.dropped.by_kind()
        );
        assert!(
            internal
                .axioms
                .iter()
                .any(|a| matches!(a, Axiom::SubClassOf { .. })),
            "SubClassOf axiom present"
        );
        assert!(
            internal
                .axioms
                .iter()
                .any(|a| matches!(a, Axiom::EquivalentClasses(_))),
            "EquivalentClasses axiom present"
        );
    }

    // ── Phase D8: DKey codec + parser-matrix + ordering unit tests ───────

    fn dec(s: &str) -> Decimal {
        parse_decimal(s).unwrap_or_else(|| panic!("parse_decimal({s:?})"))
    }

    /// Sample IRI from each of the `DKey` interval/string buckets (a bounded
    /// range so the encoding is exercised, not just `*`). Now includes the
    /// separate `xsd:double` (`db:`) bucket alongside the `xsd:float` (`f:`)
    /// bucket — the two must be mutually exclusive.
    fn sample_iris() -> Vec<(&'static str, String)> {
        vec![
            (
                "int",
                dkey_iri(IntegerRange {
                    min: Some(3),
                    max: Some(9),
                }),
            ),
            ("float", float_dkey_iri(FloatRange::point(1.5))),
            ("double", double_dkey_iri(FloatRange::point(1.5))),
            (
                "dec",
                ord_dkey_iri(DKEY_DECIMAL_TAG, &OrdRange::point(dec("1.5")), decimal_key),
            ),
            (
                "date",
                ord_dkey_iri(DKEY_DATE_TAG, &OrdRange::point((2020, 1, 15)), date_key),
            ),
            (
                "dt",
                ord_dkey_iri(
                    DKEY_DATETIME_TAG,
                    &OrdRange::point((2020, 1, 15, 12, 30, 0)),
                    datetime_key,
                ),
            ),
            (
                "str",
                str_dkey_iri(&StrSet::Set(
                    ["FULL-TIME".to_string(), "PART-TIME".to_string()]
                        .into_iter()
                        .collect(),
                )),
            ),
        ]
    }

    /// THE matrix: each decoder must return `Some` for EXACTLY its own
    /// bucket's IRI and `None` for all others. A single off-diagonal `Some`
    /// = a cross-datatype edge seeded with mismatched semantics = false
    /// positive. Now covers 7 interval/string buckets (int, float, double,
    /// dec, date, dt, str) — float and double must reject each other.
    #[test]
    fn parser_matrix_mutual_exclusivity() {
        let iris = sample_iris();
        let probe = |bucket: &str, iri: &str| -> bool {
            match bucket {
                "int" => parse_dkey_iri(iri).is_some(),
                "float" => parse_float_dkey_iri(iri).is_some(),
                "double" => parse_double_dkey_iri(iri).is_some(),
                "dec" => parse_decimal_dkey_iri(iri).is_some(),
                "date" => parse_date_dkey_iri(iri).is_some(),
                "dt" => parse_datetime_dkey_iri(iri).is_some(),
                "str" => parse_string_dkey_iri(iri).is_some(),
                _ => unreachable!(),
            }
        };
        for (decoder, _) in &iris {
            for (bucket, iri) in &iris {
                let accepted = probe(decoder, iri);
                assert_eq!(
                    accepted,
                    decoder == bucket,
                    "decoder {decoder} on {bucket} IRI {iri:?}: expected {}",
                    decoder == bucket
                );
            }
        }
    }

    /// Companion matrix for the six numeric-`DataOneOf` buckets (`io:` / `fo:` /
    /// `dbo:` / `deo:` / `dao:` / `dto:`). Each oneof decoder must return `Some` for
    /// EXACTLY its own oneof IRI and `None` for every other oneof IRI AND for
    /// every interval/string IRI — and, conversely, every interval/string decoder
    /// (including the untagged integer-interval `parse_dkey_iri`, the riskiest
    /// because it does no tag check) must REJECT every oneof IRI. A single
    /// off-diagonal `Some` = a cross-bucket decode → wrong `CardRange` → wrong
    /// capacity → potential false-positive subsumption (FP-critical).
    #[test]
    fn numeric_oneof_parser_matrix_exclusivity() {
        // Bounded oneof IRIs (one per bucket) built via the real encoders.
        let oneof: Vec<(&str, String)> = vec![
            ("io", int_oneof_iri(&[1_i64, 2].into_iter().collect())),
            (
                "fo",
                float_oneof_iri(
                    &[
                        crate::data_axioms::OrdF64::new(1.5),
                        crate::data_axioms::OrdF64::new(2.5),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                "dbo",
                double_oneof_iri(
                    &[
                        crate::data_axioms::OrdF64::new(1.5),
                        crate::data_axioms::OrdF64::new(2.5),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                "deo",
                decimal_oneof_iri(&[dec("1.5"), dec("2.5")].into_iter().collect()),
            ),
            (
                "dao",
                date_oneof_iri(&[(2020, 1, 1), (2020, 1, 2)].into_iter().collect()),
            ),
            (
                "dto",
                datetime_oneof_iri(
                    &[(2020, 1, 1, 0, 0, 0), (2020, 1, 1, 1, 0, 0)]
                        .into_iter()
                        .collect(),
                ),
            ),
        ];
        // Every IRI in the system: the 7 interval/string samples + the 6 oneof.
        let mut all = sample_iris();
        all.extend(oneof.iter().cloned());

        // Decoders for ALL thirteen buckets (7 interval/string + 6 oneof).
        // `db:` (double interval) vs `dbo:` (double oneof) is the newest
        // near-collision the matrix has to rule out.
        let probe = |bucket: &str, iri: &str| -> bool {
            match bucket {
                "int" => parse_dkey_iri(iri).is_some(),
                "float" => parse_float_dkey_iri(iri).is_some(),
                "double" => parse_double_dkey_iri(iri).is_some(),
                "dec" => parse_decimal_dkey_iri(iri).is_some(),
                "date" => parse_date_dkey_iri(iri).is_some(),
                "dt" => parse_datetime_dkey_iri(iri).is_some(),
                "str" => parse_string_dkey_iri(iri).is_some(),
                "io" => parse_int_oneof_iri(iri).is_some(),
                "fo" => parse_float_oneof_iri(iri).is_some(),
                "dbo" => parse_double_oneof_iri(iri).is_some(),
                "deo" => parse_decimal_oneof_iri(iri).is_some(),
                "dao" => parse_date_oneof_iri(iri).is_some(),
                "dto" => parse_datetime_oneof_iri(iri).is_some(),
                _ => unreachable!(),
            }
        };
        for (decoder, _) in &all {
            for (bucket, iri) in &all {
                let accepted = probe(decoder, iri);
                assert_eq!(
                    accepted,
                    decoder == bucket,
                    "decoder {decoder} on {bucket} IRI {iri:?}: expected {}",
                    decoder == bucket
                );
            }
        }
    }

    #[test]
    fn dkey_iri_round_trips_all_buckets() {
        // decimal: exact lexical round-trip (incl. negative + sub-1).
        let dr = OrdRange {
            min: Some(dec("-0.5")),
            min_incl: false,
            max: Some(dec("12.34")),
            max_incl: true,
        };
        assert_eq!(
            parse_decimal_dkey_iri(&ord_dkey_iri(DKEY_DECIMAL_TAG, &dr, decimal_key)),
            Some(dr)
        );
        // date: bounded, with an unbounded upper end.
        let dtr = OrdRange {
            min: Some((1999, 12, 31)),
            min_incl: true,
            max: None,
            max_incl: false,
        };
        assert_eq!(
            parse_date_dkey_iri(&ord_dkey_iri(DKEY_DATE_TAG, &dtr, date_key)),
            Some(dtr)
        );
        // dateTime: full six-component round-trip.
        let dttr = OrdRange::point((2020, 6, 9, 8, 15, 45));
        assert_eq!(
            parse_datetime_dkey_iri(&ord_dkey_iri(DKEY_DATETIME_TAG, &dttr, datetime_key)),
            Some(dttr)
        );
    }

    /// The public concrete-domain-solver decode (`is_dkey_iri` +
    /// `decode_integer_dkey`): integer `DKey`s round-trip to primitive bounds;
    /// non-integer-bucket and non-`DKey` IRIs decode to `None`.
    #[test]
    fn public_integer_dkey_decode() {
        let iri = dkey_iri(IntegerRange {
            min: Some(0),
            max: Some(10),
        });
        assert!(is_dkey_iri(&iri));
        assert_eq!(decode_integer_dkey(&iri), Some((Some(0), Some(10))));
        // unbounded-below integer DKey.
        let iri2 = dkey_iri(IntegerRange {
            min: None,
            max: Some(5),
        });
        assert_eq!(decode_integer_dkey(&iri2), Some((None, Some(5))));
        // a float-bucket DKey is NOT an integer DKey (tag fails i64 parse).
        let f = float_dkey_iri(FloatRange {
            min: Some(0.0),
            min_incl: true,
            max: None,
            max_incl: false,
        });
        assert!(is_dkey_iri(&f));
        assert_eq!(decode_integer_dkey(&f), None);
        // a plain class IRI is not a DKey at all.
        assert!(!is_dkey_iri("http://example.org/C"));
        assert_eq!(decode_integer_dkey("http://example.org/C"), None);
    }

    #[test]
    fn decimal_ordering_is_exact() {
        // The FP trap: distinct decimals must never compare equal.
        assert!(dec("0.1") < dec("0.2"));
        assert!(dec("0.45") < dec("0.5")); // pad-to-equal-length, not lex-raw
        assert!(dec("-0.5") < dec("0.5"));
        assert!(dec("-2") < dec("-1")); // larger magnitude ⟹ smaller
        assert_eq!(dec("1.0"), dec("1.00")); // trailing-zero normalization
        assert_eq!(dec("-0"), dec("0")); // signed-zero collapse
        assert_eq!(dec("007.50"), dec("7.5")); // leading + trailing zeros
        assert!(dec("10") > dec("9")); // length-then-lex, not raw lex
        // subset boundary semantics on the real-ish decimal line.
        let open = OrdRange {
            min: Some(dec("0")),
            min_incl: false,
            max: Some(dec("1")),
            max_incl: false,
        };
        assert!(OrdRange::point(dec("0.5")).subset(&open));
        assert!(!OrdRange::point(dec("0")).subset(&open)); // excluded endpoint
        assert!(!OrdRange::point(dec("1")).subset(&open));
    }

    #[test]
    fn temporal_parse_drops_timezone_and_fraction() {
        // Unzoned forms parse.
        assert_eq!(parse_date("2020-01-15"), Some((2020, 1, 15)));
        assert_eq!(
            parse_datetime("2020-01-15T08:30:00"),
            Some((2020, 1, 15, 8, 30, 0))
        );
        // Any timezone or fractional second → dropped (None): the
        // partial-order / precision soundness guards.
        assert_eq!(parse_date("2020-01-15Z"), None);
        assert_eq!(parse_date("2020-01-15+05:00"), None);
        assert_eq!(parse_datetime("2020-01-15T08:30:00Z"), None);
        assert_eq!(parse_datetime("2020-01-15T08:30:00.5"), None);
        assert_eq!(parse_datetime("2020-01-15T08:30:00-05:00"), None);
        // Out-of-range components → dropped.
        assert_eq!(parse_date("2020-13-01"), None);
        assert_eq!(parse_datetime("2020-01-15T25:00:00"), None);
        // Chronological tuple order.
        assert!(parse_date("2019-12-31") < parse_date("2020-01-01"));
        assert!(parse_datetime("2020-01-15T08:00:00") < parse_datetime("2020-01-15T08:00:01"));
    }

    #[test]
    fn string_dkey_round_trips_and_subsets() {
        // Round-trip through hex encoding, including content that contains
        // the IRI delimiters (`:`/`.`) and unicode — the reason for hex.
        let s = StrSet::Set(
            ["a:b".to_string(), "c.d".to_string(), "é".to_string()]
                .into_iter()
                .collect(),
        );
        assert_eq!(parse_string_dkey_iri(&str_dkey_iri(&s)), Some(s));
        assert_eq!(
            parse_string_dkey_iri(&str_dkey_iri(&StrSet::Top)),
            Some(StrSet::Top)
        );
        // Set-containment semantics.
        let pair = StrSet::Set(
            ["FULL-TIME".to_string(), "PART-TIME".to_string()]
                .into_iter()
                .collect(),
        );
        assert!(StrSet::singleton("FULL-TIME".to_string()).subset(&pair));
        assert!(!StrSet::singleton("CONTRACT".to_string()).subset(&pair));
        assert!(pair.subset(&StrSet::Top)); // anything ⊆ Top
        assert!(!StrSet::Top.subset(&pair)); // Top ⊄ a finite set
        // A `*`-marked Top can never be confused with a set member: hex is
        // never "*", so a singleton {"*"} encodes distinctly.
        let star = StrSet::singleton("*".to_string());
        assert_eq!(parse_string_dkey_iri(&str_dkey_iri(&star)), Some(star));
    }

    // ---- HF3: long-chain decomposition ----

    /// Collect the 2-leg chain axioms from an ontology as
    /// `(leg0_id, leg1_id, sup_id)` tuples (named roles only).
    fn two_leg_chains(o: &InternalOntology) -> Vec<(u32, u32, u32)> {
        o.axioms
            .iter()
            .filter_map(|ax| match ax {
                Axiom::SubObjectPropertyOf {
                    sub: SubRolePath::Chain(p),
                    sup,
                } if p.len() == 2 => Some((
                    p[0].role_id().index(),
                    p[1].role_id().index(),
                    sup.role_id().index(),
                )),
                _ => None,
            })
            .collect()
    }

    fn has_long_chain(o: &InternalOntology) -> bool {
        o.axioms.iter().any(|ax| {
            matches!(ax, Axiom::SubObjectPropertyOf { sub: SubRolePath::Chain(p), .. } if p.len() > 2)
        })
    }

    #[test]
    fn decompose_three_leg_chain_to_two_two_leg() {
        let mut o = InternalOntology::new();
        let (r0, r1, r2, s) = (
            o.vocabulary.intern_role("http://x/r0"),
            o.vocabulary.intern_role("http://x/r1"),
            o.vocabulary.intern_role("http://x/r2"),
            o.vocabulary.intern_role("http://x/s"),
        );
        let n0 = o.vocabulary.num_roles();
        o.axioms.push(Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(vec![Role::Named(r0), Role::Named(r1), Role::Named(r2)]),
            sup: Role::Named(s),
        });
        decompose_long_chains(&mut o);
        // No long chain remains.
        assert!(!has_long_chain(&o), "3-leg chain must be decomposed away");
        // Exactly one fresh aux role allocated.
        assert_eq!(o.vocabulary.num_roles(), n0 + 1, "one aux role expected");
        let aux = u32::try_from(n0).expect("fits"); // first fresh id after the 4 declared roles
        let chains = two_leg_chains(&o);
        // R0∘R1 ⊑ aux ; aux∘R2 ⊑ S.
        assert!(
            chains.contains(&(r0.index(), r1.index(), aux)),
            "expected R0∘R1⊑aux; chains={chains:?}"
        );
        assert!(
            chains.contains(&(aux, r2.index(), s.index())),
            "expected aux∘R2⊑S; chains={chains:?}"
        );
    }

    #[test]
    fn decompose_four_leg_chain() {
        let mut o = InternalOntology::new();
        let ids: Vec<u32> = (0..4)
            .map(|i| o.vocabulary.intern_role(&format!("http://x/r{i}")).index())
            .collect();
        let s = o.vocabulary.intern_role("http://x/s").index();
        let n0 = o.vocabulary.num_roles();
        o.axioms.push(Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(
                ids.iter()
                    .map(|&i| Role::Named(crate::ir::RoleId::new(i)))
                    .collect(),
            ),
            sup: Role::Named(crate::ir::RoleId::new(s)),
        });
        decompose_long_chains(&mut o);
        assert!(!has_long_chain(&o));
        // 4 legs → 3 two-leg chains → 2 aux roles.
        assert_eq!(o.vocabulary.num_roles(), n0 + 2, "two aux roles expected");
        assert_eq!(two_leg_chains(&o).len(), 3, "4-leg → three 2-leg chains");
    }

    #[test]
    fn decompose_two_distinct_chains_use_disjoint_aux() {
        // Soundness: two different 3-leg chains must NOT share an aux id.
        let mut o = InternalOntology::new();
        let r = |o: &mut InternalOntology, n: &str| o.vocabulary.intern_role(n).index();
        let (a0, a1, a2, sa) = (
            r(&mut o, "http://x/a0"),
            r(&mut o, "http://x/a1"),
            r(&mut o, "http://x/a2"),
            r(&mut o, "http://x/sa"),
        );
        let (b0, b1, b2, sb) = (
            r(&mut o, "http://x/b0"),
            r(&mut o, "http://x/b1"),
            r(&mut o, "http://x/b2"),
            r(&mut o, "http://x/sb"),
        );
        use crate::ir::RoleId;
        o.axioms.push(Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(vec![
                Role::Named(RoleId::new(a0)),
                Role::Named(RoleId::new(a1)),
                Role::Named(RoleId::new(a2)),
            ]),
            sup: Role::Named(RoleId::new(sa)),
        });
        o.axioms.push(Axiom::SubObjectPropertyOf {
            sub: SubRolePath::Chain(vec![
                Role::Named(RoleId::new(b0)),
                Role::Named(RoleId::new(b1)),
                Role::Named(RoleId::new(b2)),
            ]),
            sup: Role::Named(RoleId::new(sb)),
        });
        let n0 = o.vocabulary.num_roles();
        decompose_long_chains(&mut o);
        // Two 3-leg chains → 2 aux roles, distinct.
        assert_eq!(o.vocabulary.num_roles(), n0 + 2, "two distinct aux roles");
        let chains = two_leg_chains(&o);
        // The aux feeding chain A must differ from the aux feeding chain B.
        let aux_a = chains
            .iter()
            .find(|(l0, l1, _)| *l0 == a0 && *l1 == a1)
            .map(|(_, _, sup)| *sup)
            .expect("chain A prefix present");
        let aux_b = chains
            .iter()
            .find(|(l0, l1, _)| *l0 == b0 && *l1 == b1)
            .map(|(_, _, sup)| *sup)
            .expect("chain B prefix present");
        assert_ne!(aux_a, aux_b, "distinct chains must use distinct aux roles");
    }

    // ── DataPropertyAssertion gated-lowering tests (RUSTDL_DATA_PROPERTIES) ─

    static DP_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct DpGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl DpGuard {
        #[allow(unsafe_code)]
        fn on() -> Self {
            let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
            // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "1") };
            Self { prior }
        }

        #[allow(unsafe_code)]
        fn off() -> Self {
            let prior = std::env::var_os("RUSTDL_DATA_PROPERTIES");
            // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_DATA_PROPERTIES", "0") };
            Self { prior }
        }
    }
    impl Drop for DpGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: see DpGuard::on.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("RUSTDL_DATA_PROPERTIES", v),
                    None => std::env::remove_var("RUSTDL_DATA_PROPERTIES"),
                }
            }
        }
    }

    const XSD_INT: &str = "http://www.w3.org/2001/XMLSchema#integer";

    fn int_dp_assertion(dp: &str, ind: &str, lexical: &str, dt: &str) -> Component<RcStr> {
        Component::DataPropertyAssertion(ho::DataPropertyAssertion {
            dp: b().data_property(dp),
            from: named_ind(ind),
            to: ho::Literal::Datatype {
                literal: lexical.to_string(),
                datatype_iri: b().iri(dt),
            },
        })
    }

    #[test]
    fn data_property_assertion_lowers_when_gate_on() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = int_dp_assertion("http://t/dp", "http://t/a", "5", XSD_INT);
        let (_, ax) = convert_one(&c);
        assert!(
            matches!(ax, Some(Axiom::ClassAssertion { .. })),
            "gate ON: data assertion lowers to ClassAssertion; got {ax:?}"
        );
    }

    #[test]
    fn data_property_assertion_dropped_when_gate_off() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::off();
        let c = int_dp_assertion("http://t/dp", "http://t/a", "5", XSD_INT);
        let mut o = InternalOntology::new();
        // Issue #43 review: gate-OFF now drops via a RECORDED Err (content
        // axiom), not a benign Ok(None) — see the `convert_ontology` loop.
        let result = convert_component(&c, &mut o.vocabulary, &mut o.concepts);
        assert_eq!(
            result,
            Err(ConversionError::UnsupportedDataRange),
            "gate OFF: data assertion dropped as a recorded Err; got {result:?}"
        );
    }

    #[test]
    fn data_property_assertion_unrecognized_literal_dropped() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        // anyURI is not a DKey-recognized datatype ⇒ drop even with gate ON.
        // Issue #43 review: this is CONTENT (an ABox assertion), so it's now
        // a recorded Err rather than a benign Ok(None).
        let c = int_dp_assertion(
            "http://t/dp",
            "http://t/a",
            "x",
            "http://www.w3.org/2001/XMLSchema#anyURI",
        );
        let mut o = InternalOntology::new();
        let result = convert_component(&c, &mut o.vocabulary, &mut o.concepts);
        assert_eq!(
            result,
            Err(ConversionError::UnsupportedDataRange),
            "unrecognized datatype dropped as a recorded Err; got {result:?}"
        );
    }

    #[test]
    fn negative_data_property_assertion_lowers_to_complement() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::NegativeDataPropertyAssertion(ho::NegativeDataPropertyAssertion {
            dp: b().data_property("http://t/dp"),
            from: named_ind("http://t/a"),
            to: ho::Literal::Datatype {
                literal: "5".into(),
                datatype_iri: b().iri(XSD_INT),
            },
        });
        let (o, ax) = convert_one(&c);
        let Some(Axiom::ClassAssertion { class, .. }) = ax else {
            panic!("expected ClassAssertion, got {ax:?}");
        };
        // ¬∃dp.DKey — a Not concept (pool.not(...)).
        assert!(
            matches!(o.concepts.get(class), ConceptExpr::Not(_)),
            "expected a Not concept"
        );
    }

    #[test]
    fn sub_data_property_lowers_to_role_hierarchy() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::SubDataPropertyOf(ho::SubDataPropertyOf {
            sub: b().data_property("http://t/dp"),
            sup: b().data_property("http://t/dq"),
        });
        let (_, ax) = convert_one(&c);
        assert!(
            matches!(ax, Some(Axiom::SubObjectPropertyOf { .. })),
            "got {ax:?}"
        );
    }

    #[test]
    fn equivalent_data_properties_lowers_to_equivalent_roles() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::EquivalentDataProperties(ho::EquivalentDataProperties(vec![
            b().data_property("http://t/dp"),
            b().data_property("http://t/dq"),
        ]));
        let (_, ax) = convert_one(&c);
        let Some(Axiom::EquivalentObjectProperties(roles)) = ax else {
            panic!("got {ax:?}")
        };
        assert_eq!(roles.len(), 2);
    }

    #[test]
    fn disjoint_data_properties_lowers_to_disjoint_roles() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::DisjointDataProperties(ho::DisjointDataProperties(vec![
            b().data_property("http://t/dp"),
            b().data_property("http://t/dq"),
        ]));
        let (_, ax) = convert_one(&c);
        assert!(
            matches!(ax, Some(Axiom::DisjointObjectProperties(_))),
            "got {ax:?}"
        );
    }

    #[test]
    fn functional_data_property_lowers_to_functional_role() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::FunctionalDataProperty(ho::FunctionalDataProperty(
            b().data_property("http://t/dp"),
        ));
        let (_, ax) = convert_one(&c);
        assert!(matches!(ax, Some(Axiom::FunctionalRole(_))), "got {ax:?}");
    }

    #[test]
    fn data_property_domain_lowers_to_object_domain() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::DataPropertyDomain(ho::DataPropertyDomain {
            dp: b().data_property("http://t/dp"),
            ce: ClassExpression::Class(b().class("http://t/C")),
        });
        let (_, ax) = convert_one(&c);
        assert!(
            matches!(ax, Some(Axiom::ObjectPropertyDomain { .. })),
            "got {ax:?}"
        );
    }

    #[test]
    fn data_property_range_integer_lowers_to_object_range() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let c = Component::DataPropertyRange(ho::DataPropertyRange {
            dp: b().data_property("http://t/dp"),
            dr: DataRange::Datatype(b().datatype("http://www.w3.org/2001/XMLSchema#integer")),
        });
        let (_, ax) = convert_one(&c);
        assert!(
            matches!(ax, Some(Axiom::ObjectPropertyRange { .. })),
            "got {ax:?}"
        );
    }

    // ── Bounded DKey-disjointness seeding (RUSTDL_BOUNDED_DKEY_DISJOINT) ──
    // Merge-aware role-component bound (2026-07-20). All tests hold
    // DP_ENV_MUTEX: they depend on RUSTDL_DATA_PROPERTIES and (the flag-off
    // test) toggle RUSTDL_BOUNDED_DKEY_DISJOINT.

    struct BoundedGuard {
        prior: Option<std::ffi::OsString>,
    }
    impl BoundedGuard {
        #[allow(unsafe_code)]
        fn off() -> Self {
            let prior = std::env::var_os("RUSTDL_BOUNDED_DKEY_DISJOINT");
            // SAFETY: serialized via DP_ENV_MUTEX; restored on Drop.
            unsafe { std::env::set_var("RUSTDL_BOUNDED_DKEY_DISJOINT", "0") };
            Self { prior }
        }
    }
    impl Drop for BoundedGuard {
        #[allow(unsafe_code)]
        fn drop(&mut self) {
            // SAFETY: see BoundedGuard::off.
            unsafe {
                match &self.prior {
                    Some(v) => std::env::set_var("RUSTDL_BOUNDED_DKEY_DISJOINT", v),
                    None => std::env::remove_var("RUSTDL_BOUNDED_DKEY_DISJOINT"),
                }
            }
        }
    }

    /// Count seeded `DisjointClasses` axioms whose operands are all `DKeys`.
    fn dkey_disjoint_count(o: &InternalOntology) -> usize {
        o.axioms
            .iter()
            .filter(|ax| {
                matches!(ax, Axiom::DisjointClasses(cs) if cs.iter().all(|&c| {
                    matches!(o.concepts.get(c), ConceptExpr::Atomic(cid)
                        if is_dkey_iri(o.vocabulary.class_iri(*cid)))
                }))
            })
            .count()
    }

    fn ins(o: &mut SetOntology<RcStr>, c: Component<RcStr>) {
        o.insert(ho::AnnotatedComponent::from(c));
    }

    fn sub_dp(sub: &str, sup: &str) -> Component<RcStr> {
        Component::SubDataPropertyOf(ho::SubDataPropertyOf {
            sub: b().data_property(sub),
            sup: b().data_property(sup),
        })
    }

    fn functional_dp(dp: &str) -> Component<RcStr> {
        Component::FunctionalDataProperty(ho::FunctionalDataProperty(b().data_property(dp)))
    }

    #[test]
    fn bounded_dkey_disjoint_skips_unrelated_roles() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let mut o = SetOntology::<RcStr>::new();
        // Both properties are FUNCTIONAL, so each is merge-inducing and its own
        // same-role pair is genuinely consumable. Without this the whole fixture
        // is non-merging and the correct answer is 0 — which is what
        // `non_merging_data_property_seeds_no_dkey_disjointness` covers. Making
        // them functional is what keeps THIS test about the property it was
        // written for: unrelated roles do not cross-seed.
        ins(&mut o, functional_dp("http://t/dp1"));
        ins(&mut o, functional_dp("http://t/dp2"));
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "2", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "3", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "4", XSD_INT),
        );
        let out = convert_ontology(&o).unwrap();
        // dp1/dp2 are unconnected: only the same-role pairs (1,2) and (3,4)
        // are seeded; the four cross-role pairs are provably unconsumable.
        // 2, not 6 — that gap IS the bounded-seeding property under test.
        assert_eq!(dkey_disjoint_count(&out), 2);
    }

    #[test]
    fn bounded_dkey_disjoint_unions_via_functional_super() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let mut o = SetOntology::<RcStr>::new();
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "2", XSD_INT),
        );
        ins(&mut o, sub_dp("http://t/dp1", "http://t/f"));
        ins(&mut o, sub_dp("http://t/dp2", "http://t/f"));
        ins(&mut o, functional_dp("http://t/f"));
        let out = convert_ontology(&o).unwrap();
        // dp1 and dp2 share the merge-inducing (functional) super f: their
        // fillers CAN co-occur after the ≤1 merge — the pair must be seeded.
        assert_eq!(dkey_disjoint_count(&out), 1);
    }

    #[test]
    fn bounded_dkey_disjoint_ignores_non_merge_super() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let mut o = SetOntology::<RcStr>::new();
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "2", XSD_INT),
        );
        ins(&mut o, sub_dp("http://t/dp1", "http://t/f"));
        ins(&mut o, sub_dp("http://t/dp2", "http://t/f"));
        let out = convert_ontology(&o).unwrap();
        // Dead-end #3 guard: a shared NON-merge-inducing super (the
        // owl:topDataProperty pattern) must NOT union dp1 with dp2 —
        // that collapse is what re-creates the O(k²) component.
        assert_eq!(dkey_disjoint_count(&out), 0);
    }

    #[test]
    fn bounded_dkey_disjoint_transitive_functional_super() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let mut o = SetOntology::<RcStr>::new();
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "2", XSD_INT),
        );
        // dp1 ⊑ mid ⊑ f (functional), dp2 ⊑ f: merge-inducing-ness must
        // close DOWNWARD through the hierarchy (M*), not just direct supers.
        ins(&mut o, sub_dp("http://t/dp1", "http://t/mid"));
        ins(&mut o, sub_dp("http://t/mid", "http://t/f"));
        ins(&mut o, sub_dp("http://t/dp2", "http://t/f"));
        ins(&mut o, functional_dp("http://t/f"));
        let out = convert_ontology(&o).unwrap();
        assert_eq!(dkey_disjoint_count(&out), 1);
    }

    #[test]
    fn bounded_dkey_disjoint_range_anchors_sub_role() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let mut o = SetOntology::<RcStr>::new();
        // value 1 on dp1, dp1 ⊑ f, and a DKey range [10,∞) on f: the range
        // key lands in every f-successor label (incl. dp1-successors), so
        // the (point, range) pair IS consumable and must be seeded even
        // though nothing is functional.
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(&mut o, sub_dp("http://t/dp1", "http://t/f"));
        ins(
            &mut o,
            Component::DataPropertyRange(ho::DataPropertyRange {
                dp: b().data_property("http://t/f"),
                dr: DataRange::DatatypeRestriction(
                    b().datatype("http://www.w3.org/2001/XMLSchema#integer"),
                    vec![ho::FacetRestriction {
                        f: horned_owl::vocab::Facet::MinInclusive,
                        l: ho::Literal::Datatype {
                            literal: "10".to_string(),
                            datatype_iri: b().iri(XSD_INT),
                        },
                    }],
                ),
            }),
        );
        let out = convert_ontology(&o).unwrap();
        assert_eq!(dkey_disjoint_count(&out), 1);
    }

    #[test]
    fn unbounded_flag_restores_all_pairs() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let _b = BoundedGuard::off();
        let mut o = SetOntology::<RcStr>::new();
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "1", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp1", "http://t/a", "2", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "3", XSD_INT),
        );
        ins(
            &mut o,
            int_dp_assertion("http://t/dp2", "http://t/a", "4", XSD_INT),
        );
        let out = convert_ontology(&o).unwrap();
        // `RUSTDL_BOUNDED_DKEY_DISJOINT=0`: unconditional all-pairs — all
        // C(4,2)=6 pairwise-disjoint point pairs.
        assert_eq!(dkey_disjoint_count(&out), 6);
    }

    // ── Merging-gate boundary tests (RUSTDL_DKEY_MERGING_GATE) ────────────
    // These two fixtures differ in EXACTLY ONE axiom — `FunctionalDataProperty`
    // — isolating the gate from every other property of the input.

    /// GATE BOUNDARY, negative side: three integer data values on data property
    /// `:p` with NO merge-inducing characteristic. Nothing can put two `DKey`s
    /// in one node label, so ZERO disjointness pairs must be seeded.
    ///
    /// Non-vacuity: this test FAILS under `RUSTDL_DKEY_MERGING_GATE=0`
    /// (3 pairs get seeded without the gate).
    #[test]
    fn non_merging_data_property_seeds_no_dkey_disjointness() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let src = r#"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(DataProperty(:p))
            Declaration(NamedIndividual(:a))
            Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c))
            DataPropertyAssertion(:p :a "1"^^xsd:integer)
            DataPropertyAssertion(:p :b "2"^^xsd:integer)
            DataPropertyAssertion(:p :c "3"^^xsd:integer))"#;
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("test fixture converts");
        assert_eq!(
            dkey_disjoint_count(&internal),
            0,
            "a non-merge-inducing data property must seed no `DKey` disjointness"
        );
    }

    /// GATE BOUNDARY, positive side: the same fixture plus one
    /// `FunctionalDataProperty` axiom. `:p` is now merge-inducing, so the three
    /// values CAN be forced onto one node and all 3 pairs must be seeded.
    #[test]
    fn functional_data_property_still_seeds_dkey_disjointness() {
        let _lock = DP_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _g = DpGuard::on();
        let src = r#"Prefix(:=<http://ex/#>) Prefix(xsd:=<http://www.w3.org/2001/XMLSchema#>)
          Ontology(<http://ex/>
            Declaration(DataProperty(:p))
            FunctionalDataProperty(:p)
            Declaration(NamedIndividual(:a))
            Declaration(NamedIndividual(:b))
            Declaration(NamedIndividual(:c))
            DataPropertyAssertion(:p :a "1"^^xsd:integer)
            DataPropertyAssertion(:p :b "2"^^xsd:integer)
            DataPropertyAssertion(:p :c "3"^^xsd:integer))"#;
        let onto = read_ofn_str(src);
        let internal = convert_ontology(&onto).expect("test fixture converts");
        assert_eq!(
            dkey_disjoint_count(&internal),
            3,
            "a functional data property is merge-inducing: all 3 pairs must be seeded"
        );
    }
}
