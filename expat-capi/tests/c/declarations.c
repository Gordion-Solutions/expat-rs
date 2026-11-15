/* DTD declarations with many handlers set at once, and the default
   handler alongside them: public identifiers, predefined entities
   declared again, declarations after a parameter entity reference that
   isn't read, and the default handler's text and character data when
   input arrives in small pieces. (Internal parameter entities aren't
   expanded by expat-capi, so no case here parses them.) Built against libexpat and against
   expat-capi the outputs must be identical: tests/c/declarations.expected
   is libexpat 2.8.2's output (regenerate with tools/regen_expected.sh),
   checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static int feed; /* index of the XML_Parse call delivering the event */

static void show(const char *s, int len) {
  putchar('[');
  for (int i = 0; i < len; i++) {
    unsigned char c = s[i];
    if (c == '\n') printf("\\n");
    else if (c == '\r') printf("\\r");
    else if (c == '\t') printf("\\t");
    else putchar(c);
  }
  putchar(']');
}
static const char *opt(const XML_Char *s) { return s ? s : "-"; }

static void xmldecl(void *u, const XML_Char *v, const XML_Char *e, int sa) {
  printf("  %d xmldecl %s %s %d\n", feed, opt(v), opt(e), sa);
}
static void startdt(void *u, const XML_Char *n, const XML_Char *sys, const XML_Char *pub, int internal) {
  printf("  %d doctype %s sys=[%s] pub=[%s] %d\n", feed, n, opt(sys), opt(pub), internal);
}
static void enddt(void *u) { printf("  %d enddoctype\n", feed); }
static void entity(void *u, const XML_Char *n, int pe, const XML_Char *v, int vlen, const XML_Char *base,
                   const XML_Char *sys, const XML_Char *pub, const XML_Char *notation) {
  printf("  %d entity %s%s value=", feed, pe ? "%" : "", n);
  if (v) show(v, vlen); else printf("-");
  printf(" sys=[%s] pub=[%s] ndata=%s\n", opt(sys), opt(pub), opt(notation));
}
static void notation(void *u, const XML_Char *n, const XML_Char *base, const XML_Char *sys, const XML_Char *pub) {
  printf("  %d notation %s sys=[%s] pub=[%s]\n", feed, n, opt(sys), opt(pub));
}
static void element(void *u, const XML_Char *n, XML_Content *model) {
  printf("  %d elementdecl %s\n", feed, n);
}
static void attlist(void *u, const XML_Char *el, const XML_Char *att, const XML_Char *type, const XML_Char *dflt,
                    int req) {
  printf("  %d attlist %s %s %s %s %d\n", feed, el, att, type, opt(dflt), req);
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("  %d <%s", feed, n);
  for (; *a; a += 2) printf(" %s=%s", a[0], a[1]);
  printf(">\n");
}
static void end(void *u, const XML_Char *n) { printf("  %d </%s>\n", feed, n); }
static void text(void *u, const XML_Char *s, int len) {
  printf("  %d text ", feed);
  show(s, len);
  putchar('\n');
}
static void deflt(void *u, const XML_Char *s, int len) {
  printf("  %d default ", feed);
  show(s, len);
  putchar('\n');
}
static void comment(void *u, const XML_Char *s) { printf("  %d comment %s\n", feed, s); }
static void skipped(void *u, const XML_Char *n, int pe) { printf("  %d skipped %s%s\n", feed, pe ? "%" : "", n); }
static int extref(XML_Parser p, const XML_Char *ctx, const XML_Char *base, const XML_Char *sys,
                  const XML_Char *pub) {
  printf("  %d external %s sys=[%s] pub=[%s]\n", feed, ctx ? ctx : "(dtd)", opt(sys), opt(pub));
  return XML_STATUS_OK;
}

enum { DEFAULT = 1, EXPAND = 2, PE_ALWAYS = 4, TEXT_ONLY = 8, NO_SKIPPED = 16, DEFAULT_NULL = 32, NO_DECL_HANDLERS = 64 };

static void run(const char *label, const char *doc, int flags, size_t piece) {
  printf("== %s (%s%s%s%s%s, pieces %zu)\n", label, flags & EXPAND ? "expand" : flags & DEFAULT ? "default" : flags & DEFAULT_NULL ? "default set to NULL" : "no default",
         flags & PE_ALWAYS ? ", PEs" : "", flags & TEXT_ONLY ? ", text only" : "",
         flags & NO_SKIPPED ? ", no skipped or external handler" : "",
         flags & NO_DECL_HANDLERS ? ", no declaration handlers" : "", piece);
  XML_Parser p = XML_ParserCreate(NULL);
  if (flags & TEXT_ONLY) {
    XML_SetCharacterDataHandler(p, text);
  } else {
    XML_SetXmlDeclHandler(p, xmldecl);
    XML_SetDoctypeDeclHandler(p, startdt, enddt);
    if (!(flags & NO_DECL_HANDLERS)) {
      XML_SetEntityDeclHandler(p, entity);
      XML_SetNotationDeclHandler(p, notation);
      XML_SetElementDeclHandler(p, element);
      XML_SetAttlistDeclHandler(p, attlist);
    }
    XML_SetElementHandler(p, start, end);
    XML_SetCharacterDataHandler(p, text);
    XML_SetCommentHandler(p, comment);
    if (!(flags & NO_SKIPPED)) XML_SetSkippedEntityHandler(p, skipped);
    if (!(flags & NO_SKIPPED)) XML_SetExternalEntityRefHandler(p, extref);
  }
  if (flags & EXPAND) XML_SetDefaultHandlerExpand(p, deflt);
  else if (flags & DEFAULT) XML_SetDefaultHandler(p, deflt);
  else if (flags & DEFAULT_NULL) XML_SetDefaultHandler(p, NULL);
  if (flags & PE_ALWAYS) XML_SetParamEntityParsing(p, XML_PARAM_ENTITY_PARSING_ALWAYS);
  size_t len = strlen(doc);
  enum XML_Status st = XML_STATUS_OK;
  feed = 0;
  for (size_t off = 0; off < len && st == XML_STATUS_OK; off += piece, feed++)
    st = XML_Parse(p, doc + off, (int)(len - off < piece ? len - off : piece), XML_FALSE);
  if (st == XML_STATUS_OK) st = XML_Parse(p, NULL, 0, XML_TRUE);
  if (st == XML_STATUS_ERROR)
    printf("  -> %s @%lu:%lu\n", XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
           XML_GetCurrentColumnNumber(p));
  XML_ParserFree(p);
}

static const char *pubids =
    "<!DOCTYPE d PUBLIC \"  a  b\r\n c \" \"d.dtd\" [\n"
    "<!NOTATION n1 PUBLIC \" x\r\n y \">\n"
    "<!NOTATION n2 PUBLIC 'p  q' 'n2.sys'>\n"
    "<!ENTITY u SYSTEM 'u.bin' NDATA n1>\n"
    "<!ENTITY ext PUBLIC '\nP\r\n Q ' 'ext.xml'>\n"
    "<!ENTITY % pe PUBLIC '  R  ' 'pe.dtd'>\n"
    "]><d/>";

static const char *bad_pubid = "<!DOCTYPE d PUBLIC \"a\tb\" \"d.dtd\"><d/>";

/* Internal entities in content with a non-expanding default handler
   (XML_SetDefaultHandler): reported, not expanded. */
static const char *no_expand =
    "<!DOCTYPE d [<!ENTITY i 'I<e/>&j;'><!ENTITY j 'J'><!ENTITY x SYSTEM 'x.xml'>]>"
    "<d a='&j;'>&i;&gt;&#65;&x;<e>&j;</e></d>";

static const char *predefined =
    "<!DOCTYPE d [\n"
    "<!ENTITY gt \"&#62;\">\n"
    "<!ENTITY lt \"&#38;#60;\">\n"
    "<!ENTITY amp '&#38;#38;'>\n"
    "<!ENTITY apos \"'\">\n"
    "<!ENTITY quot 'other'>\n"
    "<!ENTITY x 'after'>\n"
    "]><d>&gt;&lt;&amp;&apos;&quot;&x;</d>";

static const char *after_internal_pe =
    "<?xml version='1.0'?>\n"
    "<!DOCTYPE d [\n"
    "<!ELEMENT d ANY>\n"
    "<!ENTITY % pe1 '<!ATTLIST d a1 CDATA \"v1\">'>\n"
    "<!ENTITY g1 'one'>\n"
    "%pe1;\n"
    "<!ENTITY g2 'two'>\n"
    "<!ATTLIST d a2 CDATA \"v2\">\n"
    "<!NOTATION n SYSTEM 'n'>\n"
    "<!ELEMENT e EMPTY>\n"
    "<!-- c -->\n"
    "]>\n"
    "<d>&g1;&g2;</d>";

static const char *after_external_pe =
    "<!DOCTYPE d [\n"
    "<!ENTITY % ext SYSTEM 'ext.dtd'>\n"
    "<!ENTITY g1 'one'>\n"
    "%ext;\n"
    "<!ENTITY g2 'two'>\n"
    "<!ATTLIST d a2 CDATA 'v2'>\n"
    "]>\n"
    "<d>&g1;&g2;</d>";

static const char *standalone_undeclared_pe =
    "<?xml version='1.0' standalone='yes'?>\n"
    "<!DOCTYPE d [\n"
    "<!ELEMENT d ANY>\n"
    "%paaa;\n"
    "<!ENTITY % paaa '<!ATTLIST d att CDATA #IMPLIED>'>\n"
    "<!ENTITY aaa 'aString'>\n"
    "]>\n"
    "<d>&aaa;</d>";

static const char *standalone_no_pe =
    "<?xml version='1.0' standalone='no'?>\n"
    "<!DOCTYPE d [\n"
    "<!ENTITY % pe1 '<!ENTITY g0 \"zero\">'>\n"
    "%pe1;\n"
    "<!ENTITY g1 'one'>\n"
    "]>\n"
    "<d>&g0;&g1;</d>";

static const char *standalone_yes_pe =
    "<?xml version='1.0' standalone='yes'?>\n"
    "<!DOCTYPE d [\n"
    "<!ENTITY % pe1 '<!ENTITY g0 \"zero\">'>\n"
    "%pe1;\n"
    "<!ENTITY g1 'one'>\n"
    "<!ATTLIST d a CDATA 'v'>\n"
    "]>\n"
    "<d>&g0;&g1;</d>";

static const char *declared_twice =
    "<!DOCTYPE d [\n"
    "<!ENTITY e 'first'>\n"
    "<!ENTITY e \"second\">\n"
    "<!ENTITY % p 'one'>\n"
    "<!ENTITY % p 'two'>\n"
    "<!ENTITY u SYSTEM 'u.bin' NDATA n>\n"
    "<!ENTITY u SYSTEM 'v.bin'>\n"
    "<!ENTITY e 'third'>\n"
    "<!ENTITY u PUBLIC 'pp  q' 'w.bin'>\n"
    "<!ENTITY u SYSTEM 'x.bin' NDATA n>\n"
    "<!ENTITY u PUBLIC 'pp' 'y.bin' NDATA n>\n"
    "<!ENTITY % p SYSTEM 'p.dtd'>\n"
    "<!ENTITY % q SYSTEM 'q.dtd'>\n"
    "<!ENTITY % q PUBLIC 'qq' 'q2.dtd'>\n"
    "<!ENTITY lt SYSTEM 'lt.txt'>\n"
    "<!ENTITY gt PUBLIC 'g' 'gt.txt' NDATA n>\n"
    "]><d>&e;</d>";

static const char *attlists =
    "<!DOCTYPE d [\n"
    "<!ATTLIST d>\n"
    "<!ATTLIST d >\n"
    "<!ATTLIST d a CDATA #IMPLIED>\n"
    "<!ATTLIST d\n  b CDATA 'x'\n  c (p|q|r) 'p'>\n"
    "]><d/>";

static const char *choices =
    "<!DOCTYPE d [\n"
    "<!ELEMENT a (b|c)+>\n"
    "<!ELEMENT a2 (b|c|d)>\n"
    "<!ELEMENT a3 ( b | c | d | e )>\n"
    "<!ELEMENT a4 ((b|c|d),e)>\n"
    "<!ELEMENT a5 ((b|c|d)|(e|f|g))*>\n"
    "<!ELEMENT a6 (#PCDATA|b|c|d)*>\n"
    "<!ELEMENT a7 (b,c,d,e)>\n"
    "<!ELEMENT a8 (b?|c*|d+)>\n"
    "]><d/>";

static const char *dtd_whitespace =
    "<?xml version='1.0'?>\n\n"
    "<!DOCTYPE d [\n\n"
    "    <!ELEMENT d ANY>   \n"
    "\t<!ATTLIST d a CDATA #IMPLIED>\n\n\n"
    "  <!-- comment -->  \n"
    " ]>  \n\n"
    "<d>\n  text\n</d>\n\n";

/* Text with two-byte characters, fed in pieces that split them. */
static const char *split_chars =
    "<d>\n \ta0=\":\" a00=\":\"\n \ta1=\"\xc3\x80\" a10=\"\xc3\x80\"\n \ta2=\"\xc3\x81\" a20=\"\xc3\x81\"\n"
    " \ta3=\"\xcb\xbe\" a30=\"\xcb\xbe\"\n \ta4=\"\xc3\x82\" a40=\"\xc3\x82\"\n \ta5=\"\xc3\x83\" a50=\"\xc3\x83\"\n"
    " \ta6=\"\xcb\xbd\" a60=\"\xcb\xbd\"\n \ta7=\"\xcb\xbf\" a70=\"\xcb\xbf\"\n \ta8=\"\xcd\xb0\" a80=\"\xcd\xb0\"\n"
    " \ta9=\"\xcd\xb1\" a90=\"\xcd\xb1\"\n</d>";

/* The same without any multi-byte character, for comparison. */
static const char *ascii_text =
    "<d>\n \ta0=\":\" a00=\":\"\n \ta1=\"A\" a10=\"A\"\n \ta2=\"B\" a20=\"B\"\n"
    " \ta3=\"C\" a30=\"C\"\n \ta4=\"D\" a40=\"D\"\n \ta5=\"E\" a50=\"E\"\n</d>";

int main(void) {
  size_t whole = 1 << 20;
  run("public identifiers", pubids, 0, whole);
  run("public identifiers", pubids, DEFAULT, whole);
  run("public identifiers", pubids, PE_ALWAYS, whole);
  run("tab in a public identifier", bad_pubid, 0, whole);

  run("internal entities, non-expanding default handler", no_expand, DEFAULT, whole);
  run("internal entities, non-expanding default handler", no_expand, DEFAULT | NO_SKIPPED, whole);
  run("internal entities, expanding default handler", no_expand, EXPAND, whole);
  run("internal entities, default handler set to NULL", no_expand, DEFAULT_NULL, whole);
  run("internal entities, default handler set to NULL", no_expand, DEFAULT_NULL | NO_SKIPPED, whole);

  run("predefined entities declared again", predefined, 0, whole);
  run("predefined entities declared again", predefined, DEFAULT, whole);
  run("predefined entities declared again", predefined, EXPAND, whole);

  run("after an internal parameter entity reference", after_internal_pe, 0, whole);
  run("after an internal parameter entity reference", after_internal_pe, DEFAULT, whole);
  run("after an internal parameter entity reference", after_internal_pe, EXPAND, whole);
  run("after an external parameter entity reference", after_external_pe, EXPAND, whole);
  run("after an external parameter entity reference", after_external_pe, PE_ALWAYS | EXPAND, whole);
  run("undeclared parameter entity, standalone", standalone_undeclared_pe, 0, whole);
  run("undeclared parameter entity, standalone", standalone_undeclared_pe, EXPAND, whole);
  run("parameter entity, standalone='no'", standalone_no_pe, EXPAND, whole);
  run("parameter entity, standalone='yes'", standalone_yes_pe, EXPAND, whole);

  run("entities declared twice", declared_twice, 0, whole);
  run("entities declared twice", declared_twice, DEFAULT, whole);
  run("entities declared twice", declared_twice, EXPAND, whole);
  run("attribute-list declarations", attlists, EXPAND, whole);
  run("choice groups", choices, EXPAND, whole);
  run("choice groups", choices, DEFAULT | NO_DECL_HANDLERS, whole);

  run("whitespace in and around the DOCTYPE", dtd_whitespace, DEFAULT, whole);

  run("two-byte characters split between pieces", split_chars, TEXT_ONLY, 7);
  run("two-byte characters split between pieces", split_chars, TEXT_ONLY, 5);
  run("two-byte characters split between pieces", split_chars, TEXT_ONLY | DEFAULT, 7);
  run("ASCII text in the same pieces", ascii_text, TEXT_ONLY, 7);
  return 0;
}
