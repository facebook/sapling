// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum.c: implementation for recalculating the checksums for
//             intermediate nodes in a tree.
//
// no-check-code

#include "sha1/sha1.h"

#include "node.h"
#include "tree.h"

static void update_checksum(node_t *node) {
  SHA1_CTX ctx;
  SHA1DCInit(&ctx);

  // find all the children and make sure their checksums are up-to-date.
  for (child_num_t ix = 0; ix < node->num_children; ix++) {
    node_t* child = get_child_by_index(node, ix);
    if (child->checksum_valid == false) {
      update_checksum(child);
    }

    SHA1DCUpdate(&ctx, (const unsigned char*) child->name, child->name_sz);
    SHA1DCUpdate(&ctx, child->checksum, child->checksum_sz);
    SHA1DCUpdate(&ctx, &child->flags, 1);
  }

  SHA1DCFinal(node->checksum, &ctx);
  node->checksum_sz = SHA1_BYTES;
  node->checksum_valid = true;
}

void update_checksums(tree_t *tree) {
  update_checksum(tree->shadow_root);
}
