// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// cdatapack_dump.c: Dump the entire contents of a datapack file by walking
//                   the datapack file.
// no-check-code

#include <inttypes.h>
#include <memory.h>
#include <stdio.h>
#include <stdlib.h>

#include "cdatapack/cdatapack.h"
#include "clib/convert.h"

#define DATAIDX_EXT ".dataidx"
#define DATAPACK_EXT ".datapack"

int main(int argc, char* argv[]) {
  if (argc < 2) {
    fprintf(stderr, "%s <path>\n", argv[0]);
    exit(1);
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

  const uint8_t* ptr = handle->data_mmap;
  const uint8_t* end = ptr + handle->data_file_sz;

  ptr += 1; // for the version field.

  const char* last_filename = NULL;
  uint16_t last_filename_sz = 0;

  char node_buffer[NODE_SZ * 2];
  char deltabase_buffer[NODE_SZ * 2];

  while (ptr < end) {
    delta_chain_link_t link;
    get_delta_chain_link_result_t next;

    next = getdeltachainlink(handle, ptr, &link);
    ptr = next.ptr;

    if (last_filename_sz != link.filename_sz ||
        memcmp(last_filename, link.filename, last_filename_sz) != 0) {
      // print the filename
      printf("\n%-.*s\n", (int)link.filename_sz, link.filename);
      last_filename_sz = link.filename_sz;
      last_filename = link.filename;
    }

    hexlify(link.node, NODE_SZ, node_buffer);
    hexlify(link.deltabase_node, NODE_SZ, deltabase_buffer);

    printf(
        "%-*s  %-*s  %s\n",
        NODE_SZ * 2,
        "Node",
        NODE_SZ * 2,
        "Delta Base",
        "Delta Length");
    printf(
        "%-.*s  %-.*s  %" PRIu64 "\n",
        NODE_SZ * 2,
        node_buffer,
        NODE_SZ * 2,
        deltabase_buffer,
        link.delta_sz);
  }

  close_datapack(handle);
}
