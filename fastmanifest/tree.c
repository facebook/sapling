// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree.c: core methods for tree creation and manipulation.  to keep this file
//         a reasonable length, some of the more complicated methods have
//         been split off into their own .c files (tree_arena.c, tree_convert.c,
//         tree_copy.c, checksum.c).
//
// no-check-code

#include <stdlib.h>

#include "tree.h"
#include "tree_arena.h"

bool valid_path(const char* path, const size_t path_sz) {
  if (path_sz > 0 && (path[0] == '/' || path[path_sz] == '/')) {
    return false;
  }

  size_t last_slash = (size_t) -1;
  for (size_t off = 0; off < path_sz; off ++) {
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
 * first component of the path.  The path must be valid according to
 * `valid_path`.
 *
 * first_component('abc/def') => 'abc'
 * first_component('abc') => ''
 */
static size_t first_component(const char* path, size_t path_sz) {
  for (size_t off = 0; off < path_sz; off ++) {
    if (path[off] == '/') {
      return off;
    }
  }

  return 0;
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

typedef enum {
  TREE_ADD_CHILD_OK,
  TREE_ADD_CHILD_OOM,
  TREE_ADD_CHILD_WTF,
} tree_add_child_code_t;
typedef struct _tree_add_child_result_t {
  tree_add_child_code_t code;
  node_t* newroot;
  node_t* newchild;
} tree_add_child_result_t;
/**
 * Adds a child to `root`.  Because `root` may need to be resized to accomodate
 * the new child, we need the *parent* of `root`.  On success (`result.code` ==
 * TREE_ADD_CHILD_OK), `result.newchild` will be set to the new node created.
 * Because the root may also have been moved, `result.newroot` will be set to
 * the new root.  Be sure to save BOTH.
 *
 * Updates the size and the non-arena-allocations in the tree state change
 * accounting structure.
 */
static tree_add_child_result_t tree_add_child(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name, const size_t name_sz,
    tree_state_changes_t* changes) {
  tree_add_child_result_t result;

  // create a new child node, and record the deltas in the change
  // register.
  //
  // NOTE: OPTIMIZATION OPPORTUNITY!
  //
  // this is a potential optimization opportunity.  we could theoretically try
  // to allocate the new node in the arena and maintain compacted state of the
  // tree.
  node_t* node = alloc_node(name, name_sz, 0);
  if (node == NULL) {
    return (tree_add_child_result_t) {
        TREE_ADD_CHILD_OOM, NULL, NULL };
  }

  // accounting changes.
  changes->size_change += node->block_sz;
  changes->non_arena_allocations = true;

  result.newchild = node;

  // attempt to add a child to `root` with the name `name`.
  node_add_child_result_t add_child_result = add_child(root, node);
  if (add_child_result == NEEDS_LARGER_NODE) {
    // NOTE: OPTIMIZATION OPPORTUNITY!
    //
    // this is a linear scan.  it's unclear whether a linear scan for a pointer
    // is better or worse than a binary search that has to chase a pointer.  the
    // answer is probably to do the linear scan for nodes with a small number of
    // children, and a binary search for nodes with a lot of children.
    uint32_t index = get_child_index(root_parent, root);
    if (index == UINT32_MAX) {
      return (tree_add_child_result_t) {
        TREE_ADD_CHILD_WTF, NULL, NULL };
    }
    node_enlarge_child_capacity_result_t enlarge_result =
        enlarge_child_capacity(root_parent, index);

    if (enlarge_result.code == ENLARGE_OOM) {
      return (tree_add_child_result_t) {
        TREE_ADD_CHILD_OOM, NULL, NULL };
    } else if (enlarge_result.code != ENLARGE_OK) {
      return (tree_add_child_result_t) {
        TREE_ADD_CHILD_WTF, NULL, NULL };
    }

    // update accounting.
    if (!in_arena(tree, enlarge_result.old_child)) {
      // not in arena, free the memory.
      uint32_t block_sz = enlarge_result.old_child->block_sz;
      free(enlarge_result.old_child);
      changes->size_change -= block_sz;
    }
    changes->size_change += enlarge_result.new_child->block_sz;

    root = enlarge_result.new_child;

    // add the child again.
    add_child_result = add_child(root, node);
    if (add_child_result != ADD_CHILD_OK) {
      return (tree_add_child_result_t) {
        TREE_ADD_CHILD_WTF, NULL, NULL };
    }
  } else if (add_child_result != ADD_CHILD_OK) {
    return (tree_add_child_result_t) {
      TREE_ADD_CHILD_WTF, NULL, NULL };
  }

  result.code = TREE_ADD_CHILD_OK;
  result.newroot = root;
  return result;
}

typedef enum {
  // walks the tree.  if the path cannot be found, exit with
  // `FIND_PATH_NOT_FOUND`.
  BASIC_WALK,

  // walks the tree.  if the intermediate paths cannot be found, create them.
  // if a leaf node exists where an intermediate path node needs to be
  // created, then return `FIND_PATH_CONFLICT`.
  CREATE_IF_MISSING,

  // walks the tree.  if the path cannot be found, exit with
  // `FIND_PATH_NOT_FOUND`.  if the operation is successful, then check
  // intermediate nodes to ensure that they still have children.  any nodes
  // that do not should be removed.
  REMOVE_EMPTY_IMPLICIT_NODES,
} find_path_operation_type;
typedef enum _find_path_result_t {
  FIND_PATH_OK,
  FIND_PATH_NOT_FOUND,
  FIND_PATH_OOM,
  FIND_PATH_CONFLICT,
  FIND_PATH_WTF,
} find_path_result_t;
typedef struct _find_path_callback_result_t {
  find_path_result_t code;
  node_t* newroot;
} find_path_callback_result_t;
/**
 * Find the directory node enclosing `path`.  If `create_if_not_found` is true,
 * then any intermediate directories that do not exist will be created.  Once
 * the directory enclosing the object at `path` is located, `callback` will be
 * invoked.  It should do whatever operation is desired and mark up how the tree
 * has been modified.
 *
 * On exit, `find_path` will examine the state changes and use them to update
 * the nodes it has encountered walking to this node.
 *
 * The path must be valid according to `valid_path`, but since it is not checked
 * internally, the caller is responsible for ensuring it.
 */
static find_path_result_t find_path(
    tree_t *tree,
    node_t *const root_parent,
    node_t *root,
    const char *path, const size_t path_sz,
    find_path_operation_type operation_type,
    tree_state_changes_t *changes,
    find_path_callback_result_t (*callback)(
        tree_t *tree,
        node_t *const dir_parent,
        node_t *dir,
        const char *path, const size_t path_sz,
        tree_state_changes_t *changes,
        void *context),
    void *context) {
  size_t first_component_sz = first_component(path, path_sz);
  find_path_result_t result;
  if (first_component_sz == 0) {
    // found it!  apply the magic function.
    find_path_callback_result_t callback_result = callback(tree,
        root_parent, root,
        path, path_sz,
        changes,
        context);

    result = callback_result.code;
    root = callback_result.newroot;
  } else {
    // resolve the first component.
    node_t* child = get_child_by_name(root, path, first_component_sz);
    if (child == NULL) {
      if (operation_type == CREATE_IF_MISSING) {
        // create the new child.
        tree_add_child_result_t tree_add_child_result =
            tree_add_child(
                tree, root_parent, root, path, first_component_sz, changes);
        switch (tree_add_child_result.code) {
          case TREE_ADD_CHILD_OOM:
            return FIND_PATH_OOM;
          case TREE_ADD_CHILD_WTF:
            return FIND_PATH_WTF;
          case TREE_ADD_CHILD_OK:
            break;
        }

        root = tree_add_child_result.newroot;
        child = tree_add_child_result.newchild;

        // it's an implicit node.
        child->type = TYPE_IMPLICIT;
      } else {
        // didn't find it, return.
        return FIND_PATH_NOT_FOUND;
      }
    } else if (child->type == TYPE_LEAF) {
      // throw an error.
      return FIND_PATH_CONFLICT;
    }

    result = find_path(
        tree,
        root,
        child,
        path + first_component_sz + 1,
        path_sz - first_component_sz - 1,
        operation_type,
        changes,
        callback,
        context);
  }

  if (result == FIND_PATH_OK) {
    // is the checksum still valid?  mark up the nodes as we pop off the stack.
    if (changes->checksum_dirty == true) {
      root->checksum_valid = false;
    }
  }

  return result;
}

tree_t *alloc_tree() {
  // do all the memory allocations.
  node_t *shadow_root = alloc_node("/", 1, 1);
  node_t *real_root = alloc_node("/", 1, 0);
  tree_t *tree = (tree_t *) calloc(1, sizeof(tree_t));

  if (shadow_root == NULL ||
      real_root == NULL ||
      tree == NULL) {
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
  tree->consumed_memory += shadow_root->block_sz;
  tree->consumed_memory += real_root->block_sz;
  tree->arena = NULL;
  tree->arena_free_start = NULL;
  tree->arena_sz = 0;
  tree->compacted = false;

  return tree;

fail:
  if (shadow_root != NULL) {
    free(shadow_root);
  }
  if (real_root != NULL) {
    free(real_root);
  }
  if (tree != NULL) {
    free(tree);
  }

  return NULL;
}

static void destroy_tree_helper(tree_t* tree, node_t* node) {
  for (int ix = 0; ix < node->num_children; ix ++) {
    destroy_tree_helper(tree, get_child_by_index(node, ix));
  }

  if (!in_arena(tree, node)) {
    free(node);
  }
}

void destroy_tree(tree_t* tree) {
  if (tree->compacted == false) {
    destroy_tree_helper(tree, tree->shadow_root);
  }
  if (tree->arena != NULL) {
    free(tree->arena);
  }

  free(tree);
}

typedef struct _get_path_metadata_t {
  node_t* node;
} get_path_metadata_t;
find_path_callback_result_t get_path_callback(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name, const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  get_path_metadata_t *metadata =
      (get_path_metadata_t *) context;

  // does the path already exist?
  node_t *child = get_child_by_name(root, name, name_sz);
  if (child == NULL) {
    return (find_path_callback_result_t) {
        FIND_PATH_NOT_FOUND, root};
  }

  metadata->node = child;

  return (find_path_callback_result_t) { FIND_PATH_OK, root };
}

get_path_result_t get_path(
    tree_t* tree,
    const char* path,
    const size_t path_sz) {
  tree_state_changes_t changes = { 0 };
  get_path_metadata_t metadata;

  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  if (real_root == NULL) {
    return (get_path_result_t) { GET_PATH_WTF, NULL };
  }

  find_path_result_t result =
      find_path(
          tree,
          shadow_root,
          real_root,
          path, path_sz,
          BASIC_WALK,
          &changes,
          get_path_callback,
          &metadata);

  assert(changes.size_change == 0);
  assert(changes.num_leaf_node_change == 0);
  assert(changes.non_arena_allocations == false);

  switch (result) {
    case FIND_PATH_OK:
      return (get_path_result_t) { GET_PATH_OK, metadata.node };
    case FIND_PATH_NOT_FOUND:
    case FIND_PATH_CONFLICT:
      // `FIND_PATH_CONFLICT` is returned if there is a leaf node where we
      // expect a directory node.  this is treated the same as a NOT_FOUND.
      return (get_path_result_t) { GET_PATH_NOT_FOUND, NULL };
    default:
      return (get_path_result_t) { GET_PATH_WTF, NULL };
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
    const char* name, const size_t name_sz,
    tree_state_changes_t* changes,
    void* context) {
  add_or_update_path_metadata_t* metadata =
      (add_or_update_path_metadata_t*) context;

  // does the path already exist?
  node_t* child = get_child_by_name(root, name, name_sz);
  if (child == NULL) {
    tree_add_child_result_t tree_add_child_result =
        tree_add_child(
            tree,
            root_parent,
            root,
            name, name_sz,
            changes);
    switch (tree_add_child_result.code) {
      case TREE_ADD_CHILD_OOM:
        return (find_path_callback_result_t) {
          FIND_PATH_OOM, NULL };
      case TREE_ADD_CHILD_WTF:
        return (find_path_callback_result_t) {
          FIND_PATH_WTF, NULL };
      case TREE_ADD_CHILD_OK:
        break;
    }
    root = tree_add_child_result.newroot;
    child = tree_add_child_result.newchild;

    // it's a leaf node.
    child->type = TYPE_LEAF;

    // update the accounting.
    changes->num_leaf_node_change ++;
  } else {
    if (child->type == TYPE_IMPLICIT) {
      // was previously a directory
      return (find_path_callback_result_t) {
        FIND_PATH_CONFLICT, NULL };
    }
  }

  // update the node.
  if (metadata->checksum_sz > CHECKSUM_BYTES) {
    return (find_path_callback_result_t) {
      FIND_PATH_WTF, NULL };
  }

  memcpy(child->checksum, metadata->checksum, metadata->checksum_sz);
  child->checksum_sz = metadata->checksum_sz;
  child->checksum_valid = true;
  child->flags = metadata->flags;

  return (find_path_callback_result_t) { FIND_PATH_OK, root };
}

add_update_path_result_t add_or_update_path(
    tree_t* tree,
    const char* path,
    const size_t path_sz,
    const uint8_t* checksum,
    const uint8_t checksum_sz,
    const uint8_t flags) {
  tree_state_changes_t changes = { 0 };
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

  find_path_result_t result =
      find_path(
          tree,
          shadow_root,
          real_root,
          path, path_sz,
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
