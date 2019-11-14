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
typedef uint32_t linelog_offset; /* index of linelog_buf.data */

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
  uint8_t* data; /* mmap-friendly, set by caller */
  size_t size; /* bytes, set by caller */
  size_t neededsize; /* set by callee on ENEEDRESIZE */
} linelog_buf;

/* an annotated line */
typedef struct {
  linelog_revnum rev; /* revision number at the first appearance */
  linelog_linenum linenum; /* line number at the first appearance */
  linelog_offset offset; /* index of linelog_buf.data */
} linelog_lineinfo;

/* annotate result, an dynamic array of linelog_lineinfo, allocated by callee
   memset to 0 before use, call linelog_annotateresult_clear to free memory */
typedef struct {
  linelog_lineinfo* lines;
  linelog_linenum linecount;
  linelog_linenum maxlinecount;
} linelog_annotateresult;

/* free memory used by ar, useful to reset ar from an invalid state */
void linelog_annotateresult_clear(linelog_annotateresult* ar);

/* (re-)initialize the buffer, make it represent an empty file */
linelog_result linelog_clear(linelog_buf* buf);

/* get the actual size needed for buf->data */
size_t linelog_getactualsize(const linelog_buf* buf);

/* get the max revision number covered by this linelog
   return 0 if buf is not initialized (by linelog_clear). */
linelog_revnum linelog_getmaxrev(const linelog_buf* buf);

/* note: some notations are from Python:
   - range(p, q) means from p (inclusive) to q (exclusive), p <= q
   - array[p:q] means a slice of the array with indexes in range(p, q) */

/* calculate annotateresult for rev from buf, output result to ar

   on success, let i be the line number at rev, in range(0, ar->linecount),
     ar->lines[i].rev is the number of the revision introducing the line
     ar->lines[i].linenum is the corresponding line number at ar->lines[i].rev

   on error, ar may be in an invalid state and needs to be cleared */
linelog_result linelog_annotate(
    const linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum rev);

/* update buf and ar, replace existing lines[a1:a2] with lines[b1:b2] in brev

   ar should be obtained using linelog_annotate(brev).
   brev introduces the change. the change is not present in earlier revisions.

   usually brev is greater than maxrev to do incremental updates, like:
     rev = linelog_getmaxrev(buf)
     linelog_annotate(buf, rev, ar)
     for-each-new-rev {
       rev += 1
       // no need to run linelog_annotate(buf, rev, ar) again, because
       // linelog_replacelines will keep it updated
       for-each-chunk {
         linelog_replacelines(buf, ar, rev, ...)
       }
     }

   however, it's also possible to edit previous revisions, but be sure to use
   the corresponding ar, obtained by calling linelog_annotate(brev).

   on error, ar may be in an invalid state and needs to be cleared */
linelog_result linelog_replacelines(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum brev,
    linelog_linenum a1,
    linelog_linenum a2,
    linelog_linenum b1,
    linelog_linenum b2);

/* like linelog_replacelines, but control details about lines being inserted

   line numbers and revision numbers are decided by blinenums and brevs.
   this table shows the difference from linelog_replacelines:

   # | linelog_replacelines | linelog_replacelines_vec
     | revnum, linenum      | revnum, linenum
   --+----------------------+----------------------------------------------
   0 | rev, b1              | brevs[0], blinenums[0]
   1 | rev, b1+1            | brevs[1], blinenums[1]
   . |                      |
   . | rev, b2-1            | brevs[blinecount-1], blinenums[blinecount-1]

   note: although lines can have revision numbers other than brev, they are
   still marked as introduced by brev. i.e. visible to brev and later
   revisions, invisible to earlier revisions.

   this is useful for merge commits. consider the following case where rev 3
   merges rev 1 and 2:

            2        : feature branch
           / \
     0 -- 1 - 3 --   : main branch

   a typical "annotate" operation running at rev 3 would show rev 1 and 2 but
   hide rev 3 if the merge is clean.

   linelog can only store linear history. typically it only tracks the main
   branch thus rev 2 won't get stored. when introducing rev 3 (brev = 3),
   individual lines can have different revisions (brevs[i] != 3) so
   linelog_annotate(rev=3) works as if rev 2 is stored. be aware that
   linelog_annotate(rev=2) will be the same as linelog_annotate(rev=1). */
linelog_result linelog_replacelines_vec(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_revnum brev,
    linelog_linenum a1,
    linelog_linenum a2,
    linelog_linenum blinecount,
    const linelog_revnum* brevs,
    const linelog_linenum* blinenums);

/* get all lines, include deleted ones, output to ar

   offsets can be obtained from annotateresult. if they are both 0,
   all lines from the entire linelog will be returned.

   internally, this is a traversal from offset1 (inclusive) to offset2
   (exclusive) and conditional jumps are ignored. */
linelog_result linelog_getalllines(
    linelog_buf* buf,
    linelog_annotateresult* ar,
    linelog_offset offset1,
    linelog_offset offset2);

#endif /* end of include guard: LINELOG_H_ZUJREV4L */
