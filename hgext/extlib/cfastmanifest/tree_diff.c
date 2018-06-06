// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_diff.c: methods to diff two trees.
//
// no-check-code

#include <stdbool.h>
#include <stdlib.h>

#include "checksum.h"
#include "hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/buffer.h"
#include "node.h"

#define DEFAULT_BUILD_BUFFER_SZ 16384
#define BUFFER_GROWTH_FACTOR 2.0
#define BUFFER_MINIMUM_GROWTH 16384
#define BUFFER_MAXIMUM_GROWTH 65536

#define DIFF_EXPAND_TO_FIT(buffer, buffer_idx, buffer_sz, input_sz) \
  expand_to_fit(                                                    \
      (void**)buffer,                                               \
      buffer_idx,                                                   \
      buffer_sz,                                                    \
      input_sz,                                                     \
      BUFFER_GROWTH_FACTOR,                                         \
      sizeof(char),                                                 \
      BUFFER_MINIMUM_GROWTH,                                        \
      BUFFER_MAXIMUM_GROWTH)

typedef struct _diff_context_t {
  bool include_all;
  void (*callback)(
      const char* path,
      const size_t path_sz,
      const bool left_present,
      const uint8_t* left_checksum,
      const uint8_t left_checksum_sz,
      const uint8_t left_flags,
      const bool right_present,
      const uint8_t* right_checksum,
      const uint8_t right_checksum_sz,
      const uint8_t right_flags,
      void* context);
  void* context;

  // used to build up the path
  char* path_build_buffer;
  size_t path_build_buffer_idx;
  size_t path_build_buffer_sz;
} diff_context_t;

static diff_result_t diff_tree_helper(
    const node_t* left,
    const node_t* right,
    diff_context_t* diff_context);

typedef enum {
  CONSIDER_PROCESSED_LEFT,
  CONSIDER_PROCESSED_RIGHT,
  CONSIDER_PROCESSED_BOTH,
  CONSIDER_PROCESSED_OOM,
  CONSIDER_PROCESSED_WTF,
} consider_children_result_t;

/**
 * Consider two nodes for diff.  If one precedes the other lexicographically,
 * then the later one is not processed.  If they are identical
 * lexicographically, but of are different types, then the leaf node is
 * processed and the implicit node is not processed.
 *
 * If they are lexicographically identical and both are leaf nodes, then the
 * callback is called.  If they are lexicographically identical and both are
 * implicit nodes, then we call diff_tree_helper(..).
 *
 * Returns a code indicating which node(s) are processed.
 */
consider_children_result_t consider_children(
    const node_t* left_candidate,
    const node_t* right_candidate,
    diff_context_t* diff_context) {
  // if there's two, then zero out the one that comes later in
  // lexicographical order.  if they are the same and they're of identical
  // types, then both will continue on.
  if (left_candidate != NULL && right_candidate != NULL) {
    int order = name_compare(
        left_candidate->name, left_candidate->name_sz, right_candidate);
    if (order < 0) {
      // left goes first, clear right
      right_candidate = NULL;
    } else if (order > 0) {
      // right goes first, clear left
      left_candidate = NULL;
    } else if (
        left_candidate->type == TYPE_LEAF &&
        right_candidate->type != TYPE_LEAF) {
      // identical types, left is a leaf node and right is not, so clear right.
      right_candidate = NULL;
    } else if (
        left_candidate->type != TYPE_LEAF &&
        right_candidate->type == TYPE_LEAF) {
      // identical types, right is a leaf node and left is not, so clear left.
      left_candidate = NULL;
    }
  }

  // save the path index
  size_t previous_path_index = diff_context->path_build_buffer_idx;
  char* name;
  size_t name_sz;

  if (left_candidate != NULL) {
    name = (char*)left_candidate->name;
    name_sz = left_candidate->name_sz;
  } else {
    name = (char*)right_candidate->name;
    name_sz = right_candidate->name_sz;
  }

  if (DIFF_EXPAND_TO_FIT(
          &diff_context->path_build_buffer,
          diff_context->path_build_buffer_idx,
          &diff_context->path_build_buffer_sz,
          name_sz) == false) {
    return CONSIDER_PROCESSED_OOM;
  }

  memcpy(
      &diff_context->path_build_buffer[diff_context->path_build_buffer_idx],
      name,
      name_sz);
  diff_context->path_build_buffer_idx += name_sz;

  if ((left_candidate != NULL && left_candidate->type == TYPE_IMPLICIT) ||
      (right_candidate != NULL && right_candidate->type == TYPE_IMPLICIT)) {
    // if one is a directory node, either the other one is NULL or
    // also a directory node.  in that case, descend into the subdirectory.

    diff_result_t result =
        diff_tree_helper(left_candidate, right_candidate, diff_context);
    switch (result) {
      case DIFF_OOM:
        return CONSIDER_PROCESSED_OOM;
      case DIFF_WTF:
        return CONSIDER_PROCESSED_WTF;
      default:;
    }
  } else if (
      diff_context->include_all != false || left_candidate == NULL ||
      right_candidate == NULL ||
      left_candidate->flags != right_candidate->flags ||
      left_candidate->checksum_sz != right_candidate->checksum_sz ||
      memcmp(
          left_candidate->checksum,
          right_candidate->checksum,
          left_candidate->checksum_sz) != 0) {
    const uint8_t* left_checksum =
        left_candidate != NULL ? left_candidate->checksum : NULL;
    const uint8_t left_checksum_sz =
        left_candidate != NULL ? left_candidate->checksum_sz : 0;
    const uint8_t left_flags =
        left_candidate != NULL ? left_candidate->flags : 0;
    const uint8_t* right_checksum =
        right_candidate != NULL ? right_candidate->checksum : NULL;
    const uint8_t right_checksum_sz =
        right_candidate != NULL ? right_candidate->checksum_sz : 0;
    const uint8_t right_flags =
        right_candidate != NULL ? right_candidate->flags : 0;

    // either the two nodes are not identical, or we're being requested to
    // include all the nodes.
    diff_context->callback(
        diff_context->path_build_buffer,
        diff_context->path_build_buffer_idx,
        left_candidate != NULL,
        left_checksum,
        left_checksum_sz,
        left_flags,
        right_candidate != NULL,
        right_checksum,
        right_checksum_sz,
        right_flags,
        diff_context->context);
  }

  // restore the old path write point.
  diff_context->path_build_buffer_idx = previous_path_index;

  if (left_candidate != NULL && right_candidate != NULL) {
    return CONSIDER_PROCESSED_BOTH;
  } else if (left_candidate != NULL) {
    return CONSIDER_PROCESSED_LEFT;
  } else {
    return CONSIDER_PROCESSED_RIGHT;
  }
}

/**
 * Diff two nodes.  One of the nodes may be NULL, and we must accommodate that
 * possibility.
 */
static diff_result_t diff_tree_helper(
    const node_t* left,
    const node_t* right,
    diff_context_t* diff_context) {
  assert(
      left == NULL || left->type == TYPE_ROOT || left->type == TYPE_IMPLICIT);
  assert(
      right == NULL || right->type == TYPE_ROOT ||
      right->type == TYPE_IMPLICIT);

  // if the two nodes have identical checksums and include_all is false, then
  // we can return immediately.
  if (diff_context->include_all == false && left != NULL &&
      left->checksum_valid && right != NULL && right->checksum_valid &&
      left->checksum_sz == right->checksum_sz &&
      memcmp(left->checksum, right->checksum, left->checksum_sz) == 0) {
    return DIFF_OK;
  }

  // now we need to merge the two nodes' children in lexicographical order.
  for (size_t left_idx = 0, right_idx = 0;
       (left != NULL && left_idx < left->num_children) ||
       (right != NULL && right_idx < right->num_children);) {
    // grab the candidates.
    node_t* left_candidate = NULL;
    node_t* right_candidate = NULL;

    if (left != NULL && left_idx < left->num_children) {
      if (!VERIFY_CHILD_NUM(left_idx)) {
        return DIFF_WTF;
      }
      left_candidate = get_child_by_index(left, (child_num_t)left_idx);
      assert(left_candidate->checksum_valid == true);
    }
    if (right != NULL && right_idx < right->num_children) {
      if (!VERIFY_CHILD_NUM(right_idx)) {
        return DIFF_WTF;
      }
      right_candidate = get_child_by_index(right, (child_num_t)right_idx);
      assert(right_candidate->checksum_valid == true);
    }

    consider_children_result_t consider_children_result =
        consider_children(left_candidate, right_candidate, diff_context);

    switch (consider_children_result) {
      case CONSIDER_PROCESSED_OOM:
        return DIFF_OOM;
      case CONSIDER_PROCESSED_WTF:
        return DIFF_WTF;
      default:
        break;
    }

    if (consider_children_result == CONSIDER_PROCESSED_BOTH ||
        consider_children_result == CONSIDER_PROCESSED_LEFT) {
      left_idx++;
    }
    if (consider_children_result == CONSIDER_PROCESSED_BOTH ||
        consider_children_result == CONSIDER_PROCESSED_RIGHT) {
      right_idx++;
    }
  }

  return DIFF_OK;
}

diff_result_t diff_trees(
    tree_t* const left,
    tree_t* const right,
    bool include_all,
    void (*callback)(
        const char* path,
        const size_t path_sz,
        const bool left_present,
        const uint8_t* left_checksum,
        const uint8_t left_checksum_sz,
        const uint8_t left_flags,
        const bool right_present,
        const uint8_t* right_checksum,
        const uint8_t right_checksum_sz,
        const uint8_t right_flags,
        void* context),
    void* context) {
  update_checksums(left);
  update_checksums(right);

  node_t *left_shadow_root, *right_shadow_root;
  node_t *left_real_root, *right_real_root;

  left_shadow_root = left->shadow_root;
  right_shadow_root = right->shadow_root;

  if (left_shadow_root->num_children != 1 ||
      right_shadow_root->num_children != 1) {
    return DIFF_WTF;
  }

  left_real_root = get_child_by_index(left_shadow_root, 0);
  right_real_root = get_child_by_index(right_shadow_root, 0);

  diff_context_t diff_context = {include_all, callback, context};

  diff_context.path_build_buffer = malloc(DEFAULT_BUILD_BUFFER_SZ);
  diff_context.path_build_buffer_idx = 0;
  diff_context.path_build_buffer_sz = DEFAULT_BUILD_BUFFER_SZ;

  if (diff_context.path_build_buffer == NULL) {
    return DIFF_OOM;
  }

  assert(left_real_root->checksum_valid == true);
  assert(right_real_root->checksum_valid == true);
  diff_result_t result =
      diff_tree_helper(left_real_root, right_real_root, &diff_context);

  free(diff_context.path_build_buffer);

  return result;
}
