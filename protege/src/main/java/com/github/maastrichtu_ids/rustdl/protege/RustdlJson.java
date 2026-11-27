package com.github.maastrichtu_ids.rustdl.protege;

import java.util.List;

/** gson-mapped POJOs for the rustdl --json contract (docs/json-schema.md, schema_version 1). */
public final class RustdlJson {
    private RustdlJson() {}

    public static final class ClassifyJson {
        public int schema_version;
        public boolean consistent;
        public boolean incomplete;
        /**
         * The wedge decided pairs without its role-hierarchy matching on an ontology with a
         * non-trivial role hierarchy (#214), so entailments needing sub-role, inverse or
         * symmetric reasoning may be missing. Separate from {@code incomplete}, which means
         * only that a class pair or probe was cut. Absent in output from rustdl before
         * 0.4.37, read as false.
         */
        public boolean wedge_hierarchy_blind;
        /**
         * How many clauses the wedge engine cannot match (#353). A refused clause never fires,
         * so entailments that need it may be missing. Separate from {@code incomplete}, which
         * means only that a class pair or probe was cut. Absent in output from rustdl
         * before 0.4.42, read as 0.
         */
        public int wedge_clauses_refused;
        /**
         * How many candidate individual equalities rustdl did not decide (#354): a probe was
         * cut or the probe cap was reached, so entailments that need them may be missing.
         * Separate from {@code incomplete}. Absent in output from rustdl before 0.4.42, read as 0.
         */
        public int derived_same_undecided;
        /**
         * #610: axioms rustdl could not convert and left out, by label
         * ({@code "HasKey: unsupported axiom (HasKey)" -> 1});
         * the hierarchy is computed over a strictly weaker ontology. Absent (null) in older
         * output.
         */
        public java.util.Map<String, Long> dropped;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        /**
         * #610: consistency rests on the consistency wedge's model alone, which is not proven
         * complete; if that model is wrong the KB is inconsistent. Absent in older output, false.
         */
        public boolean relies_on_trusted_consistency;
        /**
         * #558 / #610: with {@code relies_on_trusted_consistency}, the clauses the consistency
         * wedge refused and its clausifier's deferral events: its model is of a KB weaker by
         * these. Absent in older output, 0.
         */
        public int consistency_wedge_clauses_refused;
        public int consistency_wedge_clauses_deferred;
        /**
         * #446 / #610: rustdl could not establish that the KB is consistent ({@code consistent:
         * true} then means no clash was found). If it is in fact inconsistent, every class is
         * unsatisfiable. Absent in older output, false.
         */
        public boolean consistency_undetermined;
        /**
         * #533 / #616: classify trusted a wedge model that touches a nominal while some ABox
         * fact did not apply in it, so a subsumption or an unsatisfiable class that depends
         * on an individual's facts may be missing. Not a cut: {@code incomplete} stays false.
         * Near-universal on input with both a nominal and an ABox. Absent in older output,
         * false.
         */
        public boolean wedge_abox_blind;
        /**
         * #462 / #616: class pairs whose probe ended in {@code NoVerdict} on an internal
         * limit, likely a reasoner bug (#449). rustdl prints this JSON and then exits 3; the
         * plugin accepts that exit (see {@link RustdlProcess#classify}). Absent in older
         * output, 0.
         */
        public int no_verdict_pairs;
        public List<String> unsatisfiable;
        public List<List<String>> equivalent_groups;
        public List<List<String>> direct_subsumptions;
    }
    public static final class ConsistentJson {
        public int schema_version;
        public boolean consistent;
    }
    public static final class RealizeJson {
        public int schema_version;
        /** #610: an instance probe was cut, so a type may be missing. */
        public boolean incomplete;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        /**
         * #574 / #616: a cut instance probe hit the tableau's decision-depth limit, where a
         * larger budget does not help. Implies {@code incomplete}. Absent in older output, false.
         */
        public boolean depth_capped;
        /**
         * #387 / #616: the pseudo-model witness prune answered "not a type" from a witness
         * with a known reason to lack an entailed type, so a type may be missing with
         * {@code incomplete: false}. {@code false} is "no known reason", not a guarantee
         * (rustdl's {@code witness_prune_active}, not read here, says only that the prune ran).
         * Absent in older output, false.
         */
        public boolean witness_prune_may_drop;
        /** #387: why the witness may lack a type ({@code "branched"}, ...); null in older output. */
        public List<String> witness_prune_reasons;
        /**
         * #388 / #616: some individual has two direct types whose subsumption was left
         * undecided, so a listed direct type may be a superclass of another; every listed
         * type is a genuine type. Absent in older output, false.
         */
        public boolean direct_types_unverified;
        /**
         * #583 / #616: with {@code direct_types_unverified}, at least one of those checks hit
         * the decision-depth limit. Absent in older output, false.
         */
        public boolean direct_types_depth_capped;
        /**
         * #493 / #616: a probe or check ended in {@code NoVerdict} on an internal limit,
         * likely a reasoner bug (#449); each took its sound default, so {@code incomplete} is
         * set too. rustdl prints this JSON and then exits 3; the plugin accepts that exit
         * (see {@link RustdlProcess#realize}). Absent in older output, false.
         */
        public boolean no_verdict;
        /**
         * #616: axioms rustdl could not convert and left out, by label, as on
         * {@link ClassifyJson#dropped}. Absent (null) in older output.
         */
        public java.util.Map<String, Long> dropped;
        public List<IndividualJson> individuals;
    }
    public static final class IndividualJson {
        public String iri;
        public List<String> types;
        public List<String> direct_types;
    }

    public static final class DisjointJson {
        public int schema_version;
        public boolean incomplete;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        /**
         * #610: the KB's consistency stayed undetermined, so a positive answer may be the
         * vacuous one of an inconsistent KB (see docs/json-schema.md for this surface's
         * scope). Absent in older output, false.
         */
        public boolean consistency_undetermined;
        public List<List<String>> disjoint_classes;
        public List<List<String>> disjoint_object_properties;
        public List<List<String>> disjoint_data_properties;
    }
    public static final class PropHierSide {
        public List<List<String>> equivalent_groups;
        public List<List<String>> direct_subsumptions;
    }
    public static final class PropHierJson {
        public int schema_version;
        public boolean incomplete;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        public PropHierSide object_properties;
        public PropHierSide data_properties;
    }
    public static final class IndividualsJson {
        public int schema_version;
        public boolean incomplete;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        /**
         * #610: the KB's consistency stayed undetermined, so a positive answer may be the
         * vacuous one of an inconsistent KB (see docs/json-schema.md for this surface's
         * scope). Absent in older output, false.
         */
        public boolean consistency_undetermined;
        public List<List<String>> same_groups;
        public List<List<String>> different_pairs;
    }
    public static final class PropertyValuesJson {
        public int schema_version;
        public boolean incomplete;
        /**
         * #543 / #610: the ontology uses {@code owl:topObjectProperty} where its universal-role
         * semantics could change an answer, and rustdl reads it as an ordinary property: a
         * negative answer or a missing entailment may be wrong. Absent in older output, false.
         */
        public boolean universal_role_ordinary;
        /**
         * #610: the KB's consistency stayed undetermined, so a positive answer may be the
         * vacuous one of an inconsistent KB (see docs/json-schema.md for this surface's
         * scope). Absent in older output, false.
         */
        public boolean consistency_undetermined;
        public List<List<String>> object_property_values;   // [subj, prop, obj]
        public List<List<String>> data_property_values;     // [subj, prop, lexical, datatype]
    }

    /** `justify --json` top-level payload (docs/json-schema.md). */
    public static final class JustifyJson {
        public int schema_version;
        public String status;               // "entailed" | "not-entailed" | "unknown" (#396)
        public boolean enumeration_complete;
        public boolean minimal;             // true iff every justification is subset-minimal-guaranteed
        public boolean laconic;
        public List<JustificationJson> justifications;
    }
    public static final class JustificationJson {
        public String ofn; // self-contained OWL Functional Syntax ontology document
    }

    /**
     * `prove --json` top-level payload (docs/json-schema.md). Three shapes:
     * <ul>
     * <li>step-level EL proof: {@code entailed=true, has_proof=true, proof} set,
     *     {@code justification_fallback=null}; #594: with {@code --source-fallback}, a proof
     *     this plugin could not show (a builtin or derived axiom, or incomplete) can also
     *     carry {@code justification_fallback}, a source-only justification shown instead;</li>
     * <li>SROIQ-only justification fallback: {@code entailed=true, has_proof=false, proof=null},
     *     {@code justification_fallback} set (a full OFN ontology document, not a single axiom);</li>
     * <li>not entailed: {@code entailed=false, has_proof=false}, both {@code proof} and
     *     {@code justification_fallback} {@code null}.</li>
     * </ul>
     * #402: {@code status} is {@code "entailed" | "not-entailed" | "unknown"}. {@code "unknown"}
     * (rustdl exits 3, so {@link RustdlProcess} already throws) means the reasoner check gave up:
     * {@code entailed=false} is then NOT a non-entailment. {@code incomplete} is set by any
     * give-up, including one in the justification fallback, whose axiom list can then be empty
     * (entailed, but no justification available).
     */
    public static final class ProveJson {
        public int schema_version;
        public boolean entailed;
        public String status;               // "entailed" | "not-entailed" | "unknown" (#402)
        public boolean incomplete;          // a reasoner check gave up (#402)
        public boolean has_proof;
        public ProofNodeJson proof;
        public String justification_fallback; // self-contained OFN ontology document, or null
    }

    /**
     * One node of a {@code prove --json} step-level proof tree. {@code conclusion} is a
     * self-contained OFN ontology document containing exactly one logical axiom (the fact this
     * node proves); each entry of {@code axioms} is likewise its own one-axiom OFN document (the
     * step's cited source axioms -- possibly empty, e.g. a pure transitivity/chain step).
     */
    public static final class ProofNodeJson {
        public String conclusion;
        public String rule;
        public List<String> axioms;
        /**
         * #586: axioms the step used that rustdl derived during conversion from several
         * source axioms, so none of them is in the source. Absent (null) when there are none,
         * and in output from rustdl before the field existed.
         */
        public List<String> derived_axioms;
        /**
         * #539: axioms the step used that rustdl added from the OWL semantics during
         * conversion (e.g. {@code ∃owl:bottomObjectProperty.⊤ ⊑ ⊥}); not source axioms. Absent
         * (null) when there are none.
         */
        public List<String> builtin_axioms;
        public List<ProofNodeJson> premises;
    }
}
