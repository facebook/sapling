// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_copy.c: methods to make a copy of a tree.  the new instance is compacted
//              into an arena.
//
// no-check-code

#include "internal_result.h"
#include "node.h"
#include "tree.h"
#include "tree_arena.h"

typedef enum {
  COPY_OK,
  COPY_OOM,
  COPY_WTF,
} copy_helper_result_t;

/**
 * Clones `src` and adds it as the Nth child of `dst_parent`, where N ==
 * `child_num`.
 *
 * `child_num` must be <= `dst_parent->num_children`.
 */
copy_helper_result_t copy_helper(
    tree_t* dst_tree,
    const node_t* src,
    node_t* dst_parent,
    size_t child_num) {
  arena_alloc_node_result_t alloc_result = arena_alloc_node_strict(
      dst_tree, src->name, src->name_sz, src->num_children);
  switch (alloc_result.code) {
    case ARENA_ALLOC_OK:
      break;
    case ARENA_ALLOC_OOM:
      return COPY_OOM;
    case ARENA_ALLOC_EXCEEDED_LIMITS:
      return COPY_WTF;
  }

  // copy the attributes
  node_t* dst = alloc_result.node;
  if (src->checksum_valid) {
    memcpy(dst->checksum, src->checksum, src->checksum_sz);
    dst->checksum_sz = src->checksum_sz;
  }
  dst->checksum_valid = src->checksum_valid;
  dst->flags = src->flags;
  dst->type = src->type;

  // typically we don't like touching this field manually, but to
  // `set_child_by_index` requires the index be < num_children.
  dst->num_children = src->num_children;

  if (dst->type == TYPE_LEAF) {
    dst_tree->num_leaf_nodes ++;
  } else {
    for (int ix = 0; ix < src->num_children; ix ++) {
      copy_helper_result_t copy_result =
          copy_helper(
              dst_tree,
              get_child_by_index(src, ix),
              dst,
              ix);

      if (copy_result != COPY_OK) {
        return copy_result;
      }
    }
  }

  set_child_by_index(dst_parent, child_num, dst);

  return COPY_OK;
}

tree_t* copy(const tree_t* src) {
  tree_t* dst = alloc_tree_with_arena(src->consumed_memory);

  // prerequisite for using copy_helper is that child_num must be <
  // dst_parent->num_children, so we artificially bump up the num_chlidren
  // for the shadow root.
  assert(max_children(dst->shadow_root) > 0);
  dst->shadow_root->num_children = 1;

  copy_helper_result_t copy_result = copy_helper(
      dst,
      get_child_by_index(src->shadow_root, 0),
      dst->shadow_root,
      0);

  switch (copy_result) {
    case COPY_OK:
      dst->compacted = true;
      return dst;
    default:
      destroy_tree(dst);
      return NULL;
  }
}
