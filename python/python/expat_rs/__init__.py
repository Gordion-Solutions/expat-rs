"""A memory-safe drop-in for the standard library's XML parser.

``expat_rs`` has the API of :mod:`xml.parsers.expat` (CPython's ``pyexpat``
module), running on expat-rs, a parser written in Rust::

    from expat_rs import ParserCreate

    p = ParserCreate()
    p.StartElementHandler = lambda name, attrs: print(name, attrs)
    p.Parse(b"<doc a='1'/>", True)
"""

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
