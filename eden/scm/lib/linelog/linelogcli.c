/*
 * Copyright 2016-present Facebook. All Rights Reserved.
 *
 * linelogcli.c: a simple CLI tool manipulating a linelog file
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

/* this tool is mainly for testing and debugging purpose. it does not have
   proper error handling and is not very user-friendly. */

#ifndef _XOPEN_SOURCE
#define _XOPEN_SOURCE 500 /* ftruncate */
#endif

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include "linelog.c" /* unusual but we want to access some private structs */

#define ensure(expr)                                   \
  if (!(expr)) {                                       \
    fprintf(                                           \
        stderr,                                        \
        "unexpected: %s\n at line %d\n errno = %d %s", \
        #expr,                                         \
        __LINE__,                                      \
        errno,                                         \
        strerror(errno));                              \
    closefile();                                       \
    exit(-1);                                          \
  }

#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
const size_t UNIT_SIZE = 1;
#else
const size_t UNIT_SIZE = 0x1000; /* 4KB, used when resizing the file */
#endif

static linelog_buf buf;
static linelog_annotateresult ar;
static int fd = -1;
static size_t maplen;
static const char* filename;

static const char helptext[] =
#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
    "(built for fuzz testing)\n"
#endif
    "usage: linelogcli FILE CMDLIST\n"
    "where  CMDLIST := CMD | CMDLIST CMD\n"
    "       CMD := init | info | dump | ANNOTATECMD | REPLACELINESCMD | "
    "GETALLLINESCMD\n"
    "       ANNOTATECMD := annotate REV | annotate -\n"
    "       REPLACELINESCMD := replacelines rev a1:a2 b1:b2\n"
    "       GETALLLINESCMD := getalllines offset1:offset2\n";

static void closefile(void) {
  if (buf.data) {
    ensure(msync(buf.data, buf.size, MS_ASYNC) == 0);
    ensure(munmap(buf.data, maplen) == 0);
    buf.data = NULL;
    buf.size = 0;
  }
  if (fd != -1) {
    ensure(close(fd) == 0);
    fd = -1;
  }
}

static void openfile(void) {
  closefile();
  ensure((fd = open(filename, O_RDWR | O_CREAT, 0644)) != -1);

  struct stat st;
  ensure(fstat(fd, &st) == 0);

  maplen = (st.st_size == 0 ? 1 : (size_t)st.st_size);
  void* p = mmap(NULL, maplen, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  ensure(p != MAP_FAILED);

  buf.data = p;
  buf.size = (size_t)st.st_size;
}

static void resizefile(size_t size) {
  closefile();
  ensure((fd = open(filename, O_RDWR | O_CREAT, 0644)) != -1);
  ensure(ftruncate(fd, (off_t)size) == 0);
  openfile();
}

/* handle LINELOG_RESULT_ENEEDRESIZE automatically */
#define eval(result, expr)                                    \
  while (1) {                                                 \
    result = (expr);                                          \
    if (result != LINELOG_RESULT_ENEEDRESIZE)                 \
      break;                                                  \
    resizefile((buf.neededsize / UNIT_SIZE + 1) * UNIT_SIZE); \
  }

int cmdinit(const char* args[]) {
  (void)args;
  linelog_result r;
  eval(r, linelog_clear(&buf));
  if (r == LINELOG_RESULT_OK)
    printf("init: okay\n");
  return r;
}

int cmdinfo(const char* args[]) {
  (void)args;
  size_t size = linelog_getactualsize(&buf);
  linelog_revnum rev = linelog_getmaxrev(&buf);
  printf("info: maxrev = %u, size = %lu\n", (unsigned)rev, (unsigned long)size);
  return 0;
}

int cmdannotate(const char* args[]) {
  linelog_result r = LINELOG_RESULT_OK;
  unsigned rev = 0;
  if (sscanf(args[0], "%u", &rev) == 1) {
    printf("annotate: run annotate for rev %u\n", rev);
    eval(r, linelog_annotate(&buf, &ar, (linelog_revnum)rev));
  }
  if (r == LINELOG_RESULT_OK) {
    printf(
        "annotate: %u lines, endoffset %u\n",
        ar.linecount,
        ar.lines[ar.linecount].offset);
    for (uint32_t i = 0; i < ar.linecount; ++i) {
      linelog_lineinfo l = ar.lines[i];
      printf(
          "  %u: rev %u, line %u, offset %u\n", i, l.rev, l.linenum, l.offset);
    }
  }
  return r;
}

#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
static void doublecheckannotateresult(linelog_revnum rev) {
  /* backup current ar for later comparison */
  linelog_annotateresult ar2 = ar;
  size_t arsize = (ar.linecount + 1) * sizeof(linelog_lineinfo);
  ar2.lines = malloc(arsize);
  ar2.maxlinecount = ar2.linecount + 1;
  ensure(ar2.lines != NULL);
  memcpy(ar2.lines, ar.lines, arsize);
  linelog_result r;
  eval(r, linelog_annotate(&buf, &ar2, rev));
  if (r != LINELOG_RESULT_OK ||
      (ar.linecount == ar2.linecount &&
       memcmp(ar2.lines, ar.lines, arsize) == 0)) {
    free(ar2.lines);
    return;
  }
  fprintf(stderr, "unexpected: annotate results mismatch\n");
  int cmddump(const char* args[]);
  cmddump(NULL);

  linelog_linenum maxlc =
      ar.linecount > ar2.linecount ? ar.linecount : ar2.linecount;
  fprintf(stderr, "ar %d lines | ar2 %d lines\n", ar.linecount, ar2.linecount);
  for (uint32_t i = 0; i <= maxlc; ++i) {
    linelog_lineinfo l[2];
    memset(l, -1, sizeof(l));
    if (i <= ar.linecount)
      l[0] = ar.lines[i];
    if (i <= ar2.linecount)
      l[1] = ar2.lines[i];
    char ch = memcmp(l, l + 1, sizeof(l[0])) ? '!' : '=';
    fprintf(
        stderr,
        "%c %u: %u %u %u | %u %u %u\n",
        ch,
        i,
        l[0].rev,
        l[0].linenum,
        l[0].offset,
        l[1].rev,
        l[1].linenum,
        l[1].offset);
  }
  abort();
}
#endif

int cmdreplacelines(const char* args[]) {
  linelog_result r;
  unsigned rev = 0, b1 = 0, b2 = 0;
  int a1 = 0, a2 = 0;
  sscanf(args[0], "%u", &rev);
  sscanf(args[1], "%d:%d", &a1, &a2);
  sscanf(args[2], "%u:%u", &b1, &b2);
  /* for negative number of a1, a2. use linecount automatically */
  if (a1 < 0)
    a1 = (int)ar.linecount + 1 + a1;
  if (a2 < 0)
    a2 = (int)ar.linecount + 1 + a2;
#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
  /* make sure we use clean, up-to-date annotate result. this changes
     behavior a bit but reduces noise from fuzz testing */
  eval(r, linelog_annotate(&buf, &ar, rev));
  if (r != LINELOG_RESULT_OK)
    return r;
#endif
  eval(
      r,
      linelog_replacelines(&buf, &ar, rev, (uint32_t)a1, (uint32_t)a2, b1, b2));
  if (r == LINELOG_RESULT_OK) {
    printf("replacelines: rev %u, lines %u:%u -> %u:%u\n", rev, a1, a2, b1, b2);
#ifdef FUZZING_BUILD_MODE_UNSAFE_FOR_PRODUCTION
    /* annotateresult updated by linelog_replacelines should be
       the same with running linelog_annotate directly */
    doublecheckannotateresult(rev);
#endif
  }
  return r;
}

int cmddump(const char* args[]) {
  (void)args;
  size_t size = linelog_getactualsize(&buf) / INST_SIZE;
  printf("dump:\n");
  for (size_t offset = 1; offset < size; ++offset) {
    linelog_inst i;
    memset(&i, 0, sizeof(i));
    readinst(&buf, &i, offset);
    /* opcode */
    const char* opname = "?";
    if (i.opcode == JGE) {
      if (i.rev == 0) {
        if (i.offset == 0) /* JGE 0 0 => END */
          opname = "END";
        else /* JGE 0 => J */
          opname = "J";
      } else {
        opname = "JGE";
      }
    } else if (i.opcode == JL) {
      opname = "JL";
    } else if (i.opcode == LINE) {
      opname = "LINE";
    }
    printf("  %6u: %-4s ", (unsigned)offset, opname);
    /* operand 1 */
    if (i.rev) {
      printf("%5u ", (unsigned)i.rev);
    } else {
      printf("      ");
    }
    /* operand 2 */
    if (opname[0] == 'E') { /* END */
      printf("\n");
    } else {
      printf("%u\n", (unsigned)i.offset);
    }
  }
  return LINELOG_RESULT_OK;
}

int cmdgetalllines(const char* args[]) {
  unsigned offset1 = 0, offset2 = 0;
  sscanf(args[0], "%u:%u", &offset1, &offset2);

  linelog_result r;
  linelog_annotateresult ar;
  memset(&ar, 0, sizeof(ar));
  eval(r, linelog_getalllines(&buf, &ar, offset1, offset2));
  if (r == LINELOG_RESULT_OK) {
    printf("getalllines: %u lines\n", ar.linecount);
    for (uint32_t i = 0; i < ar.linecount; ++i) {
      linelog_lineinfo l = ar.lines[i];
      printf(
          "  %u: rev %u, line %u, offset %u\n", i, l.rev, l.linenum, l.offset);
    }
  }
  linelog_annotateresult_clear(&ar);
  return r;
}

typedef int cmdfunc(const char* args[]);
typedef struct {
  const char* name;
  const char shortname;
  int argcount;
  cmdfunc* func;
} cmdentry;

static cmdentry cmdtable[] = {
    {"init", 'i', 0, cmdinit},
    {"info", 'f', 0, cmdinfo},
    {"annotate", 'a', 1, cmdannotate},
    {"replacelines", 'r', 3, cmdreplacelines},
    {"dump", 'd', 0, cmddump},
    {"getalllines", 'l', 1, cmdgetalllines},
};

const cmdentry* findcmd(const char* name) {
  size_t len = strlen(name);
  for (size_t i = 0; i < sizeof(cmdtable) / sizeof(cmdtable[0]); ++i) {
    if (len == 1 ? name[0] == cmdtable[i].shortname
                 : strcmp(name, cmdtable[i].name) == 0)
      return &cmdtable[i];
  }
  return NULL;
}

const char* translateerror(linelog_result result) {
  switch (result) {
    case LINELOG_RESULT_ENOMEM:
      return "NOMEM";
    case LINELOG_RESULT_EILLDATA:
      return "ILLDATA";
    case LINELOG_RESULT_EOVERFLOW:
      return "OVERFLOW";
  }
  return "(unknown)";
}

int main(int argc, char const* argv[]) {
  if (argc < 3) {
    puts(helptext);
    return 1;
  }

  filename = argv[1];
  openfile();
  linelog_annotateresult_clear(&ar);

  for (int i = 2; i < argc; i++) {
    const cmdentry* cmd = findcmd(argv[i]);
    if (!cmd) {
      fprintf(stderr, "%s: unknown command\n", argv[i]);
      continue;
    }
    if (argc - i - 1 < cmd->argcount) {
      fprintf(stderr, "%s: missing argument\n", argv[i++]);
      break;
    }
    linelog_result r = cmd->func(argv + i + 1);
    if (r != LINELOG_RESULT_OK)
      fprintf(
          stderr, "%s: error %d (%s)\n", cmd->name, (int)r, translateerror(r));
    i += cmd->argcount;
  }

  /* truncate the file to actual used size */
  size_t size = linelog_getactualsize(&buf);
  if (size && size != buf.size)
    resizefile(size);

  closefile();
  linelog_annotateresult_clear(&ar);
  return 0;
}
