// Copyright 2016-present Facebook. All Rights Reserved.
//
// node.c: implementation for representing a node in a tree.
//
// no-check-code

#include <stdlib.h>

#include "bsearch.h"
#include "node.h"

static size_t calculate_required_size(uint16_t name_sz, uint32_t num_children) {
  intptr_t address = get_child_ptr_base_offset(name_sz);

  return address + (sizeof(ptrdiff_t) * num_children);
}

static void initialize_node(
    node_t* node,
    size_t block_sz,
    const char* name,
    uint16_t name_sz) {
  if (!VERIFY_BLOCK_SZ(block_sz)) {
    abort();
  }
  node->block_sz = (uint32_t)block_sz;
  node->num_children = 0;
  node->name_sz = name_sz;
  node->in_use = true;
  node->type = TYPE_UNDEFINED;
  node->checksum_valid = false;
  memcpy(&node->name, name, name_sz);
}

node_t* alloc_node(const char* name, uint16_t name_sz, uint32_t max_children) {
  size_t size = calculate_required_size(name_sz, max_children);
  node_t* result = (node_t*)malloc(size);
  if (result == NULL) {
    return result;
  }

  initialize_node(result, size, name, name_sz);
  return result;
}

void* setup_node(
    void* ptr,
    size_t ptr_size_limit,
    const char* name,
    uint16_t name_sz,
    uint32_t max_children) {
  size_t size = calculate_required_size(name_sz, max_children);
  if (size > ptr_size_limit) {
    return NULL;
  }

  node_t* node = (node_t*)ptr;
  intptr_t next = (intptr_t)ptr;
  next += size;

  initialize_node(node, size, name, name_sz);

  return (void*)next;
}

node_t* clone_node(const node_t* node) {
  uint32_t old_capacity = max_children(node);
  uint64_t desired_new_capacity =
      (((uint64_t)old_capacity) * (100 + STORAGE_INCREMENT_PERCENTAGE)) / 100;
  if (desired_new_capacity - old_capacity < MIN_STORAGE_INCREMENT) {
    desired_new_capacity = old_capacity + MIN_STORAGE_INCREMENT;
  } else if (desired_new_capacity - old_capacity > MAX_STORAGE_INCREMENT) {
    desired_new_capacity = old_capacity + MAX_STORAGE_INCREMENT;
  }

  uint32_t new_capacity;
  if (desired_new_capacity > UINT32_MAX) {
    new_capacity = UINT32_MAX;
  } else {
    new_capacity = (uint32_t)desired_new_capacity;
  }

  node_t* clone = alloc_node(node->name, node->name_sz, new_capacity);
  if (clone == NULL) {
    return NULL;
  }

  // copy metadata over.
  clone->num_children = node->num_children;
  if (node->checksum_valid) {
    memcpy(clone->checksum, node->checksum, sizeof(node->checksum));
    clone->checksum_sz = node->checksum_sz;
  }
  clone->type = node->type;
  clone->checksum_valid = node->checksum_valid;
  clone->flags = node->flags;

  // calculate the difference we need to apply to the relative pointers.
  ptrdiff_t delta = ((intptr_t)node) - ((intptr_t)clone);

  // get the child pointer base of each node.
  const ptrdiff_t* node_base = get_child_ptr_base_const(node);
  ptrdiff_t* clone_base = get_child_ptr_base(clone);

  for (int ix = 0; ix < node->num_children; ix++) {
    clone_base[ix] = node_base[ix] + delta;
  }

  return clone;
}

typedef struct {
  const char* name;
  uint16_t name_sz;
} find_child_struct_t;

#define NAME_NODE_COMPARE(nameobject, relptr, context)   \
  (name_compare(                                         \
      ((const find_child_struct_t*)nameobject)->name,    \
      ((const find_child_struct_t*)nameobject)->name_sz, \
      get_child_from_diff((node_t*)context, *((ptrdiff_t*)relptr))))

static CONTEXTUAL_COMPARATOR_BUILDER(name_node_cmp, NAME_NODE_COMPARE);

node_add_child_result_t add_child(node_t* node, const node_t* child) {
  // verify parent node.
  if (!node->in_use ||
      !(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT)) {
    return ADD_CHILD_ILLEGAL_PARENT;
  }

  // do we have enough space?  if not, we need to request a new space.
  if (node->num_children + 1 > max_children(node)) {
    return NEEDS_LARGER_NODE;
  }

  // verify child node.
  if (!child->in_use) {
    return ADD_CHILD_ILLEGAL_CHILD;
  }

  ptrdiff_t* base = get_child_ptr_base(node);
  find_child_struct_t needle = {child->name, child->name_sz};
  size_t offset = bsearch_between(
      &needle,
      get_child_ptr_base(node),
      node->num_children,
      sizeof(ptrdiff_t),
      name_node_cmp,
      node);

  if (offset < node->num_children) {
    // displacing something.  ensure we don't have a conflict.
    ptrdiff_t diff = base[offset];
    node_t* old_child = get_child_from_diff(node, diff);

    if (name_compare(child->name, child->name_sz, old_child) == 0) {
      return CONFLICTING_ENTRY_PRESENT;
    }
  }

  if (offset < node->num_children) {
    // move the remaining entries down to make space.  let's say we have 3
    // elements.  if we're supposed to insert at offset 1, then we need to move
    // elements at offset 1 & 2 down.
    memmove(
        &base[offset + 1],
        &base[offset],
        sizeof(ptrdiff_t) * (node->num_children - offset));
  }

  // bump the number of children we have.
  node->num_children++;

  // write the entry
  set_child_by_index(node, offset, child);

  return ADD_CHILD_OK;
}

node_remove_child_result_t remove_child(node_t* node, uint32_t child_num) {
  // verify parent node.
  if (!node->in_use ||
      !(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT)) {
    return REMOVE_CHILD_ILLEGAL_PARENT;
  }

  // do we have enough space?  if not, we need to request a new space.
  if (child_num >= node->num_children) {
    return REMOVE_CHILD_ILLEGAL_INDEX;
  }

  if (child_num < node->num_children - 1) {
    // we need to compact the existing entries.
    ptrdiff_t* base = get_child_ptr_base(node);

    memmove(
        &base[child_num],
        &base[child_num + 1],
        sizeof(ptrdiff_t) * (node->num_children - 1 - child_num));
  }

  // decrement the number of children we have.
  node->num_children--;

  return REMOVE_CHILD_OK;
}

node_enlarge_child_capacity_result_t enlarge_child_capacity(
    node_t* node,
    uint32_t child_num) {
  node_enlarge_child_capacity_result_t result;
  // strictly these shouldn't be necessary, because we only read these fields
  // if we succeed, and that code path does set the fields.  however, gcc
  // doesn'tknow that and throws a fit.
  result.old_child = NULL;
  result.new_child = NULL;

  // verify parent node.
  if (!node->in_use) {
    result.code = ENLARGE_ILLEGAL_PARENT;
    return result;
  }

  // verify child index.
  if (child_num >= node->num_children) {
    result.code = ENLARGE_ILLEGAL_INDEX;
    return result;
  }

  node_t* old_child = get_child_by_index(node, child_num);
  node_t* new_child = clone_node(old_child);

  if (new_child == NULL) {
    result.code = ENLARGE_OOM;
    return result;
  }

  // write the entry
  set_child_by_index(node, child_num, new_child);

  result.code = ENLARGE_OK;
  result.old_child = old_child;
  result.new_child = new_child;

  return result;
}

node_search_children_result_t
search_children(const node_t* node, const char* name, const uint16_t name_sz) {
  const ptrdiff_t* base = get_child_ptr_base_const(node);
  find_child_struct_t needle = {name, name_sz};
  size_t offset = bsearch_between(
      &needle,
      get_child_ptr_base_const(node),
      node->num_children,
      sizeof(ptrdiff_t),
      name_node_cmp,
      node);

  if (offset >= node->num_children) {
    return COMPOUND_LITERAL(node_search_children_result_t){NULL, UINT32_MAX};
  }

  // ensure the spot we found is an exact match.
  ptrdiff_t diff = base[offset];
  node_t* child = get_child_from_diff(node, diff);
  if (name_compare(name, name_sz, child) == 0) {
    // huzzah, we found it.
    return COMPOUND_LITERAL(node_search_children_result_t){child,
                                                           (uint32_t)offset};
  }

  return COMPOUND_LITERAL(node_search_children_result_t){NULL, UINT32_MAX};
}

uint32_t get_child_index(
    const node_t* const parent,
    const node_t* const child) {
  const ptrdiff_t* base = get_child_ptr_base_const(parent);
  for (uint32_t child_num = 0; child_num < parent->num_children; child_num++) {
    if (((intptr_t)parent) + base[child_num] == (intptr_t)child) {
      return child_num;
    }
  }

  return UINT32_MAX;
}
