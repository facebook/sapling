// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_path.h: implementation for the core path function for parsing and
//              traversing a path through a tree.
//
// no-check-code

#include <stdlib.h>

#include "hgext/extlib/cfastmanifest/tree.h"
#include "tree_arena.h"
#include "tree_path.h"

/**
 * Given a path, return the size of the string that would yield just the
 * first component of the path, including the path separator.  The path must be
 * valid according to `valid_path`.
 *
 * first_component('abc/def') => 'abc/'
 * first_component('abc') => ''
 */
static size_t first_component(const char* path, size_t path_sz) {
  for (size_t off = 0; off < path_sz; off++) {
    if (path[off] == '/') {
      return off + 1;
    }
  }

  return 0;
}

/**
 * Adds a child to `root`.  Because `root` may need to be resized to accommodate
 * the new child, we need the *parent* of `root`.  On success (`result.code` ==
 * TREE_ADD_CHILD_OK), `result.newchild` will be set to the new node created.
 * Because the root may also have been moved, `result.newroot` will be set to
 * the new root.  Be sure to save BOTH.
 *
 * Updates the size and the non-arena-allocations in the tree state change
 * accounting structure.
 */
tree_add_child_result_t tree_add_child(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    size_t num_children_hint,
    tree_state_changes_t* changes) {
  tree_add_child_result_t result;

  if (!VERIFY_CHILD_NUM(num_children_hint) || !VERIFY_NAME_SZ(name_sz)) {
    return COMPOUND_LITERAL(tree_add_child_result_t){
        TREE_ADD_CHILD_WTF, NULL, NULL};
  }

  // create a new child node, and record the deltas in the change
  // register.
  //
  // NOTE: OPTIMIZATION OPPORTUNITY!
  //
  // this is a potential optimization opportunity.  we could theoretically try
  // to allocate the new node in the arena and maintain compacted state of the
  // tree.
  node_t* node =
      alloc_node(name, (name_sz_t)name_sz, (child_num_t)num_children_hint);
  if (node == NULL) {
    return COMPOUND_LITERAL(tree_add_child_result_t){
        TREE_ADD_CHILD_OOM, NULL, NULL};
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
      return COMPOUND_LITERAL(tree_add_child_result_t){
          TREE_ADD_CHILD_WTF, NULL, NULL};
    }
    node_enlarge_child_capacity_result_t enlarge_result =
        enlarge_child_capacity(root_parent, index);

    if (enlarge_result.code == ENLARGE_OOM) {
      return COMPOUND_LITERAL(tree_add_child_result_t){
          TREE_ADD_CHILD_OOM, NULL, NULL};
    } else if (enlarge_result.code != ENLARGE_OK) {
      return COMPOUND_LITERAL(tree_add_child_result_t){
          TREE_ADD_CHILD_WTF, NULL, NULL};
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
      return COMPOUND_LITERAL(tree_add_child_result_t){
          TREE_ADD_CHILD_WTF, NULL, NULL};
    }
  } else if (add_child_result != ADD_CHILD_OK) {
    return COMPOUND_LITERAL(tree_add_child_result_t){
        TREE_ADD_CHILD_WTF, NULL, NULL};
  }

  result.code = TREE_ADD_CHILD_OK;
  result.newroot = root;
  return result;
}

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
find_path_result_t find_path(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* path,
    const size_t path_sz,
    find_path_operation_type operation_type,
    tree_state_changes_t* changes,
    find_path_callback_result_t (*callback)(
        tree_t* tree,
        node_t* const dir_parent,
        node_t* dir,
        const char* path,
        const size_t path_sz,
        tree_state_changes_t* changes,
        void* context),
    void* context) {
  size_t first_component_sz = first_component(path, path_sz);
  find_path_result_t result;
  if (first_component_sz == 0 ||
      (operation_type == BASIC_WALK_ALLOW_IMPLICIT_NODES &&
       first_component_sz == path_sz)) {
    // found it!  apply the magic function.
    find_path_callback_result_t callback_result =
        callback(tree, root_parent, root, path, path_sz, changes, context);

    result = callback_result.code;
    root = callback_result.newroot;
  } else {
    // resolve the first component.
    node_t* child = get_child_by_name(root, path, first_component_sz);
    if (child == NULL) {
      if (operation_type == CREATE_IF_MISSING) {
        // create the new child.
        tree_add_child_result_t tree_add_child_result = tree_add_child(
            tree,
            root_parent,
            root,
            path,
            first_component_sz,
            // since we're creating the intermediate nodes that lead to a
            // leaf node, we'll have at least one child.
            1,
            changes);
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
        // we must initialize flags to a known value, even if it's not used
        // because it participates in checksum calculation.
        child->flags = 0;
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
        path + first_component_sz,
        path_sz - first_component_sz,
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

    if (operation_type == REMOVE_EMPTY_IMPLICIT_NODES &&
        root->type == TYPE_IMPLICIT && root->num_children == 0) {
      // update metadata before we free the node.
      changes->size_change -= root->block_sz;

      node_remove_child_result_t remove_result =
          remove_child(root_parent, get_child_index(root_parent, root));

      if (remove_result != REMOVE_CHILD_OK) {
        result = FIND_PATH_WTF;
      } else {
        if (!in_arena(tree, root)) {
          free(root);
        }
      }
    }
  }

  return result;
}
