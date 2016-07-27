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
#include <stdlib.h> /* realloc, free */
#include <string.h> /* NULL, memcpy, memset */
#include <arpa/inet.h> /* htonl, ntohl */

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
	union {
		uint32_t operand1;
		linelog_revnum rev;
	};
	union {
		uint32_t operand2;
		linelog_linenum linenum;
		linelog_offset offset;
	};
} linelog_inst;

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
static inline void decode(const uint8_t data[INST_SIZE], linelog_inst *inst) {
	uint32_t buf[2];
	memcpy(buf, data, sizeof(buf));
	buf[0] = ntohl(buf[0]);
	buf[1] = ntohl(buf[1]);
	inst->opcode = (linelog_opcode)(buf[0] & 3);
	inst->operand1 = (buf[0] >> 2) & 0x3fffffffu;
	inst->operand2 = buf[1];
}

/* uint8_t[8] <- linelog_inst */
static inline void encode(uint8_t data[INST_SIZE], const linelog_inst *inst) {
	uint32_t buf[2];
	buf[0] = htonl((uint32_t)(inst->opcode) | (inst->operand1 << 2));
	buf[1] = htonl(inst->operand2);
	memcpy(data, buf, sizeof(buf));
}

/* read instruction, with error checks */
static inline linelog_result readinst(const linelog_buf *buf,
		linelog_inst *inst, linelog_loffset offset) {
	if (buf == NULL || buf->data == NULL || buf->size < INST_SIZE
			|| offset >= MAX_OFFSET)
		return LINELOG_RESULT_EILLDATA;
	size_t len = htonl(((const uint32_t *)buf->data)[1]);
	if (len > buf->size / INST_SIZE || offset >= len)
		return LINELOG_RESULT_EILLDATA;
	size_t offsetinbytes = (size_t)offset * INST_SIZE;
	decode(buf->data + offsetinbytes, inst);
	return LINELOG_RESULT_OK;
}

/* write instruction, with error checks */
static inline linelog_result writeinst(linelog_buf *buf,
		const linelog_inst *inst, linelog_loffset offset) {
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
#define returnonerror(expr) { \
	linelog_result result = (expr); \
	if (result != LINELOG_RESULT_OK) \
		return result; \
}

/* ensure `ar->lines[0:linecount]` are valid */
static linelog_result reservelines(linelog_annotateresult *ar,
		linelog_llinenum linecount) {
	if (linecount >= MAX_LINENUM)
		return LINELOG_RESULT_EOVERFLOW;
	if (ar->maxlinecount < linecount) {
		size_t size = sizeof(linelog_lineinfo) * linecount;
		void *p = realloc(ar->lines, size);
		if (p == NULL)
			return LINELOG_RESULT_ENOMEM;
		ar->lines = (linelog_lineinfo *)p;
		ar->maxlinecount = (linelog_linenum)linecount;
	}
	return LINELOG_RESULT_OK;
}

/* APIs declared in .h */

void linelog_annotateresult_clear(linelog_annotateresult *ar) {
	free(ar->lines);
	memset(ar, 0, sizeof(linelog_annotateresult));
}

linelog_result linelog_clear(linelog_buf *buf) {
	linelog_inst insts[2] = { { .offset = 2 }, { .offset = 0 } };
	returnonerror(writeinst(buf, &insts[1], 1));
	returnonerror(writeinst(buf, &insts[0], 0));
	return LINELOG_RESULT_OK;
}

size_t linelog_getactualsize(const linelog_buf *buf) {
	linelog_inst inst0;
	linelog_result r = readinst(buf, &inst0, 0);
	if (r != LINELOG_RESULT_OK)
		return 0;
	return (size_t)(inst0.offset) * INST_SIZE;
}

linelog_revnum linelog_getmaxrev(const linelog_buf *buf) {
	linelog_inst inst0;
	linelog_result r = readinst(buf, &inst0, 0);
	if (r != LINELOG_RESULT_OK)
		return 0;
	return inst0.rev;
}

linelog_result linelog_annotate(const linelog_buf *buf,
		 linelog_annotateresult *ar, linelog_revnum rev) {
	linelog_inst inst0;
	returnonerror(readinst(buf, &inst0, 0));

	linelog_offset pc, nextpc = 1, endoffset = 0;
	linelog_linenum linenum = 0;
	size_t step = (size_t)inst0.offset;

	while ((pc = nextpc++) != 0 && --step) {
		linelog_inst i;
		returnonerror(readinst(buf, &i, pc));

		switch (i.opcode) {
		case JGE: case JL: /* conditional jump */
			if (i.opcode == JGE ? rev >= i.rev : rev < i.rev) {
				nextpc = i.offset;
				if (nextpc == 0) /* met the END marker */
					endoffset = pc;
			}
			break;
		case LINE: /* append a line */
			{
				linelog_lineinfo info = {i.rev, i.linenum, pc};
				returnonerror(reservelines(ar, linenum + 1));
				ar->lines[linenum++] = info;
			}
			break;
		default: /* unknown opcode */
			return LINELOG_RESULT_EILLDATA;
		}
	}

	if (endoffset == 0) /* didn't meet a valid END marker */
		return LINELOG_RESULT_EILLDATA;

	/* ar->lines[ar->linecount].offset records the endoffset */
	returnonerror(reservelines(ar, linenum + 1));
	linelog_lineinfo endlineinfo = { .offset = endoffset };
	ar->lines[linenum] = endlineinfo;
	ar->linecount = linenum;
	return LINELOG_RESULT_OK;
}
