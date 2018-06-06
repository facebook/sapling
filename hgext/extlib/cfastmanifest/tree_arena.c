// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_arena.c: methods to create a tree with a fixed memory arena and to
//               allocate nodes from the fixed memory arena.
//
// no-check-code

#include <stdlib.h>

#include "hgext/extlib/cfastmanifest/tree.h"
#include "node.h"
#include "tree_arena.h"

#define ARENA_INCREMENT_PERCENTAGE 20
#define ARENA_MIN_STORAGE_INCREMENT (1024 * 1024)
#define ARENA_MAX_STORAGE_INCREMENT (16 * 1024 * 1024)

static inline size_t calculate_arena_free(const tree_t* tree) {
  intptr_t arena_start = (intptr_t)tree->arena;
  intptr_t arena_free_start = (intptr_t)tree->arena_free_start;
  intptr_t arena_end = arena_start + tree->arena_sz;
  size_t arena_free = arena_end - arena_free_start;

  return arena_free;
}

arena_alloc_node_result_t arena_alloc_node_helper(
    arena_policy_t policy,
    tree_t* tree,
    const char* name,
    size_t name_sz,
    size_t max_children) {
  // since name_sz and max_chlidren are going to be downcasted, we should verify
  // that they're not too large for the types in node.h
  if (!VERIFY_NAME_SZ(name_sz) || !VERIFY_CHILD_NUM(max_children)) {
    return COMPOUND_LITERAL(arena_alloc_node_result_t){
        ARENA_ALLOC_EXCEEDED_LIMITS, NULL};
  }

  do {
    size_t arena_free = calculate_arena_free(tree);

    node_t* candidate = (node_t*)tree->arena_free_start;
    void* next = setup_node(
        tree->arena_free_start,
        arena_free,
        name,
        (name_sz_t)name_sz,
        (child_num_t)max_children);

    if (next == NULL) {
      if (policy == ARENA_POLICY_FAIL) {
        return COMPOUND_LITERAL(arena_alloc_node_result_t){ARENA_ALLOC_OOM,
                                                           NULL};
      } else {
        size_t new_arena_sz =
            (tree->arena_sz * (100 + ARENA_INCREMENT_PERCENTAGE)) / 100;
        // TODO: optimization opportunity!
        // we can calculate how much free space we need and set that as another
        // minimum.  in the unlikely scenario we need a huge node, just setting
        // the lower bound on ARENA_MIN_STORAGE_INCREMENT may require multiple
        // rounds of realloc.
        if (new_arena_sz - tree->arena_sz < ARENA_MIN_STORAGE_INCREMENT) {
          new_arena_sz = tree->arena_sz + ARENA_MIN_STORAGE_INCREMENT;
        }
        if (new_arena_sz - tree->arena_sz > ARENA_MAX_STORAGE_INCREMENT) {
          new_arena_sz = tree->arena_sz + ARENA_MAX_STORAGE_INCREMENT;
        }

        // resize the arena so it's bigger.
        void* new_arena = realloc(tree->arena, new_arena_sz);

        if (new_arena == NULL) {
          return COMPOUND_LITERAL(arena_alloc_node_result_t){ARENA_ALLOC_OOM,
                                                             NULL};
        }

        // success!  update the pointers.
        if (new_arena != tree->arena) {
          intptr_t arena_start = (intptr_t)tree->arena;
          intptr_t arena_free_start = (intptr_t)tree->arena_free_start;
          intptr_t new_arena_start = (intptr_t)new_arena;

          // if the shadow root is inside the arena, we need to relocate it.
          if (in_arena(tree, tree->shadow_root)) {
            intptr_t shadow_root = (intptr_t)tree->shadow_root;
            ptrdiff_t shadow_root_offset = shadow_root - arena_start;

            tree->shadow_root = (node_t*)(new_arena_start + shadow_root_offset);
          }

          intptr_t new_arena_free_start = new_arena_start;
          new_arena_free_start += (arena_free_start - arena_start);
          tree->arena_free_start = (void*)new_arena_free_start;
          tree->arena = new_arena;
        }
        tree->arena_sz = new_arena_sz;
      }
    } else {
      tree->arena_free_start = next;
      tree->consumed_memory += candidate->block_sz;
      return COMPOUND_LITERAL(arena_alloc_node_result_t){ARENA_ALLOC_OK,
                                                         candidate};
    }
  } while (true);
}

tree_t* alloc_tree_with_arena(size_t arena_sz) {
  void* arena = malloc(arena_sz);
  tree_t* tree = (tree_t*)calloc(1, sizeof(tree_t));
  node_t* shadow_root = alloc_node("/", 1, 1);

  if (arena == NULL || tree == NULL || shadow_root == NULL) {
    free(arena);
    free(tree);
    free(shadow_root);
    return NULL;
  }

#if 0 // FIXME: (ttung) probably remove this
  tree->mode = STANDARD_MODE;
#endif /* #if 0 */
  tree->arena = tree->arena_free_start = arena;
  tree->arena_sz = arena_sz;
  tree->compacted = true;
  tree->shadow_root = NULL;

  tree->consumed_memory = 0;
  tree->num_leaf_nodes = 0;

  shadow_root->type = TYPE_ROOT;
  tree->shadow_root = shadow_root;

  return tree;
}
