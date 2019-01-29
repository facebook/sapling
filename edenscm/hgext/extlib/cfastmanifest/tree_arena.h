// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_arena.h: declarations for methods to create a tree with a fixed
//               memory arena and to allocate nodes from the fixed memory
//               arena.  for internal use only.
//
// no-check-code

#ifndef __FASTMANIFEST_TREE_ARENA_H__
#define __FASTMANIFEST_TREE_ARENA_H__

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "node.h"

typedef enum {
  ARENA_POLICY_FAIL, // fail immediately when there is
                     // insufficient space
  ARENA_POLICY_REALLOC, // attempt to realloc until realloc
                        // fails.
} arena_policy_t;

typedef enum {
  ARENA_ALLOC_OK,
  ARENA_ALLOC_OOM,
  ARENA_ALLOC_EXCEEDED_LIMITS,
} arena_alloc_node_code_t;

typedef struct _arena_alloc_node_result_t {
  arena_alloc_node_code_t code;
  node_t* node;
} arena_alloc_node_result_t;

static inline bool in_arena(const tree_t* tree, void* _ptr) {
  intptr_t arena_start = (intptr_t)tree->arena;
  intptr_t arena_end = arena_start + tree->arena_sz - 1;
  intptr_t ptr = (intptr_t)_ptr;

  if (ptr >= arena_start && ptr < arena_end) {
    return true;
  }
  return false;
}

/**
 * Allocate space for a node within a heap-allocated arena.  If the arena does
 * not have enough space for the node, consult the policy to determine what to
 * do next.
 */
extern arena_alloc_node_result_t arena_alloc_node_helper(
    arena_policy_t policy,
    tree_t* tree,
    const char* name,
    size_t name_sz,
    size_t max_children);

static inline arena_alloc_node_result_t arena_alloc_node(
    tree_t* tree,
    const char* name,
    size_t name_sz,
    size_t max_children) {
  return arena_alloc_node_helper(
      ARENA_POLICY_REALLOC, tree, name, name_sz, max_children);
}

static inline arena_alloc_node_result_t arena_alloc_node_strict(
    tree_t* tree,
    const char* name,
    size_t name_sz,
    size_t max_children) {
  return arena_alloc_node_helper(
      ARENA_POLICY_FAIL, tree, name, name_sz, max_children);
}

/**
 * Creates a tree and sets up the shadow root node.  This does *not* initialize
 * the real root node.  It is the responsibility of the caller to do so.
 */
extern tree_t* alloc_tree_with_arena(size_t arena_sz);

#endif /* #ifndef __FASTMANIFEST_TREE_ARENA_H__ */
