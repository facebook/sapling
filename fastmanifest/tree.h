// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree.h: publicly accessible functions for tree manipulation and
//         conversions.  this should be the only header file directly exposed
//         to users.
//
// no-check-code

#ifndef __FASTMANIFEST_TREE_H__
#define __FASTMANIFEST_TREE_H__

#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

#include "result.h"

#if 0 // FIXME: (ttung) probably remove this
typedef enum {
  ARENA_MODE,                           /* all allocations should come from the
                                         * arena.  this is to produce a
                                         * compact and relocatable tree. */
  STANDARD_MODE,                        /* all allocations should come from the
                                         * standard system allocator, i.e.,
                                         * malloc. */

} allocation_mode_t;
#endif /* #if 0 */

typedef struct _tree_t {
  // these fields are preserved during serialization.
  size_t consumed_memory;
  uint32_t num_leaf_nodes;

  // these fields are not preserved during serialization.
  struct _node_t *shadow_root;
  /* this is a literal pointer. */
  void *arena;
  /* this is also a literal pointer. */
  void *arena_free_start;
  /* this is also a literal pointer. */
  size_t arena_sz;
  bool compacted;

#if 0 // FIXME: (ttung) probably remove this
  allocation_mode_t mode;
#endif /* #if 0 */
} tree_t;

/**
 * Returns true iff the path is something digestible by this tree library.  The
 * rules are:
 *
 * 1) The path must be of nonzero length.
 * 2) The path must not start nor end with the path separator '/'.
 * 3) The path must not have consecutive path separators.
 */
extern bool valid_path(const char *path, const size_t path_sz);

extern tree_t *alloc_tree();

extern void destroy_tree(tree_t *tree);

extern tree_t *copy(const tree_t *src);

extern get_path_result_t get_path(
    tree_t *const tree,
    const char *path,
    const size_t path_sz);

extern add_update_path_result_t add_or_update_path(
    tree_t *const tree,
    const char *path,
    const size_t path_sz,
    const uint8_t *checksum,
    const uint8_t checksum_sz,
    const uint8_t flags);

extern remove_path_result_t remove_path(
    tree_t *const tree,
    const char *path,
    const size_t path_sz);

extern bool contains_path(
    const tree_t *tree,
    const char *path,
    const size_t path_sz);

extern tree_t *read_from_file(char *fname);

extern write_to_file_result_t write_to_file(tree_t *tree, char *fname);

extern convert_from_flat_result_t convert_from_flat(
    char *manifest, size_t manifest_sz);

extern convert_to_flat_result_t convert_to_flat(tree_t *tree);

#endif /* #ifndef __FASTMANIFEST_TREE_H__ */
