/* CDATA sections fed in pieces: libexpat delivers a section's text as far
   as the input goes (holding back a `]` or `\r` that the next piece could
   turn into `]]>` or a line end), so how the text is split depends on the
   pieces, and callers such as aapt2's pull parser see each part. Built
   against libexpat and against expat-capi the outputs must be identical:
   tests/c/cdata.expected is libexpat 2.8.2's output (regenerate with
   tools/regen_expected.sh), checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;
static int stop_at; /* stop in the n-th CDATA callback, 0 for never */
static int calls;

static void at(void) { printf(" @%lu:%lu\n", XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p)); }
static void maybe_stop(void) {
  if (++calls == stop_at) {
    printf("  (stopping)\n");
    XML_StopParser(p, XML_FALSE);
  }
}
static void chars(const XML_Char *t, int len) {
  for (int i = 0; i < len; i++) {
    if (t[i] == '\n') printf("\\n");
    else if (t[i] == '\r') printf("\\r");
    else putchar(t[i]);
  }
}
static void start(void *u, const XML_Char *n, const XML_Char **a) { printf("  <%s>", n); at(); }
static void end(void *u, const XML_Char *n) { printf("  </%s>", n); at(); }
static void text(void *u, const XML_Char *t, int len) {
  printf("  [");
  chars(t, len);
  printf("]");
  at();
  maybe_stop();
}
static void scdata(void *u) { printf("  start-cdata"); at(); maybe_stop(); }
static void ecdata(void *u) { printf("  end-cdata"); at(); maybe_stop(); }
static void dflt(void *u, const XML_Char *t, int len) {
  printf("  default [");
  chars(t, len);
  printf("]");
  at();
}

static void report(enum XML_Status st) {
  if (st == XML_STATUS_ERROR)
    printf("  -> %s @%lu:%lu\n", XML_ErrorString(XML_GetErrorCode(p)), XML_GetCurrentLineNumber(p),
           XML_GetCurrentColumnNumber(p));
}

static void run(const char *doc, size_t len, size_t piece, int with_default, const char *encoding) {
  calls = 0;
  p = XML_ParserCreate(encoding);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  if (with_default) XML_SetDefaultHandler(p, dflt);
  else XML_SetCdataSectionHandler(p, scdata, ecdata);
  enum XML_Status st = XML_STATUS_OK;
  for (size_t off = 0; off < len && st == XML_STATUS_OK; off += piece)
    st = XML_Parse(p, doc + off, (int)(len - off < piece ? len - off : piece), XML_FALSE);
  if (st == XML_STATUS_OK) st = XML_Parse(p, NULL, 0, XML_TRUE);
  report(st);
  XML_ParserFree(p);
}

int main(void) {
  const char *docs[] = {
      "<r>ab<![CDATA[<raw> & stuff\nline2]]>cd</r>",
      "<r><![CDATA[a]b]]c]]]d]]]]>e]]></r>",
      "<r><![CDATA[x\r\ny\rz\r\n]]></r>",
      "<r><![CDATA[caf\xc3\xa9 \xe2\x82\xac \xf0\x9f\x98\x80 end]]></r>",
      "<r><![CDATA[]]><![CDATA[]]]><![CDATA[]]]]></r>",
      "<r>t<![CDATA[one]]>u<![CDATA[two\nthree]]>v</r>",
  };
  for (int d = 0; d < 6; d++) {
    for (size_t piece = 1; piece <= 16; piece++) {
      printf("== document %d in pieces of %zu\n", d + 1, piece);
      run(docs[d], strlen(docs[d]), piece, 0, NULL);
    }
  }

  printf("== CDATA in an entity's replacement text\n");
  const char *ent = "<!DOCTYPE r [<!ENTITY e '<![CDATA[in & entity]]>'>]><r>&e;<![CDATA[]]>&e;</r>";
  run(ent, strlen(ent), 4096, 0, NULL);
  run(ent, strlen(ent), 5, 0, NULL);
  const char *ent2 = "<!DOCTYPE r [<!ENTITY e '<![CDATA[a]b\nc]]d\r\ne]]>'>]>\n<r>&e;</r>";
  for (size_t piece = 3; piece <= 9; piece += 3) run(ent2, strlen(ent2), piece, 0, NULL);
  run(ent2, strlen(ent2), 4096, 0, NULL);
  for (stop_at = 1; stop_at <= 3; stop_at++) {
    printf("-- stop at callback %d\n", stop_at);
    run(ent2, strlen(ent2), 4096, 0, NULL);
  }
  stop_at = 0;

  printf("== the default handler\n");
  for (size_t piece = 3; piece <= 9; piece += 3) run(docs[0], strlen(docs[0]), piece, 1, NULL);
  run(docs[1], strlen(docs[1]), 4, 1, NULL);

  printf("== an invalid character inside\n");
  const char *bad = "<r><![CDATA[good\x01 bad]]></r>";
  run(bad, strlen(bad), 4096, 0, NULL);
  run(bad, strlen(bad), 3, 0, NULL);

  printf("== unclosed\n");
  const char *open = "<r><![CDATA[never ends";
  run(open, strlen(open), 4096, 0, NULL);
  run(open, strlen(open), 4, 0, NULL);
  const char *open2 = "<r><![CDATA[almost]]";
  run(open2, strlen(open2), 3, 0, NULL);

  printf("== a stray ]]> in content\n");
  const char *strays[] = {"<r>abcd]]></r>", "<r>ab\ncd]]></r>", "<r>ab\r\ncd]]>e</r>", "<r>]]></r>", "<r>a]]>b</r>",
                          "<r>caf\xc3\xa9]]></r>"};
  const size_t stray_pieces[] = {2, 5, 4096};
  for (int i = 0; i < 6; i++)
    for (int j = 0; j < 3; j++) run(strays[i], strlen(strays[i]), stray_pieces[j], 0, NULL);

  printf("== stopping in a CDATA callback\n");
  for (stop_at = 1; stop_at <= 4; stop_at++) {
    printf("-- stop at callback %d\n", stop_at);
    run(docs[0], strlen(docs[0]), 7, 0, NULL);
  }
  stop_at = 0;

  printf("== stopping in the start handler at the end of a chunk, then more\n");
  const char *heads[] = {"<r><![CDATA[", "<r><![CDATA[x", "<r><![CDATA[]]>", "<r>t<![CDATA["};
  for (int i = 0; i < 4; i++) {
    calls = 0;
    stop_at = 1;
    p = XML_ParserCreate(NULL);
    XML_SetCharacterDataHandler(p, text);
    XML_SetCdataSectionHandler(p, scdata, ecdata);
    const char *parts[] = {heads[i], "abc]]></r>", NULL};
    for (int j = 0; j < 3; j++) {
      enum XML_Status st = XML_Parse(p, parts[j], parts[j] ? (int)strlen(parts[j]) : 0, parts[j] == NULL);
      printf("  call %d -> %d code %d", j + 1, st, XML_GetErrorCode(p));
      at();
    }
    XML_ParserFree(p);
  }
  stop_at = 0;

  printf("== UTF-16LE\n");
  unsigned char u16[256];
  size_t n = 0;
  for (const char *c = "<r><![CDATA[ab]c\ndef]]></r>"; *c; c++) {
    u16[n++] = (unsigned char)*c;
    u16[n++] = 0;
  }
  for (size_t piece = 2; piece <= 10; piece += 2) run((const char *)u16, n, piece, 0, "UTF-16");
  return 0;
}
