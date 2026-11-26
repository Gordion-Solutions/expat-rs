/* Markup in internal entities' replacement text, with handlers missing:
   libexpat passes the default handler the text in the replacement text
   that each callback would have come from (a tag, `<![CDATA[`, a comment,
   a line end), and XML_DefaultCurrent does the same. ElementTree's C
   accelerator relies on it: it sets no CDATA handlers and treats a
   reference reaching its default handler as an undefined entity. Built
   against libexpat and against expat-capi the outputs must be identical:
   tests/c/entities.expected is libexpat 2.8.2's output (regenerate with
   tools/regen_expected.sh), checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;
static int current; /* call XML_DefaultCurrent from the handlers */

static void at(void) { printf(" @%lu:%lu\n", XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p)); }
static void chars(const XML_Char *t, int len) {
  for (int i = 0; i < len; i++) {
    if (t[i] == '\n') printf("\\n");
    else if (t[i] == '\r') printf("\\r");
    else putchar(t[i]);
  }
}
static void dflt(void *u, const XML_Char *t, int len) {
  printf("  default [");
  chars(t, len);
  printf("]");
  at();
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("  <%s>", n);
  at();
  if (current) XML_DefaultCurrent(p);
}
static void end(void *u, const XML_Char *n) {
  printf("  </%s>", n);
  at();
  if (current) XML_DefaultCurrent(p);
}
static void text(void *u, const XML_Char *t, int len) {
  printf("  [");
  chars(t, len);
  printf("]");
  at();
  if (current) XML_DefaultCurrent(p);
}
static void comment(void *u, const XML_Char *t) { printf("  comment [%s]", t); at(); }
static void pi(void *u, const XML_Char *t, const XML_Char *d) { printf("  pi [%s] [%s]", t, d); at(); }
static void scdata(void *u) { printf("  start-cdata"); at(); }
static void ecdata(void *u) { printf("  end-cdata"); at(); }
/* The DOCTYPE has its handlers: what is tested is content. */
static void sdoctype(void *u, const XML_Char *n, const XML_Char *s, const XML_Char *pub, int subset) {}
static void edoctype(void *u) {}
static void entity(void *u, const XML_Char *n, int pe, const XML_Char *v, int vlen, const XML_Char *base,
                   const XML_Char *sys, const XML_Char *pub, const XML_Char *notation) {}
static void element(void *u, const XML_Char *n, XML_Content *model) { XML_FreeContentModel(p, model); }

static const char *docs[] = {
    "<!DOCTYPE d [<!ENTITY e \"a<b x='1'>c<!--k--><?p q?><![CDATA[z]]></b>&amp;&#65;<x/>\">]><d>&e;</d>",
    "<!DOCTYPE d [<!ENTITY f \"<![CDATA[&foo;]]>\"><!ENTITY e \"1&f;2\">]><d>&e;</d>",
    "<!DOCTYPE d [<!ENTITY e \"a\nb\r\nc&#10;&#38;#66;\">]>\n<d>\n&e;&e;</d>",
    /* XML 1.0 test xmltest/valid/sa/114.xml */
    "<!DOCTYPE doc [\n<!ELEMENT doc (#PCDATA)>\n<!ENTITY e \"<![CDATA[&foo;]]>\">\n]>\n<doc>&e;</doc>\n",
    "<!DOCTYPE d [<!ENTITY e \"<![CDATA[]]><![CDATA[x]y]]]]>\">]><d>&e;</d>",
    /* The same markup in the document itself. */
    "<d>a<b x='1'>c\nd\r\ne<!--k--><?p q?><![CDATA[z\n]]></b>&amp;&#65;<x/>\r\n</d>",
};

/* Which handlers are set, besides the default handler. */
enum { NONE, TEXT, ETREE, ALL, CURRENT };
static const char *set_names[] = {"none", "text", "etree", "all", "current"};

static void run(const char *doc, int set, size_t piece) {
  p = XML_ParserCreate(NULL);
  XML_SetDefaultHandlerExpand(p, dflt);
  XML_SetDoctypeDeclHandler(p, sdoctype, edoctype);
  XML_SetEntityDeclHandler(p, entity);
  XML_SetElementDeclHandler(p, element);
  current = set == CURRENT;
  if (set != NONE) {
    XML_SetElementHandler(p, start, end);
    XML_SetCharacterDataHandler(p, text);
  }
  if (set == ETREE || set == ALL) {
    XML_SetCommentHandler(p, comment);
    XML_SetProcessingInstructionHandler(p, pi);
  }
  if (set == ALL) XML_SetCdataSectionHandler(p, scdata, ecdata);
  size_t len = strlen(doc);
  enum XML_Status st = XML_STATUS_OK;
  if (piece == 0) {
    st = XML_Parse(p, doc, (int)len, 1);
  } else {
    for (size_t i = 0; i < len && st == XML_STATUS_OK; i += piece) {
      size_t n = len - i < piece ? len - i : piece;
      st = XML_Parse(p, doc + i, (int)n, 0);
    }
    if (st == XML_STATUS_OK) st = XML_Parse(p, NULL, 0, 1);
  }
  if (st == XML_STATUS_ERROR)
    printf("  -> %s @%lu:%lu\n", XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
           XML_GetCurrentColumnNumber(p));
  XML_ParserFree(p);
}

int main(void) {
  for (size_t d = 0; d < sizeof docs / sizeof *docs; d++) {
    for (int set = NONE; set <= CURRENT; set++) {
      printf("doc %zu, %s, whole\n", d, set_names[set]);
      run(docs[d], set, 0);
    }
    printf("doc %zu, etree, 1-byte pieces\n", d);
    run(docs[d], ETREE, 1);
  }
  return 0;
}
