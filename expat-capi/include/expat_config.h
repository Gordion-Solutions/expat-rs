/* expat_config.h for expat-capi.

   libexpat installs a generated expat_config.h describing how it was
   built, and some consumers (CPython's pyexpat among them) include it
   before expat.h. This one describes expat-capi: the features below are
   what expat.h consults to decide which functions to declare. */

#ifndef EXPAT_CONFIG_H
#define EXPAT_CONFIG_H 1

/* DTD processing, including parameter entities and the
   billion-laughs protection settings. */
#define XML_DTD 1

/* General entities are expanded (and their amplification limited). */
#define XML_GE 1

/* Namespace processing (XML_ParserCreateNS). */
#define XML_NS 1

/* No input context is kept: XML_GetInputContext returns NULL. */
#define XML_CONTEXT_BYTES 0

#endif /* EXPAT_CONFIG_H */
