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
#include <stddef.h>
#include <stdint.h>

#include "lib/clib/portability/portability.h"
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
  struct _node_t* shadow_root;
  /* this is a literal pointer. */
  void* arena;
  /* this is also a literal pointer. */
  void* arena_free_start;
  /* this is also a literal pointer. */
  size_t arena_sz;
  bool compacted;

#if 0 // FIXME: (ttung) probably remove this
  allocation_mode_t mode;
#endif /* #if 0 */
} tree_t;

typedef struct _iterator_t iterator_t;

/**
 * Returns true iff the path is something digestible by this tree library.  The
 * rules are:
 *
 * 1) The path must be of nonzero length.
 * 2) The path must not start nor end with the path separator '/'.
 * 3) The path must not have consecutive path separators.
 */
extern bool valid_path(const char* path, const size_t path_sz);

extern tree_t* alloc_tree(void);

extern void destroy_tree(tree_t* tree);

extern tree_t* copy_tree(const tree_t* src);

extern tree_t* filter_copy(
    const tree_t* src,
    bool (*filter)(char* path, size_t path_sz, void* callback_context),
    void* callback_context);

extern get_path_result_t
get_path(tree_t* const tree, const char* path, const size_t path_sz);

extern add_update_path_result_t add_or_update_path(
    tree_t* const tree,
    const char* path,
    const size_t path_sz,
    const uint8_t* checksum,
    const uint8_t checksum_sz,
    const uint8_t flags);

extern remove_path_result_t
remove_path(tree_t* const tree, const char* path, const size_t path_sz);

extern bool contains_path(
    // we ought to be able to do this as a const, but we can't propagate
    // const-ness through a method call.  so unless we dupe the code to create
    // a const-version of find_path, we cannot enforce this programmatically.
    /* const */ tree_t* tree,
    const char* path,
    const size_t path_sz);

extern read_from_file_result_t read_from_file(char* fname, size_t fname_sz);

extern write_to_file_result_t
write_to_file(tree_t* tree, char* fname, size_t fname_sz);

extern convert_from_flat_result_t convert_from_flat(
    char* manifest,
    size_t manifest_sz);

extern convert_to_flat_result_t convert_to_flat(tree_t* tree);

/**
 * Calculate the difference between two trees, and call a given function with
 * information about the nodes.
 *
 * If `include_all` is true, then the callback is called with all the nodes,
 * regardless of whether a difference exists or not.
 *
 * If `include_all` is false, then the callback is only called on the nodes
 * where there is a difference.
 *
 * To maintain compatibility with flat manifests, the nodes are traversed in
 * lexicographical order.  If the caller wishes to maintain a reference to
 * the path beyond the scope of the immediate callback, it must save a copy
 * of the path.  It is *not* guaranteed to be valid once the callback
 * function terminates.
 */
extern diff_result_t diff_trees(
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
    void* context);

extern iterator_t* create_iterator(const tree_t* tree, bool construct_paths);

extern iterator_result_t iterator_next(iterator_t* iterator);

extern void destroy_iterator(iterator_t* iterator);

#endif /* #ifndef __FASTMANIFEST_TREE_H__ */
