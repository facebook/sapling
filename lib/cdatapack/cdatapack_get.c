// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// cdatapack_get.c: Use the index to dump a node's delta chain.
// no-check-code

#include <inttypes.h>
#include <memory.h>
#include <stdio.h>
#include <stdlib.h>

#include "cdatapack/cdatapack.h"
#include "clib/convert.h"
#include "clib/sha1.h"

#define DATAIDX_EXT ".dataidx"
#define DATAPACK_EXT ".datapack"

// if the platform uses XMM registers to run strlen, then we might load
// beyond the memory region allocated for the strings.  to ensure we keep
// valgrind happy, we pad the strings with extra space and initialize the areas.
#define XMM_SZ 16

int main(int argc, char* argv[]) {
  if (argc < 3) {
    fprintf(stderr, "%s <path> <node>\n", argv[0]);
    return 1;
  }

  if (strlen(argv[2]) != NODE_SZ * 2) {
    fprintf(stderr, "node should be %d characters long\n", NODE_SZ * 2);
    return 1;
  }

  long len = strlen(argv[1]);
  char* idx_path = (char*)malloc(len + sizeof(DATAIDX_EXT));
  char* data_path = (char*)malloc(len + sizeof(DATAPACK_EXT));
  if (idx_path == NULL || data_path == NULL) {
    free(idx_path);
    free(data_path);
    fprintf(stderr, "Failed to allocate memory for idx_path or data_path\n");
    exit(1);
  }

  sprintf(idx_path, "%s%s", argv[1], DATAIDX_EXT);
  sprintf(data_path, "%s%s", argv[1], DATAPACK_EXT);

  datapack_handle_t* handle =
      open_datapack(idx_path, strlen(idx_path), data_path, strlen(data_path));
  free(data_path);
  free(idx_path);
  if (handle->status != DATAPACK_HANDLE_OK) {
    fprintf(stderr, "failed to open pack: %d\n", handle->status);
    return 1;
  }

  uint8_t binhash[NODE_SZ];

  unhexlify(argv[2], NODE_SZ * 2, binhash);

  delta_chain_t chain = getdeltachain(handle, binhash);

  if (chain.code != GET_DELTA_CHAIN_OK) {
    fprintf(stderr, "error retrieving delta chain (code=%d)\n", chain.code);
    return 1;
  }

  const char* last_filename = NULL;
  uint16_t last_filename_sz = 0;

  uint8_t sha[NODE_SZ];

  char node_buffer[NODE_SZ * 2 + XMM_SZ];
  char deltabase_buffer[NODE_SZ * 2 + XMM_SZ];
  char sha_buffer[NODE_SZ * 2 + +XMM_SZ];

  // to keep valgrind happy, we initialize the memory *beyond* what hexlify
  // will write to.  that way, when a parallelized strnlen comes along, it
  // does not find the memory beyond our string uninitialized.
  memset(&node_buffer[NODE_SZ * 2], 0, XMM_SZ);
  memset(&deltabase_buffer[NODE_SZ * 2], 0, XMM_SZ);
  memset(&sha_buffer[NODE_SZ * 2], 0, XMM_SZ);

  for (int ix = 0; ix < chain.links_count; ix++) {
    delta_chain_link_t* link = &chain.delta_chain_links[ix];

    fbhg_sha1_ctx_t ctx;
    fbhg_sha1_init(&ctx);
    fbhg_sha1_update(&ctx, link->delta, link->delta_sz);
    fbhg_sha1_final(sha, &ctx);

    if (last_filename_sz != link->filename_sz ||
        memcmp(last_filename, link->filename, last_filename_sz) != 0) {
      // print the filename
      printf("\n%-.*s\n", link->filename_sz, link->filename);
      last_filename_sz = link->filename_sz;
      last_filename = link->filename;
    }

    hexlify(link->node, NODE_SZ, node_buffer);
    hexlify(link->deltabase_node, NODE_SZ, deltabase_buffer);
    hexlify(sha, NODE_SZ, sha_buffer);

    printf(
        "%-*s  %-*s  %-*s  %s\n",
        NODE_SZ * 2,
        "Node",
        NODE_SZ * 2,
        "Delta Base",
        NODE_SZ * 2,
        "Delta SHA1",
        "Delta Length");
    printf("%-.*s  ", NODE_SZ * 2, node_buffer);
    printf("%-.*s  ", NODE_SZ * 2, deltabase_buffer);
    printf("%-.*s  ", NODE_SZ * 2, sha_buffer);
    printf("%" PRIu64 "\n", link->delta_sz);
  }
}
