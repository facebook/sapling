// Copyright 2016-present Facebook. All Rights Reserved.
//
// cdatapack_get.c: Use the index to dump a node's delta chain.
//
// no-check-code

#include <openssl/sha.h>

#include <inttypes.h>
#include <memory.h>
#include <stdio.h>
#include "convert.h"
#include "cdatapack.h"

#define DATAIDX_EXT  ".dataidx"
#define DATAPACK_EXT ".datapack"

int main(int argc, char *argv[]) {
  if (argc < 3) {
    fprintf(stderr, "%s <path> <node>\n", argv[0]);
    return 1;
  }

  if (strlen(argv[2]) != NODE_SZ * 2) {
    fprintf(stderr, "node should be %d characters long\n", NODE_SZ * 2);
    return 1;
  }

  long len = strlen(argv[1]);
  char idx_path[len + sizeof(DATAIDX_EXT)];
  char data_path[len + sizeof(DATAPACK_EXT)];

  sprintf(idx_path, "%s%s", argv[1], DATAIDX_EXT);
  sprintf(data_path, "%s%s", argv[1], DATAPACK_EXT);

  datapack_handle_t *handle = open_datapack(
      idx_path, strlen(idx_path),
      data_path, strlen(data_path));

  uint8_t binhash[NODE_SZ];

  unhexlify(argv[2], NODE_SZ * 2, binhash);

  delta_chain_t *chain = getdeltachain(handle, binhash);

  const char *last_filename = NULL;
  uint16_t last_filename_sz = 0;

  uint8_t sha[NODE_SZ];

  char node_buffer[NODE_SZ * 2];
  char deltabase_buffer[NODE_SZ * 2];
  char sha_buffer[NODE_SZ * 2];

  for (int ix = 0; ix < chain->links_count; ix ++) {
    delta_chain_link_t *link = &chain->delta_chain_links[ix];

    SHA_CTX ctx;
    SHA1_Init(&ctx);
    SHA1_Update(&ctx, link->delta, link->delta_sz);
    SHA1_Final(sha, &ctx);

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

    printf("%-*s  %-*s  %-*s  %s\n",
        NODE_SZ * 2, "Node",
        NODE_SZ * 2, "Delta Base",
        NODE_SZ * 2, "Delta SHA1",
        "Delta Length");
    printf("%-.*s  %-.*s  %-.*s  %" PRIu64 "\n",
        NODE_SZ * 2, node_buffer,
        NODE_SZ * 2, deltabase_buffer,
        NODE_SZ * 2, sha_buffer,
        link->delta_sz);

  }
}
