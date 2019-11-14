/*
 * Copyright 2016-present Facebook. All Rights Reserved.
 *
 * linelog.c: data structure tracking line changes
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include "linelog.h"
#include <assert.h> /* assert */
#include <stdbool.h> /* bool */
#include <stdlib.h> /* realloc, free */
#include <string.h> /* NULL, memcpy, memmove, memset */

#if defined(_WIN32) || defined(_WIN64)
/* Windows does not have arpa/inet.h. Reinvent htonl and ntohl. Modern x86
 * compiler could produce efficient "bswap" instruction. */
#define bswap32(x)                                       \
  {                                                      \
    x = ((x << 8) & 0xFF00FF00) | ((x >> 8) & 0xFF00FF); \
    x = (x << 16) | (x >> 16);                           \
  }

inline static int islittleendian(void) {
  uint32_t i = 1;
  return (*((uint8_t*)(&i))) == 1;
}

static uint32_t htonl(uint32_t x) {
  if (islittleendian())
    bswap32(x);
  return x;
}

#define ntohl htonl
#else
#include <arpa/inet.h> /* htonl, ntohl */
#endif

/* linelog_buf.data is a plain array of instructions.

   a linelog instruction has 8 bytes:

     opcode:    2 bits (linelog_opcode)
     operand1: 30 bits (linelog_revnum)
     operand2: 32 bits (linelog_offset | linelog_linenum)

   the first 8-byte slot is not a real instruction, but a 32-bit maxrev then
   a 32-bit instruction count indicating used buffer size. it can be parsed as
   a normal instruction to extract the information. the code usually uses
   "inst0" as the variable name for that purpose.

   real instructions start from the 9th byte. */

typedef enum {
  JGE = 0, /* if rev >= operand1, jump to operand2 */
  JL = 1, /* if rev < operand1, jump to operand2 */
  LINE = 2, /* line introduced by rev = operand1, linenum = operand2 */
} linelog_opcode;

typedef struct {
  linelog_opcode opcode;
  linelog_revnum rev; /* uint32_t operand1 */
  linelog_offset offset; /* uint32_t operand2, linelog_linenum linenum */
} linelog_inst;

/* static assert uint32_t, linelog_{linenum,revnum,offset} have a same size */
extern char linelog_assert_[1 / (sizeof(linelog_revnum) == sizeof(uint32_t))];
extern char linelog_assert_[1 / (sizeof(linelog_linenum) == sizeof(uint32_t))];
extern char linelog_assert_[1 / (sizeof(linelog_offset) == sizeof(uint32_t))];

/* size of the encoded representation, not sizeof(linelog_inst) */
#define INST_SIZE 8

/* like linelog_{offset,linenum} but less likely to overflow */
typedef size_t linelog_loffset;
typedef size_t linelog_llinenum;

/* hard limits, smaller than the physical limits to reserve some bits */
#ifndef MIN
#define MIN(x, y) (((x) < (y)) ? (x) : (y))
#endif
static const linelog_loffset MAX_OFFSET =
    MIN(0x0ffffff0u, SIZE_MAX / INST_SIZE);
static const linelog_llinenum MAX_LINENUM =
    MIN(0x1ffffff0u, SIZE_MAX / sizeof(linelog_lineinfo));
static const linelog_revnum MAX_REVNUM = 0x1ffffff0u;

/* uint8_t[8] -> linelog_inst */
static inline void decode(const uint8_t data[INST_SIZE], linelog_inst* inst) {
  uint32_t buf[2];
  memcpy(buf, data, sizeof(buf));
  buf[0] = ntohl(buf[0]);
  buf[1] = ntohl(buf[1]);
  inst->opcode = (linelog_opcode)(buf[0] & 3);
  inst->rev = (buf[0] >> 2) & 0x3fffffffu;
  inst->offset = buf[1];
}

/* uint8_t[8] <- linelog_inst */
static inline void encode(uint8_t data[INST_SIZE], const linelog_inst* inst) {
  uint32_t buf[2];
  buf[0] = htonl((uint32_t)(inst->opcode) | (inst->rev << 2));
  buf[1] = htonl(inst->offset);
  memcpy(data, buf, sizeof(buf));
}

/* read instruction, with error checks */
static inline linelog_result
readinst(const linelog_buf* buf, linelog_inst* inst, linelog_loffset offset) {
  if (buf == NULL || buf->data == NULL || buf->size < INST_SIZE ||
      offset >= MAX_OFFSET)
    return LINELOG_RESULT_EILLDATA;
  size_t len = htonl(((const uint32_t*)buf->data)[1]);
  if (len > buf->size / INST_SIZE || offset >= len)
    return LINELOG_RESULT_EILLDATA;
  size_t offsetinbytes = (size_t)offset * INST_SIZE;
  decode(buf->data + offsetinbytes, inst);
  return LINELOG_RESULT_OK;
}

/* write instruction, with error checks */
static inline linelog_result
writeinst(linelog_buf* buf, const linelog_inst* inst, linelog_loffset offset) {
  if (offset >= MAX_OFFSET)
    return LINELOG_RESULT_EOVERFLOW;
  if (buf == NULL || (buf->data == NULL && buf->size > 0))
    return LINELOG_RESULT_EILLDATA;
  size_t offsetinbytes = (size_t)offset * INST_SIZE;
  if (offsetinbytes + INST_SIZE > buf->size) {
    buf->neededsize = offsetinbytes + INST_SIZE;
    return LINELOG_RESULT_ENEEDRESIZE;
  }
  encode(buf->data + offsetinbytes, inst);
  return LINELOG_RESULT_OK;
}

/* helper to make code shorter */
#define returnonerror(expr)          \
  {                                  \
    linelog_result result = (expr);  \
    if (result != LINELOG_RESULT_OK) \
      return result;                 \
  }
#define mustsuccess(expr)                                          \
  {                                                                \
    linelog_result result = (expr);                                \
    (void)result; /* eliminate "unused" warning with NDEBUG set */ \
    assert(result == LINELOG_RESULT_OK);                           \
  }

/* ensure `ar->lines[0:linecount]` are valid */
static linelog_result reservelines(
    linelog_annotateresult* ar,
    linelog_llinenum linecount) {
  if (linecount >= MAX_LINENUM)
    return LINELOG_RESULT_EOVERFLOW;
  if (ar->maxlinecount < linecount) {
    size_t size = sizeof(linelog_lineinfo) * linecount;
    void* p = realloc(ar->lines, size);
    if (p == NULL)
      return LINELOG_RESULT_ENOMEM;
    ar->lines = (linelog_lineinfo*)p;
    ar->maxlinecount = (linelog_linenum)linecount;
  }
  return LINELOG_RESULT_OK;
}

/* APIs declared in .h */

void linelog_annotateresult_clear(linelog_annotateresult* ar) {
  free(ar->lines);
  memset(ar, 0, sizeof(linelog_annotateresult));
}

linelog_result linelog_clear(linelog_buf* buf) {
  linelog_inst insts[2] = {{JGE, 0, 2}, {JGE, 0, 0}};
  returnonerror(writeinst(buf, &insts[1], 1));
  returnonerror(writeinst(buf, &insts[0], 0));
  return LINELOG_RESULT_OK;
}

size_t linelog_getactualsize(const linelog_buf* buf) {
  linelog_inst inst0;
  linelog_result r = readinst(buf, &inst0, 0);
  if (r != LINELOG_RESULT_OK)
    return 0;
  return (size_t)(inst0.offset) * INST_SIZE;
}

linelog_revnum linelog_getmaxrev(const linelog_buf* buf) {
  linelog_inst inst0;
  linelog_result r = readinst(buf, &inst0, 0);
  if (r != LINELOG_RESULT_OK)
    return 0;
  return inst0.rev;
}

inline static linelog_result appendline(
    linelog_annotateresult* ar,
    const linelog_inst* inst,
    linelog_offset offset) {
  linelog_lineinfo info = {.rev = inst ? inst->rev : 0,
                           .linenum = inst ? inst->offset /* linenum */ : 0,
                           .offset = offset};
  returnonerror(reservelines(ar, ar->linecount + 1));
  ar->lines[ar->linecount++] = info;
  return LINELOG_RESULT_OK;
}

linelog_result linelog_annotate(
    const linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum rev) {
  linelog_inst inst0;
  returnonerror(readinst(buf, &inst0, 0));

  linelog_offset pc, nextpc = 1, endoffset = 0;
  ar->linecount = 0;
  size_t step = (size_t)inst0.offset;

  while ((pc = nextpc++) != 0 && --step) {
    linelog_inst i;
    returnonerror(readinst(buf, &i, pc));

    switch (i.opcode) {
      case JGE:
      case JL: /* conditional jump */
        if (i.opcode == JGE ? rev >= i.rev : rev < i.rev) {
          nextpc = i.offset;
          if (nextpc == 0) /* met the END marker */
            endoffset = pc;
        }
        break;
      case LINE: /* append a line */
        returnonerror(appendline(ar, &i, pc));
        break;
      default: /* unknown opcode */
        return LINELOG_RESULT_EILLDATA;
    }
  }

  if (endoffset == 0) /* didn't meet a valid END marker */
    return LINELOG_RESULT_EILLDATA;

  /* ar->lines[ar->linecount].offset records the endoffset */
  returnonerror(appendline(ar, NULL, endoffset));
  ar->linecount--; /* do not include this special line */
  return LINELOG_RESULT_OK;
}

static linelog_result replacelines(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum brev,
    linelog_linenum a1,
    linelog_linenum a2,
    linelog_linenum b1,
    linelog_linenum b2,
    const linelog_revnum* brevs,
    const linelog_linenum* blinenums) {
  /*       buf   before     after
                 --------   --------
                 ....       ....
        a1addr > (a1inst)   JGE     0 oldlen   [5]
      a1addr+1 > ...        ...
                 ....       ....
        a2addr > ...        ...
                 ....       ....
        oldlen > (end)      JL   brev pjge     [1]
                            LINE brev b1       [1]
                            LINE brev b1+1     [1]
                            ....               [1]
                            LINE brev b2-1     [1]
          pjge >            JGE  brev a2addr   [2]
     a1newaddr >            (a1inst)           [3]
                            JGE     0 a1addr+1 [4]
        newlen >            (end)

     [1]: insert new lines. only exist if b1 < b2
     [2]: delete old lines. only exist if a1 < a2
     [3]: move a1inst to new place, as it will be rewritten in [5]
     [4]: jump back. only exist if a1inst is not an unconditional jump
     [5]: rewrite the old instruction to jump to the new block */

  /* sanity check */
  linelog_inst inst0;
  returnonerror(readinst(buf, &inst0, 0));
  if (brev >= MAX_REVNUM || a2 >= MAX_LINENUM || b2 >= MAX_LINENUM)
    return LINELOG_RESULT_EOVERFLOW;
  if (a2 < a1 || b2 < b1 || !ar || a2 > ar->linecount || brev == 0 ||
      ar->linecount >= ar->maxlinecount)
    return LINELOG_RESULT_EILLDATA;

  /* useful variables for both step I and III */
  linelog_offset oldlen = inst0.offset;
  linelog_offset a1addr = ar->lines[a1].offset;
  linelog_inst a1inst;
  returnonerror(readinst(buf, &a1inst, a1addr));
  bool a1instisjge0 = (a1inst.opcode == JGE && a1inst.rev == 0);

  /* step I: reserve size for buf: (newlen - oldlen) more instructions */
  linelog_loffset newlen = (linelog_loffset)oldlen +
      (b2 - b1 /* LINE */ + (b2 > b1) /* JL brev */) /* [1] */
      + (a2 > a1) /* JGE brev */ /* [2] */
      + 1 /* a1inst */ /* [3] */
      + (a1instisjge0 ? 0 : 1) /* JGE 0  */ /* [4] */;
  if (newlen >= MAX_OFFSET)
    return LINELOG_RESULT_EOVERFLOW;
  size_t neededsize = (size_t)newlen * INST_SIZE;
  if (neededsize > buf->size) {
    buf->neededsize = neededsize;
    return LINELOG_RESULT_ENEEDRESIZE;
  }

  /* step II: reserve space for annotateresult */
  linelog_llinenum newlinecount =
      (linelog_llinenum)ar->linecount + b2 - b1 - (a2 - a1);
  returnonerror(reservelines(ar, newlinecount + 1));
  assert(ar->linecount < ar->maxlinecount);

/* writeinst should not fail for remaining steps - we have reserved
   enough space. any failure will be a huge headache for the caller. */

/* step III: update linelog_buf */
#define appendinst(inst) mustsuccess(writeinst(buf, &inst, inst0.offset++));
  if (b1 < b2) { /* [1] */
    linelog_offset pjge = oldlen + (b2 - b1 + 1);
    linelog_inst jl = {.opcode = JL, .rev = brev, .offset = pjge};
    appendinst(jl);
    for (linelog_linenum i = b1; i < b2; ++i) {
      linelog_inst lineinst = {
          .opcode = LINE,
          .rev = brevs ? brevs[i] : brev,
          .offset /* linenum */ = blinenums ? blinenums[i] : i};
      appendinst(lineinst);
    }
  }
  if (a1 < a2) { /* [2] */
    linelog_offset a2addr = ar->lines[a2].offset;
    /* delete a chunk of an old commit. be conservative, do not
       touch invisible lines between a2 - 1 and a2 */
    if (a2 > 0 && brev < inst0.rev /* maxrev */)
      a2addr = ar->lines[a2 - 1].offset + 1;
    linelog_inst jge = {.opcode = JGE, .rev = brev, .offset = a2addr};
    appendinst(jge);
  }
  linelog_offset a1newaddr = inst0.offset;
  appendinst(a1inst); /* [3] */
  if (!a1instisjge0) { /* [4] */
    linelog_inst ret = {/* .opcode = */ JGE,
                        0,
                        /* .offset = */ a1addr + 1};
    appendinst(ret);
  }
#undef appendinst
  linelog_inst jge0 = {.opcode = JGE, .rev = 0, .offset = oldlen};
  mustsuccess(writeinst(buf, &jge0, a1addr)); /* [5] */

  /* step IV: write back updated inst0 */
  if (brev > inst0.rev)
    inst0.rev = brev;
  mustsuccess(writeinst(buf, &inst0, 0));

  /* step V: update annotateresult */
  ar->lines[a1].offset = a1newaddr; /* a1inst got moved */
  if (a2 - a1 != b2 - b1) {
    size_t movesize = sizeof(linelog_lineinfo) * (ar->linecount + 1 - a2);
    /* the memmove is safe as step II reserved the memory */
    memmove(ar->lines + a1 + b2 - b1, ar->lines + a2, movesize);
    ar->linecount = (linelog_linenum)newlinecount;
  }
  for (linelog_linenum i = b1; i < b2; ++i) {
    linelog_lineinfo* li = ar->lines + a1 + i - b1;
    li->rev = brevs ? brevs[i] : brev;
    li->linenum = blinenums ? blinenums[i] : i;
    li->offset = oldlen + i - b1 + 1;
  }

  return LINELOG_RESULT_OK;
}

linelog_result linelog_replacelines(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum brev,
    linelog_linenum a1,
    linelog_linenum a2,
    linelog_linenum b1,
    linelog_linenum b2) {
  return replacelines(buf, ar, brev, a1, a2, b1, b2, NULL, NULL);
}

linelog_result linelog_replacelines_vec(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum brev,
    linelog_linenum a1,
    linelog_linenum a2,
    linelog_linenum blinecount,
    const linelog_revnum* brevs,
    const linelog_linenum* blinenums) {
  return replacelines(buf, ar, brev, a1, a2, 0, blinecount, brevs, blinenums);
}

linelog_result linelog_getalllines(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_offset offset1,
    linelog_offset offset2) {
  linelog_inst inst0;
  returnonerror(readinst(buf, &inst0, 0));

  linelog_offset pc, nextpc = offset1 ? offset1 : 1;
  ar->linecount = 0;

  for (linelog_offset step = inst0.offset; step; --step) {
    pc = nextpc++;
    if (pc == offset2 || pc == 0)
      return LINELOG_RESULT_OK;

    linelog_inst i;
    returnonerror(readinst(buf, &i, pc));

    switch (i.opcode) {
      case JGE:
        if (i.rev == 0) /* unconditional jump */
          nextpc = i.offset;
        break;
      case JL:
        break;
      case LINE: /* append a line */
        returnonerror(appendline(ar, &i, pc));
        break;
      default: /* unknown opcode */
        return LINELOG_RESULT_EILLDATA;
    }
  }

  /* step reaches 0, didn't meet the end condition */
  return LINELOG_RESULT_EILLDATA;
}
