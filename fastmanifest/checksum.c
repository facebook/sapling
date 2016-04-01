// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum.c: implementation for recalculating the checksums for
//             intermediate nodes in a tree.

#include <openssl/sha.h>

#include "node.h"
#include "tree.h"

static void update_checksum(node_t* node) {
  SHA_CTX ctx;
  SHA1_Init(&ctx);

  // find all the children and make sure their checksums are up-to-date.
  for (int ix = 0; ix < node->num_children; node ++) {
    node_t* child = get_child_by_index(node, ix);
    if (child->checksum_valid == false) {
      update_checksum(child);
    }

    SHA1_Update(&ctx, child->name, child->name_sz);
    SHA1_Update(&ctx, child->checksum, child->checksum_sz);
    SHA1_Update(&ctx, &child->flags, 1);
  }

  SHA1_Final(node->checksum, &ctx);
  node->checksum_sz = SHA1_BYTES;
  node->checksum_valid = true;
}

void update_checksums(tree_t* tree) {
  update_checksum(tree->shadow_root);
}
