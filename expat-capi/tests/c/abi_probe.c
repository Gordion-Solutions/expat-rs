/* Exercises the libexpat API the way CPython's pyexpat does and logs every
   callback. Built against libexpat and against expat-capi, the outputs must
   be identical: tests/c/abi_probe.expected is libexpat 2.8.2's output
   (regenerate with tools/regen_expected.sh) and tests/abi.rs checks
   expat-capi reproduces it. Behaviour not yet matched is listed in
   expat-capi/README.md, not tested here. */
#include <expat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static XML_Parser p;
static int mallocs, frees;

static void pos(void) {
  printf("    @%lu:%lu byte=%ld\n", XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p),
         (long)XML_GetCurrentByteIndex(p));
}
static const char *s(const XML_Char *x) { return x ? x : "(null)"; }

static void start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("start %s ud=%s spec=%d", n, (const char *)u, XML_GetSpecifiedAttributeCount(p));
  for (int i = 0; a[i]; i += 2) printf(" [%s=%s]", a[i], a[i + 1]);
  printf("\n");
  pos();
}
static void end(void *u, const XML_Char *n) { printf("end %s\n", n); pos(); }
static void text(void *u, const XML_Char *t, int len) { printf("text [%.*s]\n", len, t); }
static void pi(void *u, const XML_Char *t, const XML_Char *d) { printf("pi %s [%s]\n", t, d); pos(); }
static void comment(void *u, const XML_Char *d) { printf("comment [%s]\n", d); pos(); }
static void scdata(void *u) { printf("start-cdata\n"); }
static void ecdata(void *u) { printf("end-cdata\n"); }
static void xmldecl(void *u, const XML_Char *v, const XML_Char *e, int sa) {
  printf("xmldecl %s %s %d\n", s(v), s(e), sa);
}
static void sdoctype(void *u, const XML_Char *n, const XML_Char *sys, const XML_Char *pub, int has) {
  printf("doctype %s sys=%s pub=%s internal=%d\n", n, s(sys), s(pub), has);
}
static void edoctype(void *u) { printf("end-doctype\n"); }
static void entity(void *u, const XML_Char *n, int pe, const XML_Char *v, int vlen, const XML_Char *base,
                   const XML_Char *sys, const XML_Char *pub, const XML_Char *notation) {
  printf("entity %s pe=%d value=", n, pe);
  if (v) printf("[%.*s]", vlen, v); else printf("(null)");
  printf(" base=%s sys=%s pub=%s ndata=%s\n", s(base), s(sys), s(pub), s(notation));
}
static void notation(void *u, const XML_Char *n, const XML_Char *base, const XML_Char *sys, const XML_Char *pub) {
  printf("notation %s base=%s sys=%s pub=%s\n", n, s(base), s(sys), s(pub));
}
static void attlist(void *u, const XML_Char *el, const XML_Char *at, const XML_Char *ty, const XML_Char *dflt,
                    int req) {
  printf("attlist %s %s %s %s %d\n", el, at, ty, s(dflt), req);
}
static void model(const XML_Content *c) {
  static const char *ty[] = {"?", "EMPTY", "ANY", "MIXED", "NAME", "CHOICE", "SEQ"};
  static const char *q[] = {"", "?", "*", "+"};
  printf("%s", ty[c->type]);
  if (c->name) printf(":%s", c->name);
  if (c->numchildren) {
    printf("(");
    for (unsigned i = 0; i < c->numchildren; i++) { if (i) printf(" "); model(&c->children[i]); }
    printf(")");
  }
  printf("%s", q[c->quant]);
}
static void element(void *u, const XML_Char *n, XML_Content *c) {
  printf("element %s ", n); model(c); printf("\n");
  XML_FreeContentModel(p, c);
}
static void sns(void *u, const XML_Char *pre, const XML_Char *uri) { printf("start-ns %s %s\n", s(pre), s(uri)); }
static void ens(void *u, const XML_Char *pre) { printf("end-ns %s\n", s(pre)); }
static void dflt(void *u, const XML_Char *t, int len) { printf("default [%.*s]\n", len, t); }

static void *my_malloc(size_t n) { mallocs++; return malloc(n); }
static void *my_realloc(void *x, size_t n) { return realloc(x, n); }
static void my_free(void *x) { if (x) frees++; free(x); }

static void all_handlers(void) {
  XML_SetUserData(p, (void *)"U");
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  XML_SetProcessingInstructionHandler(p, pi);
  XML_SetCommentHandler(p, comment);
  XML_SetStartCdataSectionHandler(p, scdata);
  XML_SetEndCdataSectionHandler(p, ecdata);
  XML_SetXmlDeclHandler(p, xmldecl);
  XML_SetStartDoctypeDeclHandler(p, sdoctype);
  XML_SetEndDoctypeDeclHandler(p, edoctype);
  XML_SetEntityDeclHandler(p, entity);
  XML_SetNotationDeclHandler(p, notation);
  XML_SetAttlistDeclHandler(p, attlist);
  XML_SetElementDeclHandler(p, element);
  XML_SetNamespaceDeclHandler(p, sns, ens);
}

/* Only character data: for documents whose positions or declaration
   order aren't compared yet (non-UTF-8 byte indexes; see README.md). */
static void text_only(void) {
  XML_SetUserData(p, (void *)"U");
  XML_SetCharacterDataHandler(p, text);
}

static void report(int ok) {
  if (ok) printf("OK\n");
  else printf("ERROR %d '%s' @%lu:%lu\n", XML_GetErrorCode(p), XML_ErrorString(XML_GetErrorCode(p)),
              XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p));
}

static void parse_whole(const char *label, const char *doc) {
  printf("=== %s\n", label);
  report(XML_Parse(p, doc, (int)strlen(doc), 1) == XML_STATUS_OK);
  XML_ParserFree(p);
}

/* XML_StopParser from inside a handler, as pyexpat does on an exception. */
static void stopping_start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("start %s\n", n);
  if (!strcmp(n, "stop")) printf("stop -> %d\n", XML_StopParser(p, XML_FALSE));
}
/* XML_Parse re-entered from a handler must fail, not corrupt the parser. */
static void reentrant_start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("start %s; nested XML_Parse -> %d\n", n, XML_Parse(p, "<x/>", 4, 0));
}

static int latin2_ish(void *data, const XML_Char *name, XML_Encoding *info) {
  printf("unknown-encoding %s data=%s\n", name, (const char *)data);
  for (int i = 0; i < 256; i++) info->map[i] = i;
  info->map[0xA1] = 0x0104; /* Ą */
  info->map[0xFF] = -1;     /* invalid */
  info->data = NULL; info->convert = NULL; info->release = NULL;
  return XML_STATUS_OK;
}
static int refuse_encoding(void *data, const XML_Char *name, XML_Encoding *info) { return XML_STATUS_ERROR; }

int main(void) {
  setvbuf(stdout, NULL, _IOLBF, 0);
  p = XML_ParserCreate(NULL); all_handlers();
  parse_whole("all callbacks",
              "<?xml version='1.0' encoding='utf-8' standalone='no'?>\n"
              "<!DOCTYPE doc PUBLIC '-//P//EN' 'doc.dtd' [\n"
              "  <!ELEMENT doc (a, (b | c)*, d?)>\n"
              "  <!ELEMENT a (#PCDATA | b)*>\n"
              "  <!ELEMENT e EMPTY>\n"
              "  <!ATTLIST doc x CDATA #IMPLIED y (p|q) 'p' z CDATA #FIXED 'zz'>\n"
              "  <!ENTITY ge 'general &#38;amp; text'>\n"
              "  <!ENTITY % pe 'param'>\n"
              "  <!ENTITY ext SYSTEM 'ext.xml'>\n"
              "  <!ENTITY img SYSTEM 'i.gif' NDATA gif>\n"
              "  <!NOTATION gif PUBLIC '-//GIF//EN'>\n"
              "  <?dtdpi data?>\n"
              "  <!-- dtd comment -->\n"
              "]>\n"
              "<doc x='1'><a>t<![CDATA[<raw>]]>&ge;</a><?pi  body ?><!-- c --></doc>");

  p = XML_ParserCreate(NULL); all_handlers(); XML_SetBase(p, "http://base/");
  printf("base=%s\n", XML_GetBase(p));
  parse_whole("base passed to declarations",
              "<!DOCTYPE d [<!ENTITY e SYSTEM 'e.xml'><!NOTATION n SYSTEM 'n'>]><d/>");

  p = XML_ParserCreateNS(NULL, '|'); all_handlers();
  parse_whole("namespaces",
              "<r xmlns='urn:d' xmlns:p='urn:p'><p:a p:x='1' y='2'><b xmlns=''/></p:a></r>");

  p = XML_ParserCreateNS(NULL, '|'); all_handlers(); XML_SetReturnNSTriplet(p, 1);
  parse_whole("namespace triplets", "<p:r xmlns:p='urn:p' p:a='v'><p:c/></p:r>");

  p = XML_ParserCreateNS(NULL, '\n'); all_handlers();
  parse_whole("newline separator (as ElementTree)", "<r xmlns='urn:d'/>");

  p = XML_ParserCreateNS(NULL, '|'); all_handlers();
  parse_whole("namespace error", "<p:r/>");

  p = XML_ParserCreate(NULL); XML_SetDefaultHandler(p, dflt);
  parse_whole("default handler only", "<?xml version='1.0'?><r a='1'>x<![CDATA[y]]><!--c--><?p q?>\n</r>");

  p = XML_ParserCreate(NULL);
  XML_SetElementHandler(p, stopping_start, end);
  parse_whole("stop from handler", "<r><stop/><after/></r>");

  p = XML_ParserCreate(NULL);
  {
    /* Separate statements: C leaves argument evaluation order open. */
    int r = XML_StopParser(p, XML_FALSE);
    printf("=== stop before parsing\nstop -> %d code=%d\n", r, XML_GetErrorCode(p));
  }
  XML_ParserFree(p);

  p = XML_ParserCreate(NULL);
  XML_SetElementHandler(p, reentrant_start, end);
  parse_whole("re-entrant parse", "<r/>");

  p = XML_ParserCreate(NULL); all_handlers();
  printf("=== parse after final\n");
  report(XML_Parse(p, "<r/>", 4, 1) == XML_STATUS_OK);
  report(XML_Parse(p, "<r/>", 4, 1) == XML_STATUS_OK);
  XML_ParserFree(p);

  p = XML_ParserCreate(NULL); all_handlers();
  printf("=== GetBuffer/ParseBuffer in chunks\n");
  {
    const char *doc = "<r a='1'>hello<b/>world</r>";
    int ok = 1;
    for (size_t off = 0; off < strlen(doc) && ok; off += 5) {
      size_t k = strlen(doc) - off < 5 ? strlen(doc) - off : 5;
      void *buf = XML_GetBuffer(p, (int)k);
      memcpy(buf, doc + off, k);
      ok = XML_ParseBuffer(p, (int)k, 0) == XML_STATUS_OK;
    }
    if (ok) ok = XML_Parse(p, NULL, 0, 1) == XML_STATUS_OK;
    report(ok);
  }
  XML_ParserFree(p);

  p = XML_ParserCreate(NULL); text_only();
  XML_SetUnknownEncodingHandler(p, latin2_ish, (void *)"D");
  parse_whole("unknown encoding via map", "<?xml version='1.0' encoding='x-test'?><r>\xa1z</r>");
  p = XML_ParserCreate(NULL); text_only();
  XML_SetUnknownEncodingHandler(p, latin2_ish, (void *)"D");
  parse_whole("byte invalid in map", "<?xml version='1.0' encoding='x-test'?><r>\xff</r>");
  p = XML_ParserCreate(NULL); text_only();
  XML_SetUnknownEncodingHandler(p, refuse_encoding, NULL);
  parse_whole("handler refuses encoding", "<?xml version='1.0' encoding='x-test'?><r/>");
  p = XML_ParserCreate(NULL); text_only();
  parse_whole("no handler for encoding", "<?xml version='1.0' encoding='x-test'?><r/>");

  p = XML_ParserCreate("ISO-8859-1"); text_only();
  parse_whole("protocol encoding overrides declaration", "<?xml version='1.0' encoding='utf-8'?><r>caf\xe9</r>");

  {
    XML_Memory_Handling_Suite ms = {my_malloc, my_realloc, my_free};
    p = XML_ParserCreate_MM(NULL, &ms, NULL); all_handlers();
    parse_whole("memory suite for content models",
                "<!DOCTYPE d [<!ELEMENT d (a|b)+><!ELEMENT a EMPTY>]><d/>");
    printf("content models balanced: %s\n", mallocs > 0 && frees > 0 ? "yes" : "no");
  }

  p = XML_ParserCreate(NULL);
  printf("=== settings\n");
  {
    int d1 = XML_SetReparseDeferralEnabled(p, XML_TRUE);
    int d2 = XML_SetReparseDeferralEnabled(p, 2);
    printf("deferral(1) -> %d deferral(2) -> %d\n", d1, d2);
    int a1 = XML_SetBillionLaughsAttackProtectionMaximumAmplification(p, 0.5f);
    int a2 = XML_SetBillionLaughsAttackProtectionMaximumAmplification(p, 50.0f);
    int a3 = XML_SetBillionLaughsAttackProtectionActivationThreshold(p, 1000);
    printf("max amp 0.5 -> %d, 50 -> %d, threshold -> %d\n", a1, a2, a3);
  }
  printf("hash salt -> %d\n", XML_SetHashSalt(p, 42));
  printf("XML_GetUserData macro reads user data: %s\n",
         (XML_SetUserData(p, (void *)"Q"), !strcmp((const char *)XML_GetUserData(p), "Q")) ? "yes" : "no");
  XML_ParserFree(p);

  p = XML_ParserCreate(NULL);
  XML_SetBillionLaughsAttackProtectionActivationThreshold(p, 1000);
  XML_SetBillionLaughsAttackProtectionMaximumAmplification(p, 2.0f);
  parse_whole("amplification limit",
              "<!DOCTYPE d [<!ENTITY a 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'>"
              "<!ENTITY b '&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;'>]>"
              "<d>&b;&b;&b;&b;&b;&b;&b;&b;</d>");

  printf("=== errors and version\n");
  printf("ErrorString(0)=%s ErrorString(9999)=%s\n", s(XML_ErrorString(0)), s(XML_ErrorString(9999)));
  for (int code = 1; code <= 44; code++) printf("%d %s\n", code, s(XML_ErrorString(code)));
  return 0;
}
