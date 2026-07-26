package com.github.maastrichtu_ids.rustdl.protege;

import java.util.List;

/** gson-mapped POJOs for the rustdl --json contract (docs/json-schema.md, schema_version 1). */
public final class RustdlJson {
    private RustdlJson() {}

    public static final class ClassifyJson {
        public int schema_version;
        public boolean consistent;
        public boolean incomplete;
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
        public PropHierSide object_properties;
        public PropHierSide data_properties;
    }
    public static final class IndividualsJson {
        public int schema_version;
        public boolean incomplete;
        public List<List<String>> same_groups;
        public List<List<String>> different_pairs;
    }
    public static final class PropertyValuesJson {
        public int schema_version;
        public boolean incomplete;
        public List<List<String>> object_property_values;   // [subj, prop, obj]
        public List<List<String>> data_property_values;     // [subj, prop, lexical, datatype]
    }

    /** `justify --json` top-level payload (docs/json-schema.md). */
    public static final class JustifyJson {
        public int schema_version;
        public String status;               // "entailed" | "not-entailed"
        public boolean enumeration_complete;
        public boolean minimal;             // true iff every justification is subset-minimal-guaranteed
        public boolean laconic;
        public List<JustificationJson> justifications;
    }
    public static final class JustificationJson {
        public String ofn; // self-contained OWL Functional Syntax ontology document
    }

    /**
     * `prove --json` top-level payload (docs/json-schema.md). Three mutually exclusive shapes:
     * <ul>
     * <li>step-level EL proof: {@code entailed=true, has_proof=true, proof} set,
     *     {@code justification_fallback=null};</li>
     * <li>SROIQ-only justification fallback: {@code entailed=true, has_proof=false, proof=null},
     *     {@code justification_fallback} set (a full OFN ontology document, not a single axiom);</li>
     * <li>not entailed: {@code entailed=false, has_proof=false}, both {@code proof} and
     *     {@code justification_fallback} {@code null}.</li>
     * </ul>
     */
    public static final class ProveJson {
        public int schema_version;
        public boolean entailed;
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
        public List<ProofNodeJson> premises;
    }
}
