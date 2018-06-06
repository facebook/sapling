// Copyright 2016-present Facebook. All Rights Reserved.
//
// tests.c: convenience functions for unit tests.
//
// no-check-code

#include "tests.h"
#include "hgext/extlib/cfastmanifest/tree.h"
#include "node.h"
#include "tree_path.h"

typedef struct _get_path_unfiltered_metadata_t {
  node_t* node;
} get_path_unfiltered_metadata_t;

static find_path_callback_result_t get_path_unfiltered_callback(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  get_path_unfiltered_metadata_t* metadata =
      (get_path_unfiltered_metadata_t*)context;

  // does the path already exist?
  node_t* child = get_child_by_name(root, name, name_sz);
  if (child == NULL) {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_NOT_FOUND,
                                                         root};
  }

  metadata->node = child;

  return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_OK, root};
}

get_path_unfiltered_result_t
get_path_unfiltered(tree_t* tree, const char* path, const size_t path_sz) {
  tree_state_changes_t changes = {0};
  get_path_unfiltered_metadata_t metadata;

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return COMPOUND_LITERAL(get_path_unfiltered_result_t){GET_PATH_WTF, NULL};
  }

  find_path_result_t result = find_path(
      tree,
      shadow_root,
      real_root,
      path,
      path_sz,
      BASIC_WALK_ALLOW_IMPLICIT_NODES,
      &changes,
      get_path_unfiltered_callback,
      &metadata);

  assert(changes.size_change == 0);
  assert(changes.num_leaf_node_change == 0);
  assert(changes.non_arena_allocations == false);

  switch (result) {
    case FIND_PATH_OK:
      return COMPOUND_LITERAL(get_path_unfiltered_result_t){GET_PATH_OK,
                                                            metadata.node};
    case FIND_PATH_NOT_FOUND:
    case FIND_PATH_CONFLICT:
      // `FIND_PATH_CONFLICT` is returned if there is a leaf node where we
      // expect a directory node.  this is treated the same as a NOT_FOUND.
      return COMPOUND_LITERAL(get_path_unfiltered_result_t){GET_PATH_NOT_FOUND,
                                                            NULL};
    default:
      return COMPOUND_LITERAL(get_path_unfiltered_result_t){GET_PATH_WTF, NULL};
  }
}
