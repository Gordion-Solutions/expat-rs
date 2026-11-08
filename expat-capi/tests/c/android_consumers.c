/* Calls the libexpat API the way Android's users of libexpat do, following
   their source in AOSP: libcore's ExpatParser (Java's XML parser, the
   reason ART links libexpat), Skia's SVG and font configuration parsers,
   aapt2 and aapt, frameworks/av's MediaProfiles and MediaCodecsXmlParser,
   audio_route and the wayland scanner. Each section keeps that caller's
   choice of functions, buffer sizes, end-of-input convention and handler
   setup, and logs every callback as delivered (text is not joined, since
   aapt2's pull parser makes one event per callback). Built against libexpat
   and against expat-capi the outputs must be identical:
   tests/c/android_consumers.expected is libexpat 2.8.2's output
   (regenerate with tools/regen_expected.sh), checked by tests/abi.rs. */
#include <expat.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The parser being driven, for positions in the logging handlers. */
static XML_Parser cur;
static char marker[] = "user-data";

static void at(void) { printf(" @%lu:%lu\n", XML_GetCurrentLineNumber(cur), XML_GetCurrentColumnNumber(cur)); }
static const char *s(const XML_Char *x) { return x ? x : "(null)"; }

/* aapt and aapt2 use '\1' as the namespace separator; print it as '|'. */
static void name(const XML_Char *n) {
  for (; *n; n++) putchar(*n == '\1' ? '|' : *n);
}
static void chars(const XML_Char *t, int len) {
  for (int i = 0; i < len; i++) {
    if (t[i] == '\n') printf("\\n");
    else if (t[i] == '\t') printf("\\t");
    else putchar(t[i]);
  }
}
static void start(void *u, const XML_Char *n, const XML_Char **a) {
  printf("  start ");
  name(n);
  for (int i = 0; a[i]; i += 2) { printf(" ["); name(a[i]); printf("=%s]", a[i + 1]); }
  at();
}
static void end(void *u, const XML_Char *n) { printf("  end "); name(n); at(); }
static void text(void *u, const XML_Char *t, int len) { printf("  text ["); chars(t, len); printf("]"); at(); }
static void comment(void *u, const XML_Char *d) { printf("  comment [%s]", d); at(); }
static void pi(void *u, const XML_Char *t, const XML_Char *d) { printf("  pi %s [%s]", t, d); at(); }
static void scdata(void *u) { printf("  start-cdata"); at(); }
static void ecdata(void *u) { printf("  end-cdata"); at(); }
static void sns(void *u, const XML_Char *prefix, const XML_Char *uri) { printf("  start-ns %s=%s", s(prefix), s(uri)); at(); }
static void ens(void *u, const XML_Char *prefix) { printf("  end-ns %s", s(prefix)); at(); }
static void sdoctype(void *u, const XML_Char *n, const XML_Char *sys, const XML_Char *pub, int internal) {
  printf("  doctype %s sys=%s pub=%s internal=%d", n, s(sys), s(pub), internal);
  at();
}
static void edoctype(void *u) { printf("  end-doctype"); at(); }
static void notation(void *u, const XML_Char *n, const XML_Char *base, const XML_Char *sys, const XML_Char *pub) {
  printf("  notation %s base=%s sys=%s pub=%s\n", n, s(base), s(sys), s(pub));
}
static void unparsed(void *u, const XML_Char *n, const XML_Char *base, const XML_Char *sys, const XML_Char *pub,
                     const XML_Char *notation) {
  printf("  unparsed %s base=%s sys=%s pub=%s notation=%s\n", n, s(base), s(sys), s(pub), s(notation));
}

static void report(XML_Parser p, enum XML_Status st) {
  if (st == XML_STATUS_ERROR) {
    enum XML_Error e = XML_GetErrorCode(p);
    printf("  -> error %d (%s) @%lu:%lu\n", e, XML_ErrorString(e), XML_GetCurrentLineNumber(p),
           XML_GetCurrentColumnNumber(p));
  } else {
    printf("  -> ok\n");
  }
}

/* A file in memory, read in fixed-size pieces as the callers fread theirs. */
struct file {
  const char *data;
  size_t len, pos;
};
static size_t file_read(struct file *f, void *buf, size_t n) {
  size_t k = f->len - f->pos < n ? f->len - f->pos : n;
  memcpy(buf, f->data + f->pos, k);
  f->pos += k;
  return k;
}

/* MediaProfiles, MediaCodecsXmlParser, audio_route, the wayland scanner:
   XML_GetBuffer, fread, XML_ParseBuffer(len, len == 0) until a read
   returns nothing. */
static enum XML_Status read_until_empty(XML_Parser p, const char *doc, int size) {
  struct file f = {doc, strlen(doc), 0};
  for (;;) {
    void *buf = XML_GetBuffer(p, size);
    if (!buf) { printf("  XML_GetBuffer failed\n"); return XML_STATUS_ERROR; }
    int n = (int)file_read(&f, buf, size);
    if (XML_ParseBuffer(p, n, n == 0) == XML_STATUS_ERROR) return XML_STATUS_ERROR;
    if (n == 0) return XML_STATUS_OK;
  }
}

/* Skia: XML_GetBuffer, read, XML_ParseBuffer(len, at_end), stopping on the
   read that reaches the end. */
static enum XML_Status read_until_end(XML_Parser p, const char *doc, int size) {
  struct file f = {doc, strlen(doc), 0};
  int done = 0;
  enum XML_Status st = XML_STATUS_OK;
  while (!done) {
    void *buf = XML_GetBuffer(p, size);
    if (!buf) { printf("  XML_GetBuffer failed\n"); return XML_STATUS_ERROR; }
    int n = (int)file_read(&f, buf, size);
    done = f.pos == f.len;
    st = XML_ParseBuffer(p, n, done);
    if (st == XML_STATUS_ERROR) break;
  }
  return st;
}

/* libcore and aapt2's XmlDom: XML_Parse on each piece, not final, then an
   empty final XML_Parse. */
static enum XML_Status pieces_then_final(XML_Parser p, const char *doc, size_t len, size_t piece) {
  for (size_t off = 0; off < len; off += piece) {
    size_t k = len - off < piece ? len - off : piece;
    if (XML_Parse(p, doc + off, (int)k, XML_FALSE) == XML_STATUS_ERROR) return XML_STATUS_ERROR;
  }
  return XML_Parse(p, NULL, 0, XML_TRUE);
}

/* aapt2's XmlPullParser: XML_Parse on each piece, the last one final. */
static enum XML_Status pieces_last_final(XML_Parser p, const char *doc, size_t piece) {
  size_t len = strlen(doc);
  for (size_t off = 0; off < len; off += piece) {
    size_t k = len - off < piece ? len - off : piece;
    if (XML_Parse(p, doc + off, (int)k, off + k == len) == XML_STATUS_ERROR) return XML_STATUS_ERROR;
  }
  return XML_STATUS_OK;
}

/* ---- libcore: org_apache_harmony_xml_ExpatParser.cpp ---- */

static const char *entity_text;
static int entity_result;

/* libcore keeps its context in the user data and reads it back with
   XML_GetUserData from the parser the handler is given; the Java side then
   creates the entity parser with XML_ExternalEntityParserCreate and feeds it
   from inside the callback. */
static int external(XML_Parser parser, const XML_Char *context, const XML_Char *base, const XML_Char *sys,
                    const XML_Char *pub) {
  printf("  external-entity sys=%s pub=%s base=%s user-data=%s", s(sys), s(pub), s(base),
         XML_GetUserData(parser) == marker ? "kept" : "lost");
  at();
  if (!entity_result) return XML_STATUS_ERROR;
  XML_Parser child = XML_ExternalEntityParserCreate(parser, context, NULL);
  if (!child) { printf("  entity parser: NULL\n"); return XML_STATUS_ERROR; }
  XML_Parser outer = cur;
  cur = child;
  printf("  entity parser: user-data=%s\n", XML_GetUserData(child) == marker ? "inherited" : "not inherited");
  report(child, pieces_then_final(child, entity_text, strlen(entity_text), 5));
  cur = outer;
  XML_ParserFree(child);
  return XML_STATUS_OK;
}

static XML_Parser libcore_parser(const char *encoding, int namespaces) {
  XML_Parser p = namespaces ? XML_ParserCreateNS(encoding, '|') : XML_ParserCreate(encoding);
  if (namespaces) {
    XML_SetNamespaceDeclHandler(p, sns, ens);
    XML_SetReturnNSTriplet(p, 1);
  }
  XML_SetCdataSectionHandler(p, scdata, ecdata);
  XML_SetCharacterDataHandler(p, text);
  XML_SetCommentHandler(p, comment);
  XML_SetDoctypeDeclHandler(p, sdoctype, edoctype);
  XML_SetElementHandler(p, start, end);
  XML_SetExternalEntityRefHandler(p, external);
  XML_SetNotationDeclHandler(p, notation);
  XML_SetProcessingInstructionHandler(p, pi);
  XML_SetUnparsedEntityDeclHandler(p, unparsed);
  XML_SetUserData(p, marker);
  return p;
}

static const char libcore_doc[] =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
    "<!DOCTYPE r [\n"
    "  <!NOTATION gif SYSTEM \"image/gif\">\n"
    "  <!ENTITY pic SYSTEM \"pic.gif\" NDATA gif>\n"
    "  <!ENTITY ext SYSTEM \"ext.xml\">\n"
    "  <!ENTITY int \"internal\">\n"
    "]>\n"
    "<r xmlns=\"urn:d\" xmlns:a=\"urn:a\" a:k=\"v\" plain=\"p\">\n"
    "  <?target some data?>\n"
    "  <!-- a comment -->\n"
    "  <a:x>text &int; more</a:x>\n"
    "  <![CDATA[<raw> & stuff]]>\n"
    "  &ext;\n"
    "  <y a:z=\"1\"/>\n"
    "</r>\n";

static void libcore(void) {
  printf("== libcore: namespaces, triplets, DTD declarations, an external entity\n");
  entity_text = "<?xml version='1.0' encoding='UTF-8'?><a:e q='w'>in &int; entity</a:e>tail";
  entity_result = 1;
  XML_Parser p = cur = libcore_parser("UTF-8", 1);
  report(p, pieces_then_final(p, libcore_doc, strlen(libcore_doc), 7));
  XML_ParserFree(p);

  printf("== libcore: without namespaces\n");
  p = cur = libcore_parser("UTF-8", 0);
  report(p, pieces_then_final(p, libcore_doc, strlen(libcore_doc), 64));
  XML_ParserFree(p);

  printf("== libcore: the entity handler reports failure\n");
  entity_result = 0;
  p = cur = libcore_parser("UTF-8", 1);
  report(p, pieces_then_final(p, libcore_doc, strlen(libcore_doc), 4096));
  XML_ParserFree(p);

  printf("== libcore: a malformed entity\n");
  entity_result = 1;
  entity_text = "<a:e>unclosed";
  p = cur = libcore_parser("UTF-8", 1);
  report(p, pieces_then_final(p, libcore_doc, strlen(libcore_doc), 4096));
  XML_ParserFree(p);

  printf("== libcore: malformed documents\n");
  const char *bad[] = {"<r><a></r>", "<r a='1' a='2'/>", "<r>&undefined;</r>", "<r/><s/>", "<r>\n  <x:y/>\n</r>",
                       "<?xml version='1.0'?><r>\xff</r>", ""};
  for (int i = 0; i < 7; i++) {
    p = cur = libcore_parser("UTF-8", 1);
    report(p, pieces_then_final(p, bad[i], strlen(bad[i]), 3));
    XML_ParserFree(p);
  }
}

/* Java strings reach libcore as UTF-16 in the machine's byte order with
   no byte order mark: the parser is created for "UTF-16" and libexpat
   tells the byte order from the first '<'. The document's own declaration
   (often UTF-8) is overridden. */
static size_t utf16le(const char *u8, unsigned char *out) {
  size_t n = 0;
  const unsigned char *c = (const unsigned char *)u8;
  while (*c) {
    unsigned long cp;
    if (*c < 0x80) cp = *c++;
    else if (*c < 0xE0) { cp = (unsigned long)(*c++ & 0x1F) << 6; cp |= *c++ & 0x3F; }
    else if (*c < 0xF0) { cp = (unsigned long)(*c++ & 0x0F) << 12; cp |= (unsigned long)(*c++ & 0x3F) << 6; cp |= *c++ & 0x3F; }
    else {
      cp = (unsigned long)(*c++ & 0x07) << 18;
      cp |= (unsigned long)(*c++ & 0x3F) << 12;
      cp |= (unsigned long)(*c++ & 0x3F) << 6;
      cp |= *c++ & 0x3F;
    }
    if (cp >= 0x10000) {
      unsigned long v = cp - 0x10000, hi = 0xD800 | (v >> 10), lo = 0xDC00 | (v & 0x3FF);
      out[n++] = hi & 0xFF; out[n++] = hi >> 8; out[n++] = lo & 0xFF; out[n++] = lo >> 8;
    } else {
      out[n++] = cp & 0xFF; out[n++] = cp >> 8;
    }
  }
  return n;
}

static void libcore_utf16(void) {
  const char *docs[] = {
      "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<a b=\"\xc3\xa9\">x\xe2\x82\xacy \xf0\x9f\x98\x80</a>",
      "<a xmlns:p=\"urn:p\"><p:b c=\"d\">plain</p:b></a>",
      "<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xc3\xa9t\xc3\xa9</a>",
  };
  unsigned char buf[512];
  for (int ns = 0; ns < 2; ns++) {
    for (int i = 0; i < 3; i++) {
      printf("== libcore: Java chars as UTF-16LE without a BOM, namespaces %s, document %d\n", ns ? "on" : "off", i + 1);
      size_t n = utf16le(docs[i], buf);
      XML_Parser p = cur = libcore_parser("UTF-16", ns);
      /* appendChars hands over whole chars: pieces of an even length. */
      report(p, pieces_then_final(p, (const char *)buf, n, 6));
      XML_ParserFree(p);
    }
  }
}

/* ---- Skia: src/xml/SkXMLParser.cpp, src/ports/SkFontMgr_android_parser.cpp ---- */

static int sk_allocs;
static void *sk_malloc(size_t n) { sk_allocs++; void *m = malloc(n); if (!m) abort(); return m; }
static void *sk_realloc(void *x, size_t n) { void *m = realloc(x, n); if (!m && n) abort(); return m; }
static void sk_free(void *x) { free(x); }
static const XML_Memory_Handling_Suite sk_XML_alloc = {sk_malloc, sk_realloc, sk_free};

/* Skia refuses entity declarations (CVE-2013-0340) by stopping the parser. */
static void sk_entity_decl(void *u, const XML_Char *n, int pe, const XML_Char *v, int vlen, const XML_Char *base,
                           const XML_Char *sys, const XML_Char *pub, const XML_Char *notation) {
  printf("  entity-decl %s pe=%d, stopping", n, pe);
  at();
  XML_StopParser(cur, XML_FALSE);
}

static XML_Parser skia_parser(void) {
  XML_Parser p = XML_ParserCreate_MM(NULL, &sk_XML_alloc, NULL);
  XML_SetHashSalt(p, 0x5eed);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  XML_SetEntityDeclHandler(p, sk_entity_decl);
  return p;
}

static const char svg[] =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\">\n"
    "  <title>Icon &amp; label</title>\n"
    "  <path d=\"M12 2L2 7l10 5 10-5-10-5z\" fill=\"#000\"/>\n"
    "  <text x=\"1\" y=\"2\">caf\xc3\xa9 &#x263A;</text>\n"
    "</svg>\n";

static void skia_svg(void) {
  printf("== Skia SkXMLParser: a stream in memory, XML_Parse\n");
  XML_Parser p = cur = skia_parser();
  report(p, XML_Parse(p, svg, (int)strlen(svg), XML_TRUE));
  XML_ParserFree(p);

  printf("== Skia SkXMLParser: a stream, XML_GetBuffer and XML_ParseBuffer\n");
  p = cur = skia_parser();
  report(p, read_until_end(p, svg, 4096));
  XML_ParserFree(p);

  const char *refused[] = {
      "<!DOCTYPE svg [<!ENTITY a \"x\">]>\n<svg>&a;</svg>",
      "<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n <!ENTITY lol \"lol\">\n"
      " <!ENTITY lol2 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\n]>\n<lolz>&lol2;</lolz>",
      "<!DOCTYPE svg [<!ENTITY % pe \"x\">]>\n<svg/>",
  };
  for (int i = 0; i < 3; i++) {
    printf("== Skia SkXMLParser: entity declarations refused, document %d\n", i + 1);
    p = cur = skia_parser();
    report(p, XML_Parse(p, refused[i], (int)strlen(refused[i]), XML_TRUE));
    XML_ParserFree(p);
    p = cur = skia_parser();
    report(p, read_until_end(p, refused[i], 4096));
    XML_ParserFree(p);
  }
}

/* SkFontMgr_android_parser switches the character data handler in flight:
   each element's handler is installed on entry and the parent's restored
   on exit. Only <font> and <alias> elements want text here. */
static void font_text(void *u, const XML_Char *t, int len) { printf("  font-text ["); chars(t, len); printf("]"); at(); }
static void font_start(void *u, const XML_Char *n, const XML_Char **a) {
  start(u, n, a);
  XML_SetCharacterDataHandler(cur, strcmp(n, "font") == 0 ? font_text : NULL);
}
static void font_end(void *u, const XML_Char *n) {
  end(u, n);
  XML_SetCharacterDataHandler(cur, NULL);
}

static const char fonts[] =
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
    "<familyset version=\"23\">\n"
    "    <!-- first font is default -->\n"
    "    <family name=\"sans-serif\">\n"
    "        <font weight=\"100\" style=\"normal\">Roboto-Regular.ttf\n"
    "          <axis tag=\"ital\" stylevalue=\"0\" />\n"
    "          <axis tag=\"wdth\" stylevalue=\"100\" />\n"
    "        </font>\n"
    "        <font weight=\"400\" style=\"italic\" index=\"1\">Roboto-Italic.ttf</font>\n"
    "    </family>\n"
    "    <alias name=\"arial\" to=\"sans-serif\" />\n"
    "    <family lang=\"und-Arab\" variant=\"elegant\">\n"
    "        <font weight=\"400\" style=\"normal\" postScriptName=\"NotoNaskhArabic\">NotoNaskhArabic-Regular.ttf</font>\n"
    "    </family>\n"
    "</familyset>\n";

static void skia_fonts(void) {
  int sizes[] = {512, 5};
  for (int i = 0; i < 2; i++) {
    /* 512 bytes, or 512 - 507 in debug builds to catch sliced text. */
    printf("== Skia font configuration, %d-byte buffers\n", sizes[i]);
    XML_Parser p = cur = XML_ParserCreate_MM(NULL, &sk_XML_alloc, NULL);
    XML_SetUserData(p, marker);
    XML_SetEntityDeclHandler(p, sk_entity_decl);
    XML_SetElementHandler(p, font_start, font_end);
    report(p, read_until_end(p, fonts, sizes[i]));
    XML_ParserFree(p);
  }
  printf("== Skia font configuration with an entity declaration\n");
  const char *doc = "<?xml version=\"1.0\"?>\n<!DOCTYPE familyset [<!ENTITY x \"y\">]>\n<familyset>&x;</familyset>";
  XML_Parser p = cur = XML_ParserCreate_MM(NULL, &sk_XML_alloc, NULL);
  XML_SetEntityDeclHandler(p, sk_entity_decl);
  XML_SetElementHandler(p, font_start, font_end);
  report(p, read_until_end(p, doc, 5));
  XML_ParserFree(p);
}

/* ---- frameworks/av: MediaProfiles.cpp, MediaCodecsXmlParser.cpp ---- */

static const char profiles[] =
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
    "<!-- Copyright statement and licence text, long enough that the document spans several 512-byte buffers.\n"
    "     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore. -->\n"
    "<MediaSettings>\n"
    "    <CamcorderProfiles cameraId=\"0\">\n"
    "        <EncoderProfile quality=\"high\" fileFormat=\"mp4\" duration=\"30\">\n"
    "            <Video codec=\"h264\" bitRate=\"12000000\" width=\"1920\" height=\"1080\" frameRate=\"30\" />\n"
    "            <Audio codec=\"aac\" bitRate=\"96000\" sampleRate=\"48000\" channels=\"1\" />\n"
    "        </EncoderProfile>\n"
    "        <EncoderProfile quality=\"low\" fileFormat=\"3gp\" duration=\"30\">\n"
    "            <Video codec=\"m4v\" bitRate=\"192000\" width=\"176\" height=\"144\" frameRate=\"30\" />\n"
    "            <Audio codec=\"amrnb\" bitRate=\"12200\" sampleRate=\"8000\" channels=\"1\" />\n"
    "        </EncoderProfile>\n"
    "        <ImageEncoding quality=\"95\" />\n"
    "        <ImageDecoding memCap=\"20000000\" />\n"
    "    </CamcorderProfiles>\n"
    "    <EncoderOutputFileFormat name=\"3gp\" />\n"
    "    <EncoderOutputFileFormat name=\"mp4\" />\n"
    "    <VideoEncoderCap name=\"h264\" enabled=\"true\" minBitRate=\"64000\" maxBitRate=\"40000000\"\n"
    "        minFrameWidth=\"176\" maxFrameWidth=\"3840\" minFrameHeight=\"144\" maxFrameHeight=\"2160\"\n"
    "        minFrameRate=\"15\" maxFrameRate=\"60\" />\n"
    "    <AudioDecoderCap name=\"wma\" enabled=\"false\" />\n"
    "</MediaSettings>\n";

static void media_profiles(void) {
  printf("== MediaProfiles: start handler only, 512-byte buffers\n");
  XML_Parser p = cur = XML_ParserCreate(NULL);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, NULL);
  report(p, read_until_empty(p, profiles, 512));
  XML_ParserFree(p);
}

static const char codecs_extra[] =
    "<Included>\n"
    "    <MediaCodec name=\"c2.android.opus.decoder\" type=\"audio/opus\">\n"
    "        <Limit name=\"channel-count\" max=\"8\" />\n"
    "    </MediaCodec>\n"
    "</Included>\n";

static const char codecs[] =
    "<?xml version=\"1.0\" encoding=\"utf-8\" ?>\n"
    "<MediaCodecs>\n"
    "    <Settings>\n"
    "        <Variant name=\"slow-cpu\" enabled=\"false\" />\n"
    "        <Setting name=\"max-video-encoder-input-buffers\" value=\"12\" />\n"
    "    </Settings>\n"
    "    <Decoders>\n"
    "        <MediaCodec name=\"c2.android.avc.decoder\" type=\"video/avc\" variant=\"!slow-cpu\">\n"
    "            <Alias name=\"OMX.google.h264.decoder\" />\n"
    "            <Limit name=\"size\" min=\"2x2\" max=\"4080x4080\" />\n"
    "            <Limit name=\"alignment\" value=\"2x2\" />\n"
    "            <Feature name=\"adaptive-playback\" />\n"
    "        </MediaCodec>\n"
    "        <Include href=\"media_codecs_extra.xml\" />\n"
    "    </Decoders>\n"
    "    <Encoders>\n"
    "        <MediaCodec name=\"c2.android.aac.encoder\" type=\"audio/mp4a-latm\">\n"
    "            <Limit name=\"bitrate\" range=\"8000-960000\" />\n"
    "        </MediaCodec>\n"
    "    </Encoders>\n"
    "</MediaCodecs>\n";

static void codecs_parse(const char *doc);

/* <Include href=...> parses another file with a new parser, from inside
   this parser's start handler. */
static void codecs_start(void *u, const XML_Char *n, const XML_Char **a) {
  start(u, n, a);
  if (strcmp(n, "Include") == 0) {
    XML_Parser outer = cur;
    printf("  including %s\n", a[1]);
    codecs_parse(codecs_extra);
    cur = outer;
    printf("  back in the including file");
    at();
  }
}

static void codecs_parse(const char *doc) {
  XML_Parser p = cur = XML_ParserCreate(NULL);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, codecs_start, end);
  enum XML_Status st = read_until_empty(p, doc, 512);
  if (st != XML_STATUS_OK) printf("  malformed (%s)\n", XML_ErrorString(XML_GetErrorCode(p)));
  report(p, st);
  XML_ParserFree(p);
}

static void media_codecs(void) {
  printf("== MediaCodecsXmlParser: an included file parsed from inside a handler\n");
  codecs_parse(codecs);
  printf("== MediaCodecsXmlParser: malformed\n");
  codecs_parse("<MediaCodecs>\n  <Decoders>\n    <MediaCodec name=\"x\">\n  </Decoders>\n</MediaCodecs>\n");
  printf("== MediaCodecsXmlParser: empty file\n");
  codecs_parse("");
}

/* ---- system/media audio_route/audio_route.c ---- */

static void audio_route(void) {
  const char *mixer =
      "<mixer>\n"
      "    <!-- initial mixer controls -->\n"
      "    <ctl name=\"RX1 MIX1 INP1\" value=\"RX1\" />\n"
      "    <ctl name=\"SPK DRV Volume\" value=\"0\" />\n"
      "    <ctl name=\"IIR1 Band1\" id =\"0\" value=\"268435456\" />\n"
      "    <path name=\"speaker\">\n"
      "        <ctl name=\"SPK DRV Volume\" value=\"8\" />\n"
      "        <path name=\"speaker-common\" />\n"
      "    </path>\n"
      "</mixer>\n";
  printf("== audio_route: mixer paths, 1024-byte buffers\n");
  XML_Parser p = cur = XML_ParserCreate(NULL);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, end);
  report(p, read_until_empty(p, mixer, 1024));
  XML_ParserFree(p);

  printf("== audio_route: malformed\n");
  p = cur = XML_ParserCreate(NULL);
  XML_SetElementHandler(p, start, end);
  report(p, read_until_empty(p, "<mixer>\n  <ctl name=\"a\" value=\"1\">\n</mixer>\n", 1024));
  XML_ParserFree(p);
}

/* ---- external/wayland src/scanner.c ---- */

static void wayland(void) {
  const char *protocol =
      "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
      "<protocol name=\"wayland\">\n"
      "  <copyright>\n"
      "    Copyright \xc2\xa9 2008-2011 Kristian H\xc3\xb8gsberg\n"
      "  </copyright>\n"
      "  <interface name=\"wl_display\" version=\"1\">\n"
      "    <description summary=\"core global object\">\n"
      "\tThe core global object.  This is a special singleton object.  It\n"
      "\tis used for internal Wayland protocol features.\n"
      "    </description>\n"
      "    <request name=\"sync\">\n"
      "      <arg name=\"callback\" type=\"new_id\" interface=\"wl_callback\"\n"
      "\t   summary=\"callback object for the sync request\"/>\n"
      "    </request>\n"
      "    <enum name=\"error\">\n"
      "      <entry name=\"invalid_object\" value=\"0\" summary=\"server couldn't find object\"/>\n"
      "    </enum>\n"
      "  </interface>\n"
      "</protocol>\n";
  printf("== wayland scanner: a protocol, 4096-byte buffers\n");
  XML_Parser p = cur = XML_ParserCreate(NULL);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  report(p, read_until_empty(p, protocol, 4096));
  XML_ParserFree(p);

  printf("== wayland scanner: malformed\n");
  p = cur = XML_ParserCreate(NULL);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, end);
  XML_SetCharacterDataHandler(p, text);
  report(p, read_until_empty(p, "<protocol name=\"x\">\n  <interface name=\"y\" version=\"1\">\n</protocol>\n", 4096));
  XML_ParserFree(p);
}

/* ---- aapt2: tools/aapt2/xml/XmlDom.cpp, XmlPullParser.cpp; aapt: XMLNode.cpp ---- */

static const char layout[] =
    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
    "<!-- Copyright (C) 2024 -->\n"
    "<LinearLayout xmlns:android=\"http://schemas.android.com/apk/res/android\"\n"
    "    xmlns:app=\"http://schemas.android.com/apk/res-auto\"\n"
    "    android:layout_width=\"match_parent\"\n"
    "    android:orientation=\"vertical\">\n"
    "    <TextView android:id=\"@+id/title\" android:text=\"@string/title\" app:custom=\"1\" />\n"
    "    <string name=\"greeting\" xmlns:xliff=\"urn:oasis:names:tc:xliff:document:1.2\">Hello, "
    "<xliff:g id=\"name\">%1$s</xliff:g>!\n    Second line &amp; more</string>\n"
    "    <string name=\"raw\"><![CDATA[<b>bold</b>]]></string>\n"
    "</LinearLayout>\n";

/* XML_UseParserAsHandlerArg: each handler gets the parser, and reads the
   real user data back with XML_GetUserData. */
static void dom_check(void *arg) {
  printf(" [arg is parser: %s, user-data %s]", arg == cur ? "yes" : "no",
         XML_GetUserData((XML_Parser)arg) == marker ? "kept" : "lost");
}
static void dom_start(void *arg, const XML_Char *n, const XML_Char **a) { dom_check(arg); start(arg, n, a); }
static void dom_end(void *arg, const XML_Char *n) { dom_check(arg); end(arg, n); }
static void dom_sns(void *arg, const XML_Char *prefix, const XML_Char *uri) { dom_check(arg); sns(arg, prefix, uri); }
static void dom_ens(void *arg, const XML_Char *prefix) { dom_check(arg); ens(arg, prefix); }
static void dom_text(void *arg, const XML_Char *t, int len) { dom_check(arg); text(arg, t, len); }
static void dom_comment(void *arg, const XML_Char *d) { dom_check(arg); comment(arg, d); }

static void aapt2(void) {
  size_t pieces[] = {4096, 13};
  for (int i = 0; i < 2; i++) {
    printf("== aapt2 XmlDom: parser as handler argument, %zu-byte pieces\n", pieces[i]);
    XML_Parser p = cur = XML_ParserCreateNS(NULL, '\1');
    XML_SetUserData(p, marker);
    XML_UseParserAsHandlerArg(p);
    XML_SetElementHandler(p, dom_start, dom_end);
    XML_SetNamespaceDeclHandler(p, dom_sns, dom_ens);
    XML_SetCharacterDataHandler(p, dom_text);
    XML_SetCommentHandler(p, dom_comment);
    report(p, pieces_then_final(p, layout, strlen(layout), pieces[i]));
    XML_ParserFree(p);
  }

  for (int i = 0; i < 2; i++) {
    printf("== aapt2 XmlPullParser: %zu-byte pieces, the last final\n", pieces[i]);
    XML_Parser p = cur = XML_ParserCreateNS(NULL, '\1');
    XML_SetUserData(p, marker);
    XML_SetElementHandler(p, start, end);
    XML_SetNamespaceDeclHandler(p, sns, ens);
    XML_SetCharacterDataHandler(p, text);
    XML_SetCommentHandler(p, comment);
    XML_SetCdataSectionHandler(p, scdata, ecdata);
    report(p, pieces_last_final(p, layout, pieces[i]));
    XML_ParserFree(p);
  }

  const char *bad[] = {"<resources>\n  <string name=\"a\">x</strin>\n</resources>\n",
                       "<resources xmlns:a=\"urn:a\">\n  <b:c/>\n</resources>\n", "<resources>\n  <a>\n"};
  for (int i = 0; i < 3; i++) {
    printf("== aapt2: malformed resource %d\n", i + 1);
    XML_Parser p = cur = XML_ParserCreateNS(NULL, '\1');
    XML_SetUserData(p, marker);
    XML_SetElementHandler(p, start, end);
    report(p, pieces_then_final(p, bad[i], strlen(bad[i]), 4096));
    XML_ParserFree(p);
  }

  printf("== aapt XMLNode: the whole file at once\n");
  XML_Parser p = cur = XML_ParserCreateNS(NULL, 1);
  XML_SetUserData(p, marker);
  XML_SetElementHandler(p, start, end);
  XML_SetNamespaceDeclHandler(p, sns, ens);
  XML_SetCharacterDataHandler(p, text);
  XML_SetCommentHandler(p, comment);
  report(p, XML_Parse(p, layout, (int)strlen(layout), XML_TRUE));
  XML_ParserFree(p);
}

int main(void) {
  libcore();
  libcore_utf16();
  skia_svg();
  skia_fonts();
  media_profiles();
  media_codecs();
  audio_route();
  wayland();
  aapt2();
  return 0;
}
