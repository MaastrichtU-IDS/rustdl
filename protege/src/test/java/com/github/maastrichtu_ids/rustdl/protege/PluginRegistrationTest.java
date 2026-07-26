package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory;

import java.io.InputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Scanner;
import java.util.ServiceLoader;
import static org.junit.Assert.*;

public class PluginRegistrationTest {
    @Test public void pluginXmlRegistersRustdl() throws Exception {
        try (InputStream in = getClass().getResourceAsStream("/plugin.xml")) {
            assertNotNull("plugin.xml must be on the classpath", in);
            String xml = new Scanner(in, "UTF-8").useDelimiter("\\A").next();
            assertTrue(xml.contains("org.protege.editor.owl.inference_reasonerfactory"));
            // Protégé reads <name value=.../> and <class value=.../> child elements
            // (as ELK/HermiT do), not name=/factoryClass= attributes.
            // The reasoner-menu name carries the version ("rustdl <version>"),
            // injected by Maven resource filtering of ${project.version}.
            assertTrue("menu name should be 'rustdl <version>'", xml.contains("<name value=\"rustdl "));
            assertFalse("resource filtering must substitute ${project.version}",
                xml.contains("${project.version}"));
            assertTrue(xml.contains("<class value=\"com.github.maastrichtu_ids.rustdl.protege.RustdlReasonerInfo\""));
        }
    }

    @Test public void servicesFileListsBothExplanationFactories() throws Exception {
        String resource = "/META-INF/services/org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory";
        try (InputStream in = getClass().getResourceAsStream(resource)) {
            assertNotNull(resource + " must be on the classpath", in);
            String contents = new Scanner(in, "UTF-8").useDelimiter("\\A").next();
            assertTrue(contents.contains(
                "com.github.maastrichtu_ids.rustdl.protege.RustdlExplanationGeneratorFactory"));
            assertTrue(contents.contains(
                "com.github.maastrichtu_ids.rustdl.protege.RustdlLaconicExplanationGeneratorFactory"));
        }
    }

    @Test public void serviceLoaderDiscoversBothExplanationFactories() {
        List<String> names = new ArrayList<>();
        for (ExplanationGeneratorFactory<?> factory
                : ServiceLoader.load(ExplanationGeneratorFactory.class, getClass().getClassLoader())) {
            names.add(factory.getClass().getName());
        }
        assertTrue("ServiceLoader must discover RustdlExplanationGeneratorFactory: " + names,
            names.contains("com.github.maastrichtu_ids.rustdl.protege.RustdlExplanationGeneratorFactory"));
        assertTrue("ServiceLoader must discover RustdlLaconicExplanationGeneratorFactory: " + names,
            names.contains("com.github.maastrichtu_ids.rustdl.protege.RustdlLaconicExplanationGeneratorFactory"));
    }
}
