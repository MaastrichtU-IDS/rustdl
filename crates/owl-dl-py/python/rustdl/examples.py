"""Bundled example ontologies for trying rustdl with no network access.

The pizza ontology (https://github.com/owlcs/pizza-ontology) ships inside
the wheel, so `rustdl.examples.pizza()` works offline. It is a small,
classic OWL 2 DL teaching ontology — a good first thing to classify.
"""

from importlib import resources

# Namespace of the bundled pizza ontology. Class IRIs are this prefix
# plus the local name, e.g. PIZZA_NS + "Margherita".
PIZZA_NS = (
    "https://raw.githubusercontent.com/owlcs/pizza-ontology/"
    "refs/heads/master/pizza.owl#"
)


def pizza() -> str:
    """Filesystem path to the bundled pizza ontology (RDF/XML, `.owl`).

    Pass it straight to `rustdl.classify`:

        >>> import rustdl
        >>> result = rustdl.classify(rustdl.examples.pizza())
        >>> result.is_subclass(
        ...     rustdl.examples.PIZZA_NS + "Margherita",
        ...     rustdl.examples.PIZZA_NS + "Pizza",
        ... )
        True
    """
    return str(resources.files("rustdl").joinpath("data", "pizza.owl"))


__all__ = ["PIZZA_NS", "pizza"]
