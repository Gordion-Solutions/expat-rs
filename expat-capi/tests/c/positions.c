/* Positions handlers see in the DOCTYPE, and where parsing stopped by
   XML_StopParser reports itself. libexpat reports each DTD declaration at
   the token that completes it (an entity's value, an ATTLIST default, the
   DOCTYPE's `[`), and a stop before the root element where the handler
   was, but one in or after it at the end of the token. Built against
   libexpat and against expat-capi the outputs must be identical:
   tests/c/positions.expected is libexpat 2.8.2's output (regenerate with
   tools/regen_expected.sh), checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;
static const char *target;

static void at(const char *what) {
  printf("  %-16s @%lu:%lu byte=%ld\n", what, XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p),
         (long)XML_GetCurrentByteIndex(p));
  if (target && strcmp(what, target) == 0) XML_StopParser(p, XML_FALSE);
}
static void sdoctype(void *u, const XML_Char *n, const XML_Char *sys, const XML_Char *pub, int internal) {
  at(internal ? "doctype+subset" : "doctype");
}
static void edoctype(void *u) { at("end-doctype"); }
static void entity(void *u, const XML_Char *n, int pe, const XML_Char *v, int vlen, const XML_Char *base,
                   const XML_Char *sys, const XML_Char *pub, const XML_Char *notation) {
  char what[64];
  snprintf(what, sizeof what, "entity %s%s", pe ? "%" : "", n);
  at(what);
}
static void notation(void *u, const XML_Char *n, const XML_Char *base, const XML_Char *sys, const XML_Char *pub) {
  char what[64];
  snprintf(what, sizeof what, "notation %s", n);
  at(what);
}
static void attlist(void *u, const XML_Char *el, const XML_Char *att, const XML_Char *type, const XML_Char *dflt,
                    int required) {
  char what[64];
  snprintf(what, sizeof what, "attlist %s", att);
  at(what);
}
static void element(void *u, const XML_Char *n, XML_Content *model) {
  char what[64];
  snprintf(what, sizeof what, "element %s", n);
  at(what);
  XML_FreeContentModel(p, model);
}
static void pi(void *u, const XML_Char *t, const XML_Char *d) { at(t); }
static void comment(void *u, const XML_Char *d) { at(d); }
static void xmldecl(void *u, const XML_Char *v, const XML_Char *e, int sa) { at("xmldecl"); }
static void start(void *u, const XML_Char *n, const XML_Char **a) { at(n); }
static void end(void *u, const XML_Char *n) {
  char what[64];
  snprintf(what, sizeof what, "/%s", n);
  at(what);
}

static void run(const char *label, const char *doc, int piece) {
  printf("== %s%s%s\n", label, target ? ", stopping at " : "", target ? target : "");
  p = XML_ParserCreate(NULL);
  XML_SetDoctypeDeclHandler(p, sdoctype, edoctype);
  XML_SetEntityDeclHandler(p, entity);
  XML_SetNotationDeclHandler(p, notation);
  XML_SetAttlistDeclHandler(p, attlist);
  XML_SetElementDeclHandler(p, element);
  XML_SetProcessingInstructionHandler(p, pi);
  XML_SetCommentHandler(p, comment);
  XML_SetXmlDeclHandler(p, xmldecl);
  XML_SetElementHandler(p, start, end);
  size_t len = strlen(doc);
  enum XML_Status st = XML_STATUS_OK;
  for (size_t off = 0; off < len && st == XML_STATUS_OK; off += piece) {
    st = XML_Parse(p, doc + off, (int)(len - off < (size_t)piece ? len - off : piece), XML_FALSE);
  }
  if (st == XML_STATUS_OK) st = XML_Parse(p, NULL, 0, XML_TRUE);
  if (st == XML_STATUS_ERROR) {
    printf("  -> %s @%lu:%lu byte=%ld\n", XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
           XML_GetCurrentColumnNumber(p), (long)XML_GetCurrentByteIndex(p));
  }
  XML_ParserFree(p);
}

int main(void) {
  const char *dtd =
      "<?xml version='1.0'?>\n"
      "<!DOCTYPE r [\n"
      "<!ENTITY i 'v'>\n"
      "<!ENTITY  e   SYSTEM  'e.xml' >\n"
      "<!ENTITY p PUBLIC '-//p' 'p.xml'>\n"
      "<!ENTITY u SYSTEM 'u.gif' NDATA gif>\n"
      "<!ENTITY % pi 'x'>\n"
      "<!ENTITY % pe SYSTEM 'pe.dtd'>\n"
      "<!NOTATION gif SYSTEM 'image/gif'>\n"
      "<!NOTATION n2 PUBLIC '-//n2'>\n"
      "<!NOTATION n3 PUBLIC '-//n3' 'n3.sys'>\n"
      "<!ELEMENT r (a|b)*>\n"
      "<!ELEMENT a EMPTY>\n"
      "<!ELEMENT b (#PCDATA)>\n"
      "<!ELEMENT c (#PCDATA|a)*>\n"
      "<!ELEMENT d ((a,b)|c)+>\n"
      "<!ELEMENT e ANY>\n"
      "<!ATTLIST r x CDATA #IMPLIED y (p|q) 'p' z CDATA #FIXED 'f'\n"
      "            w NOTATION (gif|n2) #REQUIRED>\n"
      "<?t data?>\n"
      "<!-- c -->\n"
      "]>\n"
      "<r/>\n";
  run("declarations", dtd, 4096);
  run("declarations in pieces of 3", dtd, 3);
  run("doctype system", "<!DOCTYPE r SYSTEM 'r.dtd'>\n<r/>", 4096);
  run("doctype public", "<!DOCTYPE r PUBLIC '-//r' 'r.dtd' >\n<r/>", 4096);
  run("doctype plain", "<!DOCTYPE r>\n<r/>", 4096);
  run("doctype system and subset", "<!DOCTYPE r SYSTEM 'r.dtd' [ <!ENTITY i 'v'> ] >\n<r/>", 4096);
  run("multi-line declarations", "<!DOCTYPE r\n  [\n<!ENTITY\n  i\n  'v'\n>\n<!ATTLIST r\n  x\n  CDATA\n  #IMPLIED>\n]\n>\n<r/>", 4096);

  const char *doc =
      "<?xml version='1.0'?>\n<?pro x?><!--c1-->\n<!DOCTYPE r [\n<!ENTITY e 'v'>\n<?dtdpi x?>\n]>\n"
      "<r a='1'><?inpi x?><!--c2-->text<e/></r>\n<?epi x?><!--c3-->\n";
  const char *stops[] = {"xmldecl", "pro", "c1", "doctype+subset", "entity e", "dtdpi", "end-doctype",
                         "r", "inpi", "c2", "e", "/e", "/r", "epi", "c3"};
  for (int i = 0; i < 15; i++) {
    target = stops[i];
    run("stop", doc, 4096);
  }
  /* Within an entity's replacement text a stop stays at the reference. */
  const char *ent =
      "<!DOCTYPE r [<!ENTITY e 'a<x><?ep d?><!--ec--></x>b'><!ENTITY f '&e;'>]>\n"
      "<r>&amp;&#65;<y/>&e;<z/>&f;</r>\n";
  const char *entity_stops[] = {"y", "x", "ep", "ec", "/x", "z", "/y", "/z", "/r"};
  for (int i = 0; i < 9; i++) {
    target = entity_stops[i];
    run("stop in an entity", ent, 4096);
  }
  target = NULL;

  /* A first chunk shorter than four bytes is parsed at once. */
  const char *shorts[] = {"<a>", "<a", "<", "\xEF\xBB", "<?x"};
  for (int i = 0; i < 5; i++) {
    printf("== first chunk of %d bytes\n", (int)strlen(shorts[i]));
    p = XML_ParserCreate(NULL);
    XML_SetElementHandler(p, start, end);
    XML_Parse(p, shorts[i], (int)strlen(shorts[i]), XML_FALSE);
    printf("  after it @%lu:%lu byte=%ld\n", XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p),
           (long)XML_GetCurrentByteIndex(p));
    XML_ParserFree(p);
  }
  return 0;
}
