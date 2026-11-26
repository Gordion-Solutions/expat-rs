/* Namespace URIs containing the namespace separator: libexpat (2.4.5 and
   later, CVE-2022-25236) rejects them as a syntax error at the start tag,
   unless the separator is a character URIs may contain. ElementTree's
   separator is '}', minidom's ' '. Built against libexpat and against
   expat-capi the outputs must be identical: tests/c/ns_separator.expected
   is libexpat 2.8.2's output (regenerate with tools/regen_expected.sh),
   checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;

static void start(void *u, const XML_Char *n, const XML_Char **a) { printf("  <%s>\n", n); }
static void ns(void *u, const XML_Char *prefix, const XML_Char *uri) {
  printf("  xmlns:%s=%s\n", prefix ? prefix : "(none)", uri ? uri : "(none)");
}

static const struct { char sep; const char *doc; } cases[] = {
    {' ', "<e xmlns:a=\"x y\"><a:f/></e>"},
    {' ', "<e xmlns=\"x y\"/>"},
    {' ', "<e xmlns:b=\"ok\" xmlns:a=\"x y\"/>"},
    {' ', "<e>\n<f xmlns:a=\"u\">t</f><g xmlns=\"p q\"/></e>"},
    {'}', "<e xmlns=\"x}y\"/>"},
    {'}', "<e xmlns=\"x{y\"/>"},
    {'!', "<e xmlns:a=\"x!y\"/>"},
    {':', "<e xmlns:a=\"x:y\"/>"},
    {'#', "<e xmlns:a=\"x#y\"/>"},
    {'\n', "<e xmlns:a=\"x&#10;y\"/>"},
    {' ', "<e xmlns:a=\"x&#32;y\"/>"},
    {' ', "<!DOCTYPE e [<!ATTLIST e xmlns:a CDATA \"x y\">]><e/>"},
    {' ', "<!DOCTYPE e [<!ENTITY t \"<f xmlns='a b'/>\">]><e>&t;</e>"},
    {'|', "<e xmlns:a=\"x|y\"><a:f/></e>"},
};

int main(void) {
  for (size_t i = 0; i < sizeof cases / sizeof *cases; i++) {
    for (int triplets = 0; triplets <= 1; triplets++) {
      printf("case %zu, triplets %d\n", i, triplets);
      p = XML_ParserCreateNS(NULL, cases[i].sep);
      XML_SetReturnNSTriplet(p, triplets);
      XML_SetStartElementHandler(p, start);
      XML_SetStartNamespaceDeclHandler(p, ns);
      if (XML_Parse(p, cases[i].doc, (int)strlen(cases[i].doc), 1) == XML_STATUS_ERROR)
        printf("  -> %s @%lu:%lu byte %ld\n", XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
               XML_GetCurrentColumnNumber(p), XML_GetCurrentByteIndex(p));
      XML_ParserFree(p);
    }
  }
  return 0;
}
