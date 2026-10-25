/* Print libexpat's error code, message and position for each file given,
   parsed in one piece; -n first enables namespace processing. Used to
   derive expat-rs's ErrorKind mapping. */
#include <expat.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv) {
  int ns = argc > 1 && argv[1][0] == '-' && argv[1][1] == 'n';
  for (int i = 1 + ns; i < argc; i++) {
    FILE *f = fopen(argv[i], "rb");
    if (!f) { perror(argv[i]); continue; }
    char *buf = malloc(1 << 20);
    size_t n = fread(buf, 1, 1 << 20, f);
    fclose(f);
    XML_Parser p = ns ? XML_ParserCreateNS(NULL, '}') : XML_ParserCreate(NULL);
    if (XML_Parse(p, buf, (int)n, 1) == XML_STATUS_ERROR)
      printf("%s\t%d\t%s\t%lu\t%lu\n", argv[i], XML_GetErrorCode(p), XML_ErrorString(XML_GetErrorCode(p)),
             XML_GetCurrentLineNumber(p), XML_GetCurrentColumnNumber(p));
    else
      printf("%s\t0\tOK\t0\t0\n", argv[i]);
    XML_ParserFree(p);
    free(buf);
  }
  return 0;
}
