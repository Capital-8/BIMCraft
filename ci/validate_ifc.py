"""Validate IFC files as CI does: IfcOpenShell's schema check with the EXPRESS rules, then a shape for every product.

Usage: python ci/validate_ifc.py FILE.ifc...  Exits 1 if any file breaks a rule or has a product whose shape won't build.
"""

import logging
import sys

import ifcopenshell
import ifcopenshell.geom
import ifcopenshell.ifcopenshell_wrapper
import ifcopenshell.validate


def check(path):
    problems = []
    f = ifcopenshell.open(path)
    logger = ifcopenshell.validate.json_logger()
    ifcopenshell.validate.validate(f, logger, express_rules=True)
    problems += [f"rule: {s.get('message', s)}" for s in logger.statements]
    settings = ifcopenshell.geom.settings()
    # A product drawn only as a curve (a room separation line's Axis) is built as that curve.
    curves = ifcopenshell.geom.settings()
    curves.set("dimensionality", ifcopenshell.ifcopenshell_wrapper.CURVES_SURFACES_AND_SOLIDS)
    shapes = 0
    for product in f.by_type("IfcProduct"):
        if product.Representation is None:
            continue
        try:
            bodied = any(r.RepresentationIdentifier == "Body" for r in product.Representation.Representations)
            ifcopenshell.geom.create_shape(settings if bodied else curves, product)
            shapes += 1
        except Exception as e:  # IfcOpenShell raises its own RuntimeError subclasses
            problems.append(f"shape of #{product.id()} {product.is_a()} {product.Name!r}: {e}")
    print(f"{path}: {len(f.by_type('IfcProduct'))} products, {shapes} shapes built, {len(problems)} problems")
    for p in problems:
        print(f"  {p}")
    return not problems


if __name__ == "__main__":
    logging.basicConfig(level=logging.WARNING)
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    ok = [check(p) for p in sys.argv[1:]]
    sys.exit(0 if all(ok) else 1)
