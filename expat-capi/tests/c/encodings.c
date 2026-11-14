/* Unknown encodings through XML_SetUnknownEncodingHandler: which maps
   libexpat accepts, single-byte and multi-byte (`convert`) decoding, what a
   converter may return, when `release` is called, and a UTF-8 byte order
   mark before a declaration of another encoding. Byte indexes are shown
   only for documents without multi-byte sequences (expat-capi counts a
   sequence as one byte; see README). Built against libexpat and against
   expat-capi the outputs must be identical: tests/c/encodings.expected is
   libexpat 2.8.2's output (regenerate with tools/regen_expected.sh),
   checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <string.h>

static XML_Parser p;
static int releases;
static int mode; /* which map the handler gives */
static int show_bytes;

static void at(const char *what) {
  printf("  %-12s @%lu:%lu", what, XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p));
  if (show_bytes) printf(" byte=%ld", (long)XML_GetCurrentByteIndex(p));
  printf("\n");
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  char w[64];
  snprintf(w, sizeof w, "<%s>", n);
  at(w);
  for (; *a; a += 2) printf("    attr %s=%s\n", a[0], a[1]);
}
static void end(void *u, const XML_Char *n) { at("end"); }
static void text(void *u, const XML_Char *t, int len) {
  printf("  text [");
  for (int i = 0; i < len; i++) printf((unsigned char)t[i] < 0x80 && t[i] >= 0x20 ? "%c" : "\\x%02x", (unsigned char)t[i]);
  printf("]");
  at("");
}
static int conv(void *data, const char *s) {
  unsigned char b = s[1];
  if (b == 0xFF) return -1;
  if (b == 0xF0) return 0x10000;  /* above U+FFFF */
  if (b == 0xF1) return 0xD800;   /* surrogate */
  if (b == 0xF2) return 0x01;     /* control */
  if (b == 0xF3) return 0xFFFE;   /* not a character */
  if (b == 0xF4) return 0xB7;     /* name char, not a name start */
  return 0x4E00 + b;
}
static int conv3(void *data, const char *s) { return 0x5000 + (unsigned char)s[2]; }
static void rel(void *data) { releases++; printf("  release(%s)\n", (const char *)data); }

static int handler(void *u, const XML_Char *name, XML_Encoding *info) {
  for (int i = 0; i < 256; i++) info->map[i] = i < 0x80 ? i : -1;
  info->data = "d";
  info->convert = conv;
  info->release = rel;
  switch (mode) {
  case 0: for (int i = 0x81; i <= 0x82; i++) info->map[i] = -2; break;     /* double byte */
  case 1: info->map[0xA0] = 0x10000; break;                                /* single > U+FFFF */
  case 2: info->map[0xA0] = 0xD800; break;                                 /* single surrogate */
  case 3: info->map[0x81] = -5; break;                                     /* below -4 */
  case 4: info->map[0x81] = -2; info->convert = NULL; break;               /* lead, no convert */
  case 5: info->map[0x01] = -2; break;                                     /* lead on a control byte */
  case 6: info->map['$'] = -2; break;                                      /* lead on '$' */
  case 7: info->map[0xA0] = 0xE9; break;                                   /* single only, convert set */
  case 8: info->map[0x83] = -3; info->convert = conv3; break;              /* triple byte */
  case 9: info->map[0xA0] = 0x1; break;                                    /* single control */
  case 10: info->map[0xA0] = 0xFFFE; break;
  }
  return XML_STATUS_OK;
}

static void run(const char *label, int m, const char *doc, size_t len, size_t piece) {
  mode = m;
  /* A byte from 0x81 to 0x83, or 0x01 in map 5, starts a sequence. */
  show_bytes = 1;
  for (size_t i = 0; i < len; i++) {
    unsigned char b = (unsigned char)doc[i];
    if ((m == 0 || m == 8) && b >= 0x81 && b <= 0x83) show_bytes = 0;
    if (m == 5 && b == 0x01) show_bytes = 0;
  }
  releases = 0;
  printf("== %s (map %d, pieces %zu)\n", label, m, piece);
  p = XML_ParserCreate(NULL);
  XML_SetUnknownEncodingHandler(p, handler, NULL);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  enum XML_Status st = XML_STATUS_OK;
  for (size_t off = 0; off < len && st == XML_STATUS_OK; off += piece)
    st = XML_Parse(p, doc + off, (int)(len - off < piece ? len - off : piece), XML_FALSE);
  if (st == XML_STATUS_OK) st = XML_Parse(p, NULL, 0, XML_TRUE);
  if (st == XML_STATUS_ERROR) {
    printf("  -> %s", XML_ErrorString(XML_GetErrorCode(p)));
    at("");
  }
  printf("  releases before free: %d\n", releases);
  XML_ParserFree(p);
  printf("  releases after free: %d\n", releases);
}

#define D(s) s, sizeof(s) - 1
#define H "<?xml version='1.0' encoding='x-test'?>"

int main(void) {
  size_t pieces[] = {4096, 1};
  for (int k = 0; k < 2; k++) {
    size_t pc = pieces[k];
    run("plain double byte", 0, D(H "<r a='\x81\x41'>x\x81\x42y\n\x82\x43</r>"), pc);
    run("name chars", 0, D(H "<\x81\x41 \x82\x42='1'/>"), pc);
    run("name start not allowed", 0, D(H "<\x81\xF4/>"), pc);
    run("name char after start", 0, D(H "<a\x81\xF4/>"), pc);
    run("convert -1", 0, D(H "<r>ab\x81\xFF</r>"), pc);
    run("convert > U+FFFF", 0, D(H "<r>ab\x81\xF0</r>"), pc);
    run("convert surrogate", 0, D(H "<r>ab\x81\xF1</r>"), pc);
    run("convert control", 0, D(H "<r>ab\x81\xF2</r>"), pc);
    run("convert FFFE", 0, D(H "<r>ab\x81\xF3</r>"), pc);
    run("cut off", 0, D(H "<r>ab\x81"), pc);
    run("bad lead byte", 0, D(H "<r>ab\x90</r>"), pc);
    run("triple", 8, D(H "<r>a\x83\x01\x41z</r>"), pc);
  }
  run("single > U+FFFF", 1, D(H "<r>\xA0</r>"), 4096);
  run("single surrogate", 2, D(H "<r>\xA0</r>"), 4096);
  run("below -4", 3, D(H "<r/>"), 4096);
  run("lead without convert", 4, D(H "<r/>"), 4096);
  run("lead on a control byte", 5, D(H "<r>\x01\x41</r>"), 4096);
  run("lead on $", 6, D(H "<r/>"), 4096);
  run("single only, convert set", 7, D(H "<r>\xA0</r>"), 4096);
  run("single control", 9, D(H "<r>\xA0</r>"), 4096);
  run("single FFFE", 10, D(H "<r>\xA0</r>"), 4096);
  run("BOM then latin-1", 0, D("\xEF\xBB\xBF<?xml version='1.0' encoding='iso-8859-1'?>\n<r>caf\xE9</r>"), 4096);
  run("BOM then latin-1, pieces", 0, D("\xEF\xBB\xBF<?xml version='1.0' encoding='iso-8859-1'?>\n<r>caf\xE9</r>"), 1);
  run("BOM then unknown", 0, D("\xEF\xBB\xBF<?xml version='1.0' encoding='x-test'?><r>\x81\x41</r>"), 4096);
  run("BOM then ascii", 0, D("\xEF\xBB\xBF<?xml version='1.0' encoding='us-ascii'?><r>a</r>"), 4096);
  return 0;
}
