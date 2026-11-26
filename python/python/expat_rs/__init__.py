"""A memory-safe drop-in for the standard library's XML parser.

``expat_rs`` has the API of :mod:`xml.parsers.expat` (CPython's ``pyexpat``
module), running on expat-rs, a parser written in Rust::

    from expat_rs import ParserCreate

    p = ParserCreate()
    p.StartElementHandler = lambda name, attrs: print(name, attrs)
    p.Parse(b"<doc a='1'/>", True)

``expat_rs.install()`` makes the standard library use it: ElementTree
(including its C accelerator), SAX, minidom, pulldom, plistlib, xmlrpc and
anything else that imports ``pyexpat`` or ``xml.parsers.expat``.
"""

import sys

from ._pyexpat import (  # noqa: F401
    EXPAT_VERSION,
    ErrorString,
    ExpatError,
    ParserCreate,
    XML_PARAM_ENTITY_PARSING_ALWAYS,
    XML_PARAM_ENTITY_PARSING_NEVER,
    XML_PARAM_ENTITY_PARSING_UNLESS_STANDALONE,
    XMLParserType,
    error,
    errors,
    features,
    model,
    native_encoding,
    version_info,
)


# Standard library modules that keep the parser they found when imported:
# install() must come before them.
_USERS = (
    "_elementtree",
    "xml.etree.ElementTree",
    "xml.etree.cElementTree",
    "xml.sax.expatreader",
    "xml.dom.expatbuilder",
    "plistlib",
    "xmlrpc.client",
)


def install():
    """Make ``pyexpat`` and ``xml.parsers.expat`` this module.

    Call it before anything imports the standard library's XML modules
    (ElementTree, SAX, minidom, pulldom, plistlib, xmlrpc): it raises
    RuntimeError if one of them is already imported, since that one would
    go on using libexpat. Parsers created earlier, and names imported
    from ``pyexpat`` or ``xml.parsers.expat`` earlier, keep using libexpat.
    Calling it again does nothing.
    """
    from . import _pyexpat

    if sys.modules.get("pyexpat") is _pyexpat:
        return
    early = [name for name in _USERS if name in sys.modules]
    if early:
        raise RuntimeError(
            "expat_rs.install() must be called before these modules are "
            "imported: " + ", ".join(early)
        )
    # _elementtree checks the capsule's magic and libexpat version against
    # the header it was built with: the interpreter's own pyexpat has them.
    try:
        import pyexpat

        real = getattr(pyexpat, "expat_CAPI", None)
        # Before reparse deferral (libexpat 2.6; CPython 3.10.14, 3.11.9,
        # 3.12.3), the standard library can neither turn it off nor flush.
        deferral = hasattr(pyexpat.ParserCreate(), "SetReparseDeferralEnabled")
    except ImportError:
        real = None
        deferral = True
    _pyexpat._set_reparse_deferral_default(deferral)
    _pyexpat.expat_CAPI = _pyexpat._expat_capi(real)
    sys.modules["pyexpat"] = _pyexpat
    sys.modules["pyexpat.errors"] = _pyexpat.errors
    sys.modules["pyexpat.model"] = _pyexpat.model
    # Imported again from the new pyexpat when next needed.
    for name in ("xml.parsers.expat", "xml.parsers.expat.errors", "xml.parsers.expat.model"):
        sys.modules.pop(name, None)
    parsers = sys.modules.get("xml.parsers")
    if parsers is not None and hasattr(parsers, "expat"):
        del parsers.expat
