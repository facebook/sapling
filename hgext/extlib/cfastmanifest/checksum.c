// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum.c: implementation for recalculating the checksums for
//             intermediate nodes in a tree.
//
// no-check-code

#include "hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/sha1.h"
#include "node.h"

static void update_checksum(node_t* node) {
  fbhg_sha1_ctx_t ctx;
  fbhg_sha1_init(&ctx);

  // find all the children and make sure their checksums are up-to-date.
  for (child_num_t ix = 0; ix < node->num_children; ix++) {
    node_t* child = get_child_by_index(node, ix);
    if (child->checksum_valid == false) {
      update_checksum(child);
    }

    fbhg_sha1_update(&ctx, child->name, child->name_sz);
    fbhg_sha1_update(&ctx, child->checksum, child->checksum_sz);
    fbhg_sha1_update(&ctx, &child->flags, 1);
  }

  fbhg_sha1_final(node->checksum, &ctx);
  node->checksum_sz = SHA1_BYTES;
  node->checksum_valid = true;
}

void update_checksums(tree_t* tree) {
  update_checksum(tree->shadow_root);
}
