// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_path.h: declarations for the core path function for parsing and
//              traversing a path through a tree.
//
// no-check-code

#ifndef __FASTMANIFEST_TREE_PATH_H__
#define __FASTMANIFEST_TREE_PATH_H__

#include "lib/clib/portability/portability.h"
#include "node.h"

typedef struct _tree_state_changes_t {
  ptrdiff_t size_change;
  int32_t num_leaf_node_change;
  bool non_arena_allocations;
  bool checksum_dirty;
} tree_state_changes_t;

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

typedef enum {
  // walks the tree and searches for a leaf node.  if the path cannot be found,
  // exit with `FIND_PATH_NOT_FOUND`.
  BASIC_WALK,

  // walks the tree and searches for any node (including implicit nodes).  if
  // the path cannot be found, exit with `FIND_PATH_NOT_FOUND`.
  BASIC_WALK_ALLOW_IMPLICIT_NODES,

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
typedef enum {
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

extern tree_add_child_result_t tree_add_child(
    tree_t* tree,
    node_t* const root_parent,
    node_t* root,
    const char* name,
    const size_t name_sz,
    size_t num_children_hint,
    tree_state_changes_t* changes);

extern find_path_result_t find_path(
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
    void* context);

#endif // #ifndef __FASTMANIFEST_TREE_PATH_H__
