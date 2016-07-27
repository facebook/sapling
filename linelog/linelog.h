#ifndef LINELOG_H_ZUJREV4L
#define LINELOG_H_ZUJREV4L

/*
 * Copyright 2016-present Facebook. All Rights Reserved.
 *
 * linelog.h: data structure tracking line changes
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2 or any later version.
 */

#include <stddef.h>
#include <stdint.h>

/* static assert sizeof(size_t) >= sizeof(uint32_t) */
extern char linelog_assert_sizet_[1 / (sizeof(size_t) >= 4)];

typedef uint32_t linelog_linenum; /* line number, starting from 0 */
typedef uint32_t linelog_revnum; /* rev x is the only parent of rev x + 1 */
typedef uint32_t linelog_offset; /* internal use, index of linelog_buf.data */

typedef int linelog_result; /* return value of some apis */

#define LINELOG_RESULT_OK (0) /* success */
#define LINELOG_RESULT_ENOMEM (-1) /* failed to malloc or realloc */
#define LINELOG_RESULT_EILLDATA (-2) /* illegal data, unexpected values */
#define LINELOG_RESULT_EOVERFLOW (-3) /* hard limit exceeded */
#define LINELOG_RESULT_ENEEDRESIZE (-4) /* buf.size should >= neededsize */

/* main storage (memory buffer) for linelog data, allocated by caller
   same on-disk and in-memory format, endianness-insensitive.
   designed to be used with mmap for efficient updates. */
typedef struct {
	uint8_t *data; /* mmap-friendly, set by caller */
	size_t size; /* bytes, set by caller */
	size_t neededsize; /* set by callee on ENEEDRESIZE */
} linelog_buf;

/* an annotated line */
typedef struct {
	linelog_revnum rev; /* revision number at the first appearance */
	linelog_linenum linenum; /* line number at the first appearance */
	linelog_offset offset; /* internal use, index of linelog_buf.data */
} linelog_lineinfo;

/* annotate result, an dynamic array of linelog_lineinfo, allocated by callee
   memset to 0 before use, call linelog_annotateresult_clear to free memory */
typedef struct {
	linelog_lineinfo *lines;
	linelog_linenum linecount;
	linelog_linenum maxlinecount;
} linelog_annotateresult;

/* free memory used by ar, useful to reset ar from an invalid state */
void linelog_annotateresult_clear(linelog_annotateresult *ar);

/* (re-)initialize the buffer, make it represent an empty file */
linelog_result linelog_clear(linelog_buf *buf);

/* get the actual size needed for buf->data */
size_t linelog_getactualsize(const linelog_buf *buf);

/* get the max revision number covered by this linelog
   return 0 if buf is not initialized (by linelog_clear). */
linelog_revnum linelog_getmaxrev(const linelog_buf *buf);

#endif /* end of include guard: LINELOG_H_ZUJREV4L */
