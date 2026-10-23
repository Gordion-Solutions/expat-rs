/* Logs libexpat callbacks with positions, for comparing semantics with expat-rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>
static XML_Parser p;
static void pos(const char *what) {
  printf("  %-10s line=%lu col=%lu byte=%ld\n", what, XML_GetCurrentLineNumber(p),
         XML_GetCurrentColumnNumber(p), (long)XML_GetCurrentByteIndex(p));
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  int i = 0; while (a[i]) i++;
  printf("start <%s> attrs-array-len=%d specified-count=%d\n", n, i, XML_GetSpecifiedAttributeCount(p));
  pos("start");
}
static void end(void *u, const XML_Char *n) { printf("end </%s>\n", n); pos("end"); }
static void text(void *u, const XML_Char *s, int len) { printf("text [%.*s] (%d bytes)\n", len, s, len); pos("text"); }
static void dflt(void *u, const XML_Char *s, int len) { printf("default [%.*s]\n", len, s); }
static int run(const char *label, const char *doc, int chunk, int deferral, int with_default) {
  p = XML_ParserCreate(NULL);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  if (with_default) XML_SetDefaultHandler(p, dflt);
  XML_SetReparseDeferralEnabled(p, deferral ? XML_TRUE : XML_FALSE);
  printf("=== %s (chunk=%d deferral=%d)\n", label, chunk, deferral);
  size_t n = strlen(doc), off = 0;
  int ok = 1;
  while (off < n && ok) {
    size_t k = chunk ? (n - off < (size_t)chunk ? n - off : (size_t)chunk) : n;
    printf("-- feed %zu bytes\n", k);
    if (XML_Parse(p, doc + off, (int)k, 0) == XML_STATUS_ERROR) ok = 0;
    off += k;
  }
  if (ok) { printf("-- final\n"); if (XML_Parse(p, "", 0, 1) == XML_STATUS_ERROR) ok = 0; }
  if (!ok) printf("ERROR code=%d '%s' line=%lu col=%lu byte=%ld\n", XML_GetErrorCode(p),
                  XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
                  XML_GetCurrentColumnNumber(p), (long)XML_GetCurrentByteIndex(p));
  XML_ParserFree(p);
  return ok;
}
int main(void) {
  run("multibyte columns", "<r>\xc3\xa9\xe2\x82\xac<b x='1'/>\n  \xf0\x9d\x84\x9e<c/></r>", 0, 1, 0);
  run("specified count + DTD default", "<!DOCTYPE r [<!ATTLIST r d CDATA 'dv'>]><r a='1' b='2'/>", 0, 1, 0);
  run("error: mismatched tag", "<a>\n  <b></c>\n</a>", 0, 1, 0);
  run("error: invalid char after multibyte", "<a>\xc3\xa9\x01</a>", 0, 1, 0);
  run("error: unclosed", "<a><b>", 0, 1, 0);
  run("error: junk after root", "<a/>\n  x", 0, 1, 0);
  run("error: dup attr", "<a x='1' x='2'/>", 0, 1, 0);
  run("error: undefined entity", "<a>&nope;</a>", 0, 1, 0);
  run("deferral on", "<doc/>", 4, 1, 0);
  run("deferral off", "<doc/>", 4, 0, 0);
  run("text chunking with refs", "<a>one&amp;two&#65;three</a>", 0, 1, 0);
  run("default handler", "<?xml version='1.0'?>\n<!-- c --><a x='&#65;'>t&amp;</a>\n", 0, 1, 1);
  return 0;
}
