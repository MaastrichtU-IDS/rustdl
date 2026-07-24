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
}
