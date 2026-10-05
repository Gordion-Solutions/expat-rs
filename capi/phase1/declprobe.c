/* Prints libexpat's element, attribute-list, entity and doctype
   declaration callbacks, to check expat-rs's declaration events. */
#include <expat.h>
#include <stdio.h>
#include <string.h>
static XML_Parser p;
static const char *kind[] = {"?", "EMPTY", "ANY", "MIXED", "NAME", "CHOICE", "SEQ"};
static const char *quant[] = {"", "?", "*", "+"};
static void model(XML_Content *m) {
  printf("%s%s%s", kind[m->type], m->name ? ":" : "", m->name ? m->name : "");
  if (m->numchildren) {
    printf("[");
    for (unsigned i = 0; i < m->numchildren; i++) { if (i) printf(" "); model(&m->children[i]); }
    printf("]");
  }
  printf("%s", quant[m->quant]);
}
static void el(void *u, const XML_Char *name, XML_Content *m) {
  printf("element %s: ", name); model(m); printf("\n"); XML_FreeContentModel(p, m);
}
static void att(void *u, const XML_Char *e, const XML_Char *a, const XML_Char *t, const XML_Char *d, int req) {
  printf("attlist %s %s type=%s default=%s required=%d\n", e, a, t, d ? d : "(null)", req);
}
static void ent(void *u, const XML_Char *n, int pe, const XML_Char *v, int vl, const XML_Char *b,
                const XML_Char *sys, const XML_Char *pub, const XML_Char *nota) {
  printf("entity %s param=%d value=%s%.*s%s sys=%s pub=%s notation=%s\n", n, pe, v ? "[" : "(null)", v ? vl : 0, v ? v : "",
         v ? "]" : "", sys ? sys : "(null)", pub ? pub : "(null)", nota ? nota : "(null)");
}
static void dt(void *u, const XML_Char *n, const XML_Char *sys, const XML_Char *pub, int internal) {
  printf("doctype %s sys=%s pub=%s internal=%d\n", n, sys ? sys : "(null)", pub ? pub : "(null)", internal);
}
int main(void) {
  const char *doc =
    "<!DOCTYPE r PUBLIC '-//X//Y//EN' 'r.dtd' [\n"
    "<!ELEMENT e1 EMPTY><!ELEMENT e2 ANY><!ELEMENT e3 (#PCDATA)><!ELEMENT e4 (#PCDATA|a|b)*>\n"
    "<!ELEMENT e5 (a)><!ELEMENT e6 (a,(b|c)*,d?)+>\n"
    "<!ATTLIST r c CDATA 'x  y' n NMTOKENS #IMPLIED q ID #REQUIRED f CDATA #FIXED 'fv' "
    "en ( a | b ) 'a' no NOTATION ( x | y ) #IMPLIED>\n"
    "<!ENTITY g 'v&#65;&amp;'><!ENTITY % pe 'pv'><!ENTITY x SYSTEM 'x.xml'>"
    "<!ENTITY u PUBLIC '-//U//EN' 'u.gif' NDATA gif>\n"
    "]><r q='1'/>";
  p = XML_ParserCreate(NULL);
  XML_SetElementDeclHandler(p, el);
  XML_SetAttlistDeclHandler(p, att);
  XML_SetEntityDeclHandler(p, ent);
  XML_SetStartDoctypeDeclHandler(p, dt);
  if (!XML_Parse(p, doc, (int)strlen(doc), 1)) printf("ERROR %s\n", XML_ErrorString(XML_GetErrorCode(p)));
  XML_ParserFree(p);
  return 0;
}
