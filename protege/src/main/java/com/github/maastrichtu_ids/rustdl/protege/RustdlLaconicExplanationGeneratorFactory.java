package com.github.maastrichtu_ids.rustdl.protege;

/**
 * As {@link RustdlExplanationGeneratorFactory}, but requests LACONIC justifications from
 * {@code rustdl justify --laconic}: each justification axiom is weakened to the fragment
 * actually responsible for the entailment (sound structural weakening; see
 * {@code docs/superpowers/specs/2026-06-21-laconic-justifications-design.md}). Registered
 * under its own {@code META-INF/services} entry alongside the non-laconic factory so both
 * appear as distinct choices in Protégé's Explanation ("?") dialog.
 */
public final class RustdlLaconicExplanationGeneratorFactory extends RustdlExplanationGeneratorFactory {

    public RustdlLaconicExplanationGeneratorFactory() {
        super();
    }

    public RustdlLaconicExplanationGeneratorFactory(RustdlExplainConfiguration configuration) {
        super(configuration);
    }

    @Override
    public String getExplanationGeneratorName() {
        return "rustdl (laconic)";
    }

    @Override
    boolean isLaconic() {
        return true;
    }
}
