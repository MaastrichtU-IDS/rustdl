package com.github.maastrichtu_ids.rustdl.protege;

/** Immutable process and safety bounds for the rustdl Explanation-API adapter. */
public final class RustdlExplainConfiguration {

    public static final int DEFAULT_MAX_JUSTIFICATIONS = 8;
    public static final long DEFAULT_TIMEOUT_SECONDS = 600L;

    private final long timeoutSeconds;
    private final int maxJustifications;

    public RustdlExplainConfiguration(long timeoutSeconds, int maxJustifications) {
        this.timeoutSeconds = requirePositive(timeoutSeconds, "timeoutSeconds");
        this.maxJustifications = requirePositive(maxJustifications, "maxJustifications");
    }

    public static RustdlExplainConfiguration fromSystemProperties() {
        return new RustdlExplainConfiguration(
            longSetting(
                "rustdl.explain.timeout.seconds",
                "RUSTDL_EXPLAIN_TIMEOUT_SECONDS",
                DEFAULT_TIMEOUT_SECONDS),
            intSetting(
                "rustdl.explain.max.justifications",
                "RUSTDL_EXPLAIN_MAX_JUSTIFICATIONS",
                DEFAULT_MAX_JUSTIFICATIONS));
    }

    public long getTimeoutSeconds() {
        return timeoutSeconds;
    }

    public int getMaxJustifications() {
        return maxJustifications;
    }

    private static String setting(String property, String environment, String fallback) {
        String value = System.getProperty(property);
        if (value == null || value.isEmpty()) {
            value = System.getenv(environment);
        }
        return value == null || value.isEmpty() ? fallback : value;
    }

    private static int intSetting(String property, String environment, int fallback) {
        String value = setting(property, environment, Integer.toString(fallback));
        try {
            return requirePositive(Integer.parseInt(value), property);
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException(property + " must be an integer", error);
        }
    }

    private static long longSetting(String property, String environment, long fallback) {
        String value = setting(property, environment, Long.toString(fallback));
        try {
            return requirePositive(Long.parseLong(value), property);
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException(property + " must be an integer", error);
        }
    }

    private static int requirePositive(int value, String name) {
        if (value <= 0) {
            throw new IllegalArgumentException(name + " must be greater than zero");
        }
        return value;
    }

    private static long requirePositive(long value, String name) {
        if (value <= 0) {
            throw new IllegalArgumentException(name + " must be greater than zero");
        }
        return value;
    }
}
