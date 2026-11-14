/* XML_ParserReset: the parser is as new (handlers, user data, settings,
   position and error cleared; the new encoding used), it keeps its
   namespace processing, an unknown encoding's data is released, and a
   parser from XML_ExternalEntityParserCreate can't be reset. Built
   against libexpat and against expat-capi the outputs must be identical:
   tests/c/reset.expected is libexpat 2.8.2's output (regenerate with
   tools/regen_expected.sh), checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;
static int releases;

static void state(const char *when) {
  printf("  %s: error=%d @%lu:%lu byte=%ld userdata=%s base=%s\n", when, XML_GetErrorCode(p),
         XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p), (long)XML_GetCurrentByteIndex(p),
         XML_GetUserData(p) ? (const char *)XML_GetUserData(p) : "(null)", XML_GetBase(p) ? XML_GetBase(p) : "(null)");
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("    <%s> user=%s\n", n, u ? (const char *)u : "(null)");
}
static void end(void *u, const XML_Char *n) { printf("    </%s>\n", n); }
static void text(void *u, const XML_Char *t, int len) { printf("    text [%.*s]\n", len, t); }
static void decl(void *u, const XML_Char *v, const XML_Char *e, int sa) {
  printf("    xmldecl %s %s\n", v ? v : "-", e ? e : "-");
}
static int ext(XML_Parser parser, const XML_Char *context, const XML_Char *base, const XML_Char *sys,
               const XML_Char *pub) {
  XML_Parser child = XML_ExternalEntityParserCreate(parser, context, NULL);
  printf("    external entity: child reset -> %d\n", XML_ParserReset(child, NULL));
  XML_ParserFree(child);
  return XML_STATUS_OK;
}
static void rel(void *data) { releases++; printf("    release\n"); }
static int unknown(void *u, const XML_Char *name, XML_Encoding *info) {
  printf("    unknown-encoding handler (data %s)\n", u ? (const char *)u : "(null)");
  for (int i = 0; i < 256; i++) info->map[i] = i < 0x80 ? i : 0x400 + i;
  info->data = NULL;
  info->convert = NULL;
  info->release = rel;
  return XML_STATUS_OK;
}

static void parse(const char *label, const char *doc) {
  enum XML_Status st = XML_Parse(p, doc, (int)strlen(doc), XML_TRUE);
  printf("  parse %s -> %d\n", label, st);
}
static void handlers(void) {
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  XML_SetXmlDeclHandler(p, decl);
}

int main(void) {
  printf("== reset after a document, then another\n");
  p = XML_ParserCreate(NULL);
  handlers();
  XML_SetUserData(p, "u1");
  XML_SetBase(p, "base1");
  parse("first", "<?xml version='1.0'?><a>x</a>");
  state("after");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  state("reset");
  parse("without handlers", "<b>y</b>");
  state("after");
  XML_ParserReset(p, NULL);
  handlers();
  parse("with handlers again", "<c>z</c>");
  XML_ParserFree(p);

  printf("== reset after an error\n");
  p = XML_ParserCreate(NULL);
  handlers();
  parse("bad", "<a><b></a>");
  state("after");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  state("reset");
  handlers();
  parse("good", "<a/>");
  state("after");
  XML_ParserFree(p);

  printf("== reset in the middle of a document\n");
  p = XML_ParserCreate(NULL);
  handlers();
  XML_Parse(p, "<a>some", 7, XML_FALSE);
  state("partway");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("new", "<n>new</n>");
  state("after");
  XML_ParserFree(p);

  printf("== reset after a stop\n");
  p = XML_ParserCreate(NULL);
  XML_Parse(p, "<a>", 3, XML_FALSE);
  printf("  stop -> %d\n", XML_StopParser(p, XML_FALSE));
  parse("more", "</a>");
  state("stopped");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("new", "<s/>");
  XML_ParserFree(p);

  printf("== reset with an encoding\n");
  p = XML_ParserCreate("UTF-8");
  printf("  reset -> %d\n", XML_ParserReset(p, "ISO-8859-1"));
  handlers();
  parse("latin-1", "<l>caf\xe9</l>");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("declared latin-1", "<?xml version='1.0' encoding='iso-8859-1'?><l>caf\xe9</l>");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("now UTF-8 again", "<l>caf\xc3\xa9</l>");
  XML_ParserFree(p);

  printf("== a namespace parser stays one\n");
  p = XML_ParserCreateNS(NULL, '|');
  handlers();
  parse("first", "<x:a xmlns:x='urn:x'/>");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("second", "<y:b xmlns:y='urn:y'/>");
  XML_ParserFree(p);

  printf("== unknown encoding data is released\n");
  releases = 0;
  p = XML_ParserCreate(NULL);
  XML_SetUnknownEncodingHandler(p, unknown, "hd");
  handlers();
  parse("x-enc", "<?xml version='1.0' encoding='x-enc'?><e>\xc1</e>");
  printf("  releases before reset: %d\n", releases);
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  printf("  releases after reset: %d\n", releases);
  handlers();
  parse("x-enc without a handler", "<?xml version='1.0' encoding='x-enc'?><e/>");
  XML_ParserFree(p);
  printf("  releases after free: %d\n", releases);

  printf("== settings are cleared\n");
  p = XML_ParserCreate(NULL);
  XML_SetParamEntityParsing(p, XML_PARAM_ENTITY_PARSING_ALWAYS);
  XML_SetReturnNSTriplet(p, 1);
  XML_SetExternalEntityRefHandler(p, ext);
  parse("with an external entity", "<!DOCTYPE r [<!ENTITY e SYSTEM 'e.xml'>]><r>&e;</r>");
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  handlers();
  parse("the same, no handler now", "<!DOCTYPE r [<!ENTITY e SYSTEM 'e.xml'>]><r>&e;</r>");
  XML_ParserFree(p);

  printf("== the foreign DTD setting\n");
  p = XML_ParserCreate(NULL);
  XML_UseForeignDTD(p, XML_TRUE);
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  XML_SetParamEntityParsing(p, XML_PARAM_ENTITY_PARSING_ALWAYS);
  XML_SetExternalEntityRefHandler(p, ext);
  parse("no DOCTYPE", "<r/>");
  XML_ParserFree(p);

  printf("== the amplification limit\n");
  for (int reset = 0; reset < 2; reset++) {
    p = XML_ParserCreate(NULL);
    XML_SetBillionLaughsAttackProtectionMaximumAmplification(p, 1.0f);
    XML_SetBillionLaughsAttackProtectionActivationThreshold(p, 1);
    if (reset) printf("  reset -> %d\n", XML_ParserReset(p, NULL));
    parse(reset ? "entities after a reset" : "entities",
          "<!DOCTYPE r [<!ENTITY a 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'><!ENTITY b '&a;&a;&a;&a;&a;&a;&a;&a;'>]>"
          "<r>&b;&b;&b;&b;&b;&b;&b;&b;</r>");
    if (XML_GetErrorCode(p)) printf("  -> %s\n", XML_ErrorString(XML_GetErrorCode(p)));
    XML_ParserFree(p);
  }

  printf("== a fresh parser, reset at once\n");
  p = XML_ParserCreate(NULL);
  printf("  reset -> %d\n", XML_ParserReset(p, NULL));
  state("reset");
  handlers();
  parse("doc", "<f/>");
  XML_ParserFree(p);
  return 0;
}
