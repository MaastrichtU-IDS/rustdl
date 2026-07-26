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
}
