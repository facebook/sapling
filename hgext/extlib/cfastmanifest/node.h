// Copyright 2016-present Facebook. All Rights Reserved.
//
// node.h: declarations for representing a node in a tree.  for internal use
//         only.
//
// no-check-code

#ifndef __FASTMANIFEST_NODE_H__
#define __FASTMANIFEST_NODE_H__

#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include "internal_result.h"
#include "lib/clib/portability/portability.h"

#define STORAGE_INCREMENT_PERCENTAGE 20
#define MIN_STORAGE_INCREMENT 10
#define MAX_STORAGE_INCREMENT 100

#define CHECKSUM_BYTES 21
#define SHA1_BYTES 20

#define PTR_ALIGN_MASK (~((ptrdiff_t)(sizeof(ptrdiff_t) - 1)))

#define TYPE_UNDEFINED 0
#define TYPE_IMPLICIT 1
#define TYPE_LEAF 2
#define TYPE_ROOT 3

// the start of each of these nodes must be 32-bit aligned.
typedef struct _node_t {
  uint32_t block_sz;
  uint32_t num_children;
  uint16_t name_sz;
  uint8_t checksum[CHECKSUM_BYTES];
  uint8_t checksum_sz;
  uint8_t flags;
  bool in_use : 1;
  unsigned int type : 2;
  bool checksum_valid : 1;
  unsigned int unused : 4;
  char name[0];
  // padding to the nearest ptrdiff_t boundary.
  // then a series of ptrdiff_t-sized pointers to the children.
} node_t;

/**
 * Define some macros for users to test if their values are within the
 * restrictions of our node implementation.
 */
#define VERIFY_BLOCK_SZ(block_sz) ((uintmax_t)(block_sz) < UINT32_MAX)
#define VERIFY_NAME_SZ(name_sz) ((uintmax_t)(name_sz) < UINT16_MAX)
#define VERIFY_CHILD_NUM(child_num) ((uintmax_t)(child_num) < UINT32_MAX)

#define block_sz_t uint32_t
#define name_sz_t uint16_t
#define child_num_t uint32_t

/**
 * Returns <0 if (`name`, `name_sz`) is lexicographically less than the name in
 * node.
 *
 * Returns =0 if (`name`, `name_sz`) is lexicographically equal to the name in
 * node.
 *
 * Returns >0 if (`name`, `name_sz`) is lexicographically greater than the name
 * in node.
 */
static inline int
name_compare(const char* name, uint16_t name_sz, const node_t* node) {
  uint32_t min_sz = (name_sz < node->name_sz) ? name_sz : node->name_sz;
  int sz_compare = name_sz - node->name_sz;

  int cmp = strncmp(name, node->name, min_sz);
  if (cmp) {
    return cmp;
  } else {
    return sz_compare;
  }
}

/**
 * Returns the offset of the first child pointer, given a node with name size
 * `name_sz`.
 */
static inline ptrdiff_t get_child_ptr_base_offset(uint16_t name_sz) {
  intptr_t ptr = offsetof(node_t, name);
  ptr += name_sz;

  // this aligns to ptrdiff_t, since some platforms do not support unaligned
  // loads.
  ptr = (ptr + sizeof(intptr_t) - 1) & PTR_ALIGN_MASK;

  return (ptrdiff_t)ptr;
}

/**
 * Returns the address of the first child pointer.  Since a child pointer is an
 * ptrdiff_t, the type returned is an ptrdiff_t.  Note that this is *not* the
 * value of the first child pointer.
 */
static inline ptrdiff_t* get_child_ptr_base(node_t* node) {
  assert(node->in_use);

  intptr_t address = (intptr_t)node;
  ptrdiff_t offset = get_child_ptr_base_offset(node->name_sz);
  return (ptrdiff_t*)(address + offset);
}

/**
 * Const version of get_child_ptr_base
 */
static inline const ptrdiff_t* get_child_ptr_base_const(const node_t* node) {
  return get_child_ptr_base((node_t*)node);
}

static inline uint32_t max_children(const node_t* node) {
  ptrdiff_t bytes_avail = node->block_sz;
  bytes_avail -= ((intptr_t)get_child_ptr_base_const(node)) - ((intptr_t)node);

  // if it requires > 32b, then we're kind of hosed.
  if (!VERIFY_CHILD_NUM(bytes_avail)) {
    abort();
  }
  return ((uint32_t)(bytes_avail / sizeof(intptr_t)));
}

static inline node_t* get_child_by_index(
    const node_t* node,
    uint32_t child_num) {
  assert(node->in_use);
  assert(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT);
  assert(child_num < node->num_children);

  intptr_t address = (intptr_t)get_child_ptr_base_const(node);
  address += sizeof(ptrdiff_t) * child_num;

  intptr_t base = (intptr_t)node;
  ptrdiff_t offset = *((ptrdiff_t*)address);
  base += offset;
  return (node_t*)base;
}

static inline node_t* get_child_from_diff(const node_t* node, ptrdiff_t diff) {
  assert(node->in_use);
  assert(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT);

  intptr_t base = (intptr_t)node;
  base += diff;
  return (node_t*)base;
}

static inline void
set_child_by_index(node_t* node, size_t child_num, const node_t* child) {
  assert(node->in_use);
  assert(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT);
  assert(child_num < node->num_children);
  assert(child->in_use);

  ptrdiff_t* base = get_child_ptr_base(node);
  ptrdiff_t delta = ((intptr_t)child) - ((intptr_t)node);
  base[child_num] = delta;
}

/**
 * Allocate a node on the heap suitably sized for a given name and a given
 * number of children.  Initialize the node as unused, but copy the name to the
 * node.
 */
extern node_t*
alloc_node(const char* name, uint16_t name_sz, uint32_t max_children);

/**
 * Given a block of memory, attempt to place a node at the start of the block.
 * The node will suitably sized for a given name and a given number of children.
 * Initialize the node as unused, but copy the name to the node.
 *
 * Returns the address following the end of the node if the block is large
 * enough to accommodate the node, or NULL if the block is too small.
 */
extern void* setup_node(
    void* ptr,
    size_t ptr_size_limit,
    const char* name,
    uint16_t name_sz,
    uint32_t max_children);

/**
 * Clone a node and increase the storage capacity by
 * STORAGE_INCREMENT_PERCENTAGE, but by at least MIN_STORAGE_INCREMENT and no
 * more than MAX_STORAGE_INCREMENT.
 */
extern node_t* clone_node(const node_t* node);

/**
 * Adds a child to the node.  A child with the same name must not already exist.
 *
 * The caller is responsible for going up the chain and updating metadata, such
 * as the total number of leaf nodes in tree_t and marking the checksum bit
 * dirty recursively up the tree.
 */
extern node_add_child_result_t add_child(node_t* node, const node_t* child);

/**
 * Remove a child of a node, given a child index.
 *
 * The caller is responsible for going up the chain and updating metadata, such
 * as the total number of leaf nodes in tree_t and marking the checksum bit
 * dirty recursively up the tree.
 */
extern node_remove_child_result_t remove_child(
    node_t* node,
    uint32_t child_num);

/**
 * Enlarge a child of a node, given a child index.  By itself, this operation
 * should not affect things like the total number of leaf nodes in the tree and
 * the freshness of the checksums.  However, it may affect total allocation.
 */
extern node_enlarge_child_capacity_result_t enlarge_child_capacity(
    node_t* node,
    uint32_t child_num);

/**
 * Find the index of a child given a name.  Returns true iff the child was
 * found.
 *
 * If the child was found, return the index and the pointer to the child.
 */
extern node_search_children_result_t
search_children(const node_t* node, const char* name, const uint16_t name_sz);

/**
 * Find the index of a child given a node.  If the node is found, return its
 * index.  Otherwise return UINT32_MAX.
 */
extern uint32_t get_child_index(
    const node_t* const parent,
    const node_t* const child);

/**
 * Convenience function just to find a child.
 */
static inline node_t*
get_child_by_name(const node_t* node, const char* name, uint16_t name_sz) {
  node_search_children_result_t result = search_children(node, name, name_sz);

  return result.child;
}

#endif /* #ifndef __FASTMANIFEST_NODE_H__ */
