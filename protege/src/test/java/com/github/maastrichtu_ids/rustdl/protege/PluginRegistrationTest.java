package com.github.maastrichtu_ids.rustdl.protege;

import org.junit.Test;
import org.semanticweb.owl.explanation.api.ExplanationGeneratorFactory;
import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.NodeList;
import org.xml.sax.ErrorHandler;
import org.xml.sax.SAXParseException;

import javax.xml.parsers.DocumentBuilder;
import javax.xml.parsers.DocumentBuilderFactory;
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

    /**
     * Fold-in M4.1 (Task 4 review): catches a proof-service registration regression at
     * {@code mvn test} speed rather than only at the antrun {@code verify}-phase
     * {@code resourcecontains} check (protege/pom.xml's {@code verify-bundle-contents}
     * execution), which only runs on a full {@code mvn verify}.
     */
    @Test public void pluginXmlRegistersRustdlProofService() throws Exception {
        try (InputStream in = getClass().getResourceAsStream("/plugin.xml")) {
            assertNotNull("plugin.xml must be on the classpath", in);
            String xml = new Scanner(in, "UTF-8").useDelimiter("\\A").next();
            assertTrue("plugin.xml must register the proof-service extension point",
                xml.contains("org.liveontologies.protege.explanation.proof.service"));
            assertTrue(
                "plugin.xml must wire that extension point to RustdlProofService",
                xml.contains(
                    "<class value=\"com.github.maastrichtu_ids.rustdl.protege.RustdlProofService\""));
        }
    }

    /**
     * plugin.xml must be WELL-FORMED XML, not merely contain the right substrings.
     *
     * Issue #79: a `--` inside an XML comment (from the literal `rustdl prove --json`) is
     * illegal XML. Protege rejected the whole file with "Could not parse XML contribution",
     * which silently unregistered the reasoner extension too - the file's FIRST element, and
     * the one every other test here asserts on. It shipped in 22 releases, v0.4.5 through
     * v0.4.26.
     *
     * Every other assertion in this class passed throughout, because they all read the file
     * as a String and call contains(). A substring check cannot see malformed XML; only a
     * parser can. That is the whole reason this test exists, so do not weaken it back into
     * a contains() check.
     *
     * It parses the resource off the CLASSPATH rather than the source tree, so it also
     * covers the case where Maven resource filtering itself produces something unparseable.
     */
    @Test public void pluginXmlIsWellFormedXml() throws Exception {
        Document doc = parsePluginXml();
        assertEquals("root element must be <plugin>", "plugin", doc.getDocumentElement().getTagName());
    }

    /**
     * Both extension points must survive PARSING, not just appear in the text.
     *
     * The sibling contains() tests above cannot distinguish "registered" from "present in a
     * file Protege discards", which is exactly the state #79 shipped in.
     */
    @Test public void bothExtensionPointsSurviveParsing() throws Exception {
        Document doc = parsePluginXml();
        NodeList extensions = doc.getElementsByTagName("extension");
        List<String> points = new ArrayList<>();
        for (int i = 0; i < extensions.getLength(); i++) {
            points.add(((Element) extensions.item(i)).getAttribute("point"));
        }
        assertTrue("reasoner factory extension point must parse: " + points,
            points.contains("org.protege.editor.owl.inference_reasonerfactory"));
        assertTrue("proof-service extension point must parse: " + points,
            points.contains("org.liveontologies.protege.explanation.proof.service"));
    }

    /** Parse /plugin.xml off the classpath, failing the test on any parse error or warning. */
    private Document parsePluginXml() throws Exception {
        DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
        factory.setNamespaceAware(true);
        // Do not fetch anything over the network for a DTD/entity; a build must not depend
        // on remote availability, and plugin.xml declares no external entities.
        factory.setFeature("http://apache.org/xml/features/nonvalidating/load-external-dtd", false);
        DocumentBuilder builder = factory.newDocumentBuilder();
        builder.setErrorHandler(new ErrorHandler() {
            @Override public void warning(SAXParseException e) throws SAXParseException { throw e; }
            @Override public void error(SAXParseException e) throws SAXParseException { throw e; }
            @Override public void fatalError(SAXParseException e) throws SAXParseException { throw e; }
        });
        try (InputStream in = getClass().getResourceAsStream("/plugin.xml")) {
            assertNotNull("plugin.xml must be on the classpath", in);
            try {
                return builder.parse(in);
            } catch (SAXParseException e) {
                fail("plugin.xml is not well-formed XML, so Protege will reject the WHOLE file "
                    + "and register nothing (issue #79). Line " + e.getLineNumber() + ", column "
                    + e.getColumnNumber() + ": " + e.getMessage()
                    + "  --  the usual cause is a double hyphen inside an XML comment.");
                throw e; // unreachable; keeps the compiler happy about the return type
            }
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
