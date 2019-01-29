// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree.c: core methods for tree creation and manipulation.  to keep this file
//         a reasonable length, some of the more complicated methods have
//         been split off into their own .c files (tree_arena.c, tree_convert.c,
//         tree_copy.c, checksum.c).
//
// no-check-code

#include <stdlib.h>

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "tree_arena.h"
#include "tree_path.h"

bool valid_path(const char* path, const size_t path_sz) {
  if (path_sz > 0 && (path[0] == '/' || path[path_sz] == '/')) {
    return false;
  }

  size_t last_slash = (size_t)-1;
  for (size_t off = 0; off < path_sz; off++) {
    if (path[off] == '/') {
      if (last_slash == off - 1) {
        return false;
      }

      last_slash = off;
    }
  }

  return true;
}

/**
 * Given a path, return the size of the string that would yield just the
 * directory name.  The path must be valid according to `valid_path`, but
 * otherwise the semantics are like os.path.dirname on python.
 *
 * dirname('abc/def/ghi') => 'abc/def'
 * dirname('abc/def') => 'abc'
 * dirname('abc') => ''
 */
/*static size_t dirname(const char* path, size_t path_sz) {
  for (size_t off = path_sz; off > 0; off --) {
    if (path[off - 1] == '/') {
      if (off == 1) {
        return 1;
      } else {
        return off - 1;
      }
    }
  }

  return 0;
}
*/

tree_t* alloc_tree() {
  // do all the memory allocations.
  node_t* shadow_root = alloc_node("/", 1, 1);
  node_t* real_root = alloc_node("/", 1, 0);
  tree_t* tree = (tree_t*)calloc(1, sizeof(tree_t));

  if (shadow_root == NULL || real_root == NULL || tree == NULL) {
    goto fail;
  }

  shadow_root->type = TYPE_ROOT;
  real_root->type = TYPE_ROOT;

  node_add_child_result_t add_result = add_child(shadow_root, real_root);
  if (add_result != ADD_CHILD_OK) {
    goto fail;
  }

  tree->shadow_root = shadow_root;
  tree->consumed_memory = 0;
  tree->consumed_memory += real_root->block_sz;
  tree->arena = NULL;
  tree->arena_free_start = NULL;
  tree->arena_sz = 0;
  tree->compacted = false;

  return tree;

fail:
  free(shadow_root);
  free(real_root);
  free(tree);

  return NULL;
}

static void destroy_tree_helper(tree_t* tree, node_t* node) {
  for (int ix = 0; ix < node->num_children; ix++) {
    destroy_tree_helper(tree, get_child_by_index(node, ix));
  }

  if (!in_arena(tree, node)) {
    free(node);
  }
}

void destroy_tree(tree_t* tree) {
  if (tree == NULL) {
    return;
  }
  if (tree->compacted == false) {
    destroy_tree_helper(tree, tree->shadow_root);
  } else {
    free(tree->shadow_root);
  }
  free(tree->arena);

  free(tree);
}

typedef struct _get_path_metadata_t {
  const node_t* node;
} get_path_metadata_t;

find_path_callback_result_t get_path_callback(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  get_path_metadata_t* metadata = (get_path_metadata_t*)context;

  // does the path already exist?
  node_t* child = get_child_by_name(root, name, name_sz);
  if (child == NULL || child->type != TYPE_LEAF) {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_NOT_FOUND,
                                                         root};
  }

  metadata->node = child;

  return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_OK, root};
}

get_path_result_t
get_path(tree_t* tree, const char* path, const size_t path_sz) {
  tree_state_changes_t changes = {0};
  get_path_metadata_t metadata;

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return COMPOUND_LITERAL(get_path_result_t){GET_PATH_WTF, NULL};
  }

  find_path_result_t result = find_path(
      tree,
      shadow_root,
      real_root,
      path,
      path_sz,
      BASIC_WALK,
      &changes,
      get_path_callback,
      &metadata);

  assert(changes.size_change == 0);
  assert(changes.num_leaf_node_change == 0);
  assert(changes.non_arena_allocations == false);

  switch (result) {
    case FIND_PATH_OK:
      return COMPOUND_LITERAL(get_path_result_t){GET_PATH_OK,
                                                 metadata.node->checksum,
                                                 metadata.node->checksum_sz,
                                                 metadata.node->flags};
    case FIND_PATH_NOT_FOUND:
    case FIND_PATH_CONFLICT:
      // `FIND_PATH_CONFLICT` is returned if there is a leaf node where we
      // expect a directory node.  this is treated the same as a NOT_FOUND.
      return COMPOUND_LITERAL(get_path_result_t){GET_PATH_NOT_FOUND, NULL};
    default:
      return COMPOUND_LITERAL(get_path_result_t){GET_PATH_WTF, NULL};
  }
}

typedef struct _add_or_update_path_metadata_t {
  const uint8_t* checksum;
  const uint8_t checksum_sz;
  const uint8_t flags;
} add_or_update_path_metadata_t;

find_path_callback_result_t add_or_update_path_callback(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  add_or_update_path_metadata_t* metadata =
      (add_or_update_path_metadata_t*)context;

  // does the path already exist?
  node_t* child = get_child_by_name(root, name, name_sz);
  if (child == NULL) {
    tree_add_child_result_t tree_add_child_result = tree_add_child(
        tree,
        root_parent,
        root,
        name,
        name_sz,
        0, // leaf nodes don't have children.
        changes);
    switch (tree_add_child_result.code) {
      case TREE_ADD_CHILD_OOM:
        return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_OOM,
                                                             NULL};
      case TREE_ADD_CHILD_WTF:
        return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_WTF,
                                                             NULL};
      case TREE_ADD_CHILD_OK:
        break;
    }
    root = tree_add_child_result.newroot;
    child = tree_add_child_result.newchild;

    // it's a leaf node.
    child->type = TYPE_LEAF;

    // update the accounting.
    changes->num_leaf_node_change++;
  } else {
    if (child->type == TYPE_IMPLICIT) {
      // was previously a directory
      return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_CONFLICT,
                                                           NULL};
    }
  }

  // update the node.
  if (metadata->checksum_sz > CHECKSUM_BYTES) {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_WTF, NULL};
  }

  memcpy(child->checksum, metadata->checksum, metadata->checksum_sz);
  child->checksum_sz = metadata->checksum_sz;
  child->checksum_valid = true;
  child->flags = metadata->flags;

  changes->checksum_dirty = true;

  return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_OK, root};
}

add_update_path_result_t add_or_update_path(
    tree_t* tree,
    const char* path,
    const size_t path_sz,
    const uint8_t* checksum,
    const uint8_t checksum_sz,
    const uint8_t flags) {
  tree_state_changes_t changes = {0};
  add_or_update_path_metadata_t metadata = {
      checksum,
      checksum_sz,
      flags,
  };

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return ADD_UPDATE_PATH_WTF;
  }

  find_path_result_t result = find_path(
      tree,
      shadow_root,
      real_root,
      path,
      path_sz,
      CREATE_IF_MISSING,
      &changes,
      add_or_update_path_callback,
      &metadata);

  // apply the changes back to the tree struct
  tree->consumed_memory += changes.size_change;
  tree->num_leaf_nodes += changes.num_leaf_node_change;
  if (changes.non_arena_allocations) {
    tree->compacted = false;
  }

  switch (result) {
    case FIND_PATH_OK:
      return ADD_UPDATE_PATH_OK;
    case FIND_PATH_OOM:
      return ADD_UPDATE_PATH_OOM;
    case FIND_PATH_CONFLICT:
      return ADD_UPDATE_PATH_CONFLICT;
    default:
      return ADD_UPDATE_PATH_WTF;
  }
}

find_path_callback_result_t remove_path_callback(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  // does the path already exist?
  node_search_children_result_t search_result =
      search_children(root, name, name_sz);

  if (search_result.child == NULL) {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_NOT_FOUND,
                                                         NULL};
  }

  // record the metadata changes.
  changes->checksum_dirty = true;
  changes->num_leaf_node_change--;
  changes->size_change -= search_result.child->block_sz;

  node_remove_child_result_t remove_result =
      remove_child(root, search_result.child_num);

  if (remove_result == REMOVE_CHILD_OK) {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_OK, root};
  } else {
    return COMPOUND_LITERAL(find_path_callback_result_t){FIND_PATH_WTF, root};
  }
}

remove_path_result_t
remove_path(tree_t* tree, const char* path, const size_t path_sz) {
  tree_state_changes_t changes = {0};

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return REMOVE_PATH_WTF;
  }

  find_path_result_t result = find_path(
      tree,
      shadow_root,
      real_root,
      path,
      path_sz,
      REMOVE_EMPTY_IMPLICIT_NODES,
      &changes,
      remove_path_callback,
      NULL);

  // apply the changes back to the tree struct
  tree->consumed_memory += changes.size_change;
  tree->num_leaf_nodes += changes.num_leaf_node_change;
  if (changes.non_arena_allocations) {
    tree->compacted = false;
  }

  switch (result) {
    case FIND_PATH_OK:
      return REMOVE_PATH_OK;
    case FIND_PATH_NOT_FOUND:
      return REMOVE_PATH_NOT_FOUND;
    default:
      return REMOVE_PATH_WTF;
  }
}

bool contains_path(tree_t* tree, const char* path, const size_t path_sz) {
  tree_state_changes_t changes = {0};
  get_path_metadata_t metadata;

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return false;
  }

  find_path_result_t result = find_path(
      (tree_t*)tree,
      shadow_root,
      real_root,
      path,
      path_sz,
      BASIC_WALK,
      &changes,
      get_path_callback,
      &metadata);

  assert(changes.size_change == 0);
  assert(changes.num_leaf_node_change == 0);
  assert(changes.non_arena_allocations == false);

  switch (result) {
    case FIND_PATH_OK:
      return true;

    default:
      return false;
  }
}
