/* The cases of declarations.c with the DOCTYPE arriving in small pieces.
   libexpat reports each declaration (and the default handler's text) as
   it completes; expat-rs reads the internal subset as one construct, so
   until it closes nothing is reported, and reparse deferral then holds
   later events too. Not yet matching: tests/abi.rs ignores this probe.

   From declarations.c: public identifiers, predefined entities
   declared again, declarations after a parameter entity reference that
   isn't read, and the default handler's text and character data when
   input arrives in small pieces. (Internal parameter entities aren't
   expanded by expat-capi, so no case here parses them.) Built against libexpat and against
   expat-capi the outputs must be identical: tests/c/dtd_pieces.expected
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

static const char *dtd_whitespace =
    "<?xml version='1.0'?>\n\n"
    "<!DOCTYPE d [\n\n"
    "    <!ELEMENT d ANY>   \n"
    "\t<!ATTLIST d a CDATA #IMPLIED>\n\n\n"
    "  <!-- comment -->  \n"
    " ]>  \n\n"
    "<d>\n  text\n</d>\n\n";

int main(void) {
  size_t whole = 1 << 20;
  (void)whole;
  run("public identifiers", pubids, 0, 5);
  run("internal entities, non-expanding default handler", no_expand, DEFAULT, 4);
  run("predefined entities declared again", predefined, EXPAND, 6);
  run("after an internal parameter entity reference", after_internal_pe, EXPAND, 7);
  run("whitespace in and around the DOCTYPE", dtd_whitespace, DEFAULT, 1);
  run("whitespace in and around the DOCTYPE", dtd_whitespace, DEFAULT, 3);
  run("whitespace in and around the DOCTYPE", dtd_whitespace, DEFAULT, 7);
  run("whitespace in and around the DOCTYPE", dtd_whitespace, EXPAND, 7);
  return 0;
}
