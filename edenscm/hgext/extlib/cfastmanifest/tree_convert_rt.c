// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_convert_rt.c: simple benchmark for converting a flat manifest to a tree
//                    and back.  the output can be diff'ed against the input as
//                    for more sophisticated testing than the unit tests
//                    provide.
//
// no-check-code

#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/time.h>

#include "checksum.h"
#include "edenscm/hgext/extlib/cfastmanifest/tree.h"

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

  struct timeval before_checksum, after_checksum;
  gettimeofday(&before_checksum, NULL);
  update_checksums(from_flat.tree);
  gettimeofday(&after_checksum, NULL);

  struct timeval before_to, after_to;
  gettimeofday(&before_to, NULL);
  convert_to_flat_result_t to_flat = convert_to_flat(from_flat.tree);
  gettimeofday(&after_to, NULL);

  if (to_flat.code != CONVERT_TO_FLAT_OK) {
    fprintf(stderr, "Error: converting to flat manifest\n");
    exit(1);
  }

  if (fwrite(to_flat.flat_manifest, to_flat.flat_manifest_sz, 1, ofh) != 1) {
    fprintf(stderr, "Error: writing flat manifest\n");
    exit(1);
  }

  fclose(ofh);

  uint64_t usecs_before_from =
      before_from.tv_sec * 1000000 + before_from.tv_usec;
  uint64_t usecs_after_from = after_from.tv_sec * 1000000 + after_from.tv_usec;
  uint64_t usecs_before_checksum =
      before_checksum.tv_sec * 1000000 + before_checksum.tv_usec;
  uint64_t usecs_after_checksum =
      after_checksum.tv_sec * 1000000 + after_checksum.tv_usec;
  uint64_t usecs_before_to = before_to.tv_sec * 1000000 + before_to.tv_usec;
  uint64_t usecs_after_to = after_to.tv_sec * 1000000 + after_to.tv_usec;

  printf(
      "flat -> tree: %" PRIu64 " us\n", (usecs_after_from - usecs_before_from));
  printf(
      "checksum: %" PRIu64 " us\n",
      (usecs_after_checksum - usecs_before_checksum));
  printf("tree -> flat: %" PRIu64 " us\n", (usecs_after_to - usecs_before_to));
  printf(
      "tree consumed memory: %" PRIuPTR "\n", from_flat.tree->consumed_memory);
}
