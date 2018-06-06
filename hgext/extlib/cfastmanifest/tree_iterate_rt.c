// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_iterate_rt.c: simple benchmark for converting a flat manifest to a tree
//                    and then back into a flat manifest through iteration.
//
// no-check-code

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/time.h>

#include "hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/convert.h"
#include "node.h"

int main(int argc, char* argv[]) {
  if (argc < 3) {
    fprintf(stderr, "Usage: %s <manifestfile> <outputfile>\n", argv[0]);
    exit(1);
  }

  FILE* fh = fopen(argv[1], "r");
  if (fh == NULL) {
    fprintf(stderr, "Error: cannot open %s\n", argv[1]);
    exit(1);
  }

  FILE* ofh = fopen(argv[2], "w");
  if (ofh == NULL) {
    fprintf(stderr, "Error: cannot open %s\n", argv[2]);
    exit(1);
  }

  fseeko(fh, 0, SEEK_END);
  off_t length = ftello(fh);
  rewind(fh);

  char* flatmanifest = malloc(length);
  if (flatmanifest == NULL) {
    fprintf(stderr, "Error: cannot allocate memory for reading %s\n", argv[1]);
    exit(1);
  }

  if (fread(flatmanifest, length, 1, fh) != 1) {
    fprintf(stderr, "Error: cannot read %s\n", argv[1]);
    exit(1);
  }

  struct timeval before_from, after_from;
  gettimeofday(&before_from, NULL);
  convert_from_flat_result_t from_flat =
      convert_from_flat(flatmanifest, length);
  gettimeofday(&after_from, NULL);

  if (from_flat.code != CONVERT_FROM_FLAT_OK) {
    fprintf(stderr, "Error: converting from flat manifest\n");
    exit(1);
  }

  struct timeval before_to, after_to;
  gettimeofday(&before_to, NULL);
  iterator_t* iterator = create_iterator(from_flat.tree, true);

  char sha_ascii[SHA1_BYTES * 2];

  while (true) {
    iterator_result_t iterator_result = iterator_next(iterator);

    if (iterator_result.valid == false) {
      break;
    }

    hexlify(iterator_result.checksum, SHA1_BYTES, sha_ascii);

    fwrite(iterator_result.path, iterator_result.path_sz, 1, ofh);
    fputc(0, ofh);
    fwrite(sha_ascii, SHA1_BYTES * 2, 1, ofh);
    if (iterator_result.flags != 0) {
      fputc(iterator_result.flags, ofh);
    }
    fputc('\n', ofh);
  }
  gettimeofday(&after_to, NULL);

  fclose(ofh);

  uint64_t usecs_before_from =
      before_from.tv_sec * 1000000 + before_from.tv_usec;
  uint64_t usecs_after_from = after_from.tv_sec * 1000000 + after_from.tv_usec;
  uint64_t usecs_before_to = before_to.tv_sec * 1000000 + before_to.tv_usec;
  uint64_t usecs_after_to = after_to.tv_sec * 1000000 + after_to.tv_usec;

  printf(
      "flat -> tree: %" PRIu64 " us\n", (usecs_after_from - usecs_before_from));
  printf(
      "tree -> iterater -> flat: %" PRIu64 " us\n",
      (usecs_after_to - usecs_before_to));
  printf(
      "tree consumed memory: %" PRIuPTR "\n", from_flat.tree->consumed_memory);
}
