// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_copy.c: methods to make a copy of a tree.  the new instance is compacted
//              into an arena.
//
// no-check-code

#include "hgext/extlib/cfastmanifest/tree.h"
#include "internal_result.h"
#include "node.h"
#include "path_buffer.h"
#include "tree_arena.h"

typedef enum {
  COPY_OK,
  COPY_OOM,
  COPY_WTF,
} copy_helper_result_t;

/**
 * Clones `src` and adds it as the Nth child of `dst_parent`, where N ==
 * `child_num`.
 *
 * `child_num` must be <= `dst_parent->num_children`.
 */
copy_helper_result_t copy_helper(
    tree_t* dst_tree,
    const node_t* src,
    node_t* dst_parent,
    size_t child_num) {
  arena_alloc_node_result_t alloc_result = arena_alloc_node_strict(
      dst_tree, src->name, src->name_sz, src->num_children);
  switch (alloc_result.code) {
    case ARENA_ALLOC_OK:
      break;
    case ARENA_ALLOC_OOM:
      return COPY_OOM;
    case ARENA_ALLOC_EXCEEDED_LIMITS:
      return COPY_WTF;
  }

  // copy the attributes
  node_t* dst = alloc_result.node;
  if (src->checksum_valid) {
    memcpy(dst->checksum, src->checksum, src->checksum_sz);
    dst->checksum_sz = src->checksum_sz;
  }
  dst->checksum_valid = src->checksum_valid;
  dst->flags = src->flags;
  dst->type = src->type;

  // typically we don't like touching this field manually, but to
  // `set_child_by_index` requires the index be < num_children.
  dst->num_children = src->num_children;

  if (dst->type == TYPE_LEAF) {
    dst_tree->num_leaf_nodes++;
  } else {
    for (int ix = 0; ix < src->num_children; ix++) {
      copy_helper_result_t copy_result =
          copy_helper(dst_tree, get_child_by_index(src, ix), dst, ix);

      if (copy_result != COPY_OK) {
        return copy_result;
      }
    }
  }

  set_child_by_index(dst_parent, child_num, dst);

  return COPY_OK;
}

tree_t* copy_tree(const tree_t* src) {
  tree_t* dst = alloc_tree_with_arena(src->consumed_memory);

  // prerequisite for using copy_helper is that child_num must be <
  // dst_parent->num_children, so we artificially bump up the num_chlidren
  // for the shadow root.
  assert(max_children(dst->shadow_root) > 0);
  dst->shadow_root->num_children = 1;

  copy_helper_result_t copy_result = copy_helper(
      dst, get_child_by_index(src->shadow_root, 0), dst->shadow_root, 0);

  switch (copy_result) {
    case COPY_OK:
      dst->compacted = true;
      return dst;
    default:
      destroy_tree(dst);
      return NULL;
  }
}

typedef enum {
  // returned if the child was copied, but not all of its descendants are not
  // copied.
  FILTER_COPY_OK,

  // returned if the child was copied, and all of its descendants are also
  // copied.
  FILTER_COPY_OK_RECURSIVELY,

  // returned if the child was not copied.
  FILTER_COPY_NOT_COPIED,
  FILTER_COPY_OOM,
  FILTER_COPY_WTF,
} filter_copy_helper_result_t;

typedef struct {
  bool (*filter)(char* path, size_t path_sz, void* callback_context);

  // use this buffer to construct the paths.
  char* path;
  size_t path_idx;
  size_t path_sz;

  void* callback_context;
} filter_copy_context_t;

/**
 * Clones `src` and adds it as the Nth child of `dst_parent`, where N ==
 * `child_num`, but only iff the clone has children.
 *
 * `child_num` must be <= `dst_parent->num_children`.
 */
filter_copy_helper_result_t filter_copy_helper(
    tree_t* dst_tree,
    filter_copy_context_t* context,
    const node_t* src,
    node_t* dst_parent,
    size_t child_num) {
  filter_copy_helper_result_t result;

  // save the old path size so we can restore when we exit.
  size_t prev_path_idx = context->path_idx;

  // construct the path.
  if (src->type != TYPE_ROOT) {
    if (PATH_APPEND(
            &context->path,
            &context->path_idx,
            &context->path_sz,
            (char*)src->name,
            src->name_sz) == false) {
      return FILTER_COPY_OOM;
    }
  }

  if (src->type == TYPE_LEAF) {
    // call the filter and determine whether this node should be added.
    if (context->filter(
            context->path, context->path_idx, context->callback_context)) {
      dst_tree->num_leaf_nodes++;

      arena_alloc_node_result_t alloc_result = arena_alloc_node_strict(
          dst_tree, src->name, src->name_sz, src->num_children);
      switch (alloc_result.code) {
        case ARENA_ALLOC_OK:
          break;
        case ARENA_ALLOC_OOM:
          return FILTER_COPY_OOM;
        case ARENA_ALLOC_EXCEEDED_LIMITS:
          return FILTER_COPY_WTF;
      }

      // copy the attributes
      node_t* dst = alloc_result.node;
      if (src->checksum_valid) {
        memcpy(dst->checksum, src->checksum, src->checksum_sz);
        dst->checksum_sz = src->checksum_sz;
      }
      dst->checksum_valid = src->checksum_valid;
      dst->flags = src->flags;
      dst->type = src->type;

      set_child_by_index(dst_parent, child_num, dst);

      result = FILTER_COPY_OK_RECURSIVELY;
    } else {
      result = FILTER_COPY_NOT_COPIED;
    }

    // restore the path and exit.
    context->path_idx = prev_path_idx;
    return result;
  }

  // allocate a temporary node to hold the entries.
  node_t* temp_node = alloc_node(NULL, 0, src->num_children);
  if (temp_node == NULL) {
    return FILTER_COPY_OOM;
  }

  // set enough fields such that we can write children.
  temp_node->type = src->type;
  // typically we don't like touching this field manually, but to
  // `set_child_by_index` requires the index be < num_children.
  temp_node->num_children = src->num_children;

  child_num_t dst_child_index = 0;

  // assume everything gets copied.
  bool recursive = true;

  for (child_num_t ix = 0; ix < src->num_children; ix++) {
    filter_copy_helper_result_t filter_copy_result = filter_copy_helper(
        dst_tree,
        context,
        get_child_by_index(src, ix),
        temp_node,
        dst_child_index);

    switch (filter_copy_result) {
      case FILTER_COPY_OK:
        recursive = false;
      case FILTER_COPY_OK_RECURSIVELY:
        dst_child_index++;
        break;

      case FILTER_COPY_NOT_COPIED:
        recursive = false;
        break;

      case FILTER_COPY_OOM:
      case FILTER_COPY_WTF:
        return filter_copy_result;
    }
  }

  // how many children were written?
  if (dst_child_index == 0 && src->type != TYPE_ROOT) {
    // none, that means we shouldn't write to our parent either.
    result = FILTER_COPY_NOT_COPIED;
  } else {
    // hey, we wrote something.  allocate within the arena and copy the
    // entries over.

    arena_alloc_node_result_t alloc_result = arena_alloc_node_strict(
        dst_tree, src->name, src->name_sz, src->num_children);
    switch (alloc_result.code) {
      case ARENA_ALLOC_OK:
        break;
      case ARENA_ALLOC_OOM:
        return FILTER_COPY_OOM;
      case ARENA_ALLOC_EXCEEDED_LIMITS:
        return FILTER_COPY_WTF;
    }

    // copy the attributes
    node_t* dst = alloc_result.node;
    if (src->checksum_valid && recursive) {
      memcpy(dst->checksum, src->checksum, src->checksum_sz);
      dst->checksum_sz = src->checksum_sz;
      dst->checksum_valid = true;
    } else {
      dst->checksum_valid = false;
    }
    dst->flags = src->flags;
    dst->type = src->type;

    // typically we don't like touching this field manually, but to
    // `set_child_by_index` requires the index be < num_children.
    dst->num_children = dst_child_index;

    for (child_num_t ix = 0; ix < dst_child_index; ix++) {
      const node_t* child = get_child_by_index(temp_node, ix);
      set_child_by_index(dst, ix, child);
    }

    set_child_by_index(dst_parent, child_num, dst);

    result = recursive ? FILTER_COPY_OK_RECURSIVELY : FILTER_COPY_OK;
  }

  free(temp_node);
  context->path_idx = prev_path_idx;

  return result;
}

tree_t* filter_copy(
    const tree_t* src,
    bool (*filter)(char* path, size_t path_sz, void* callback_context),
    void* context) {
  tree_t* dst = alloc_tree_with_arena(src->consumed_memory);
  filter_copy_context_t filter_copy_context;

  filter_copy_context.path = malloc(DEFAULT_PATH_BUFFER_SZ);
  filter_copy_context.path_idx = 0;
  filter_copy_context.path_sz = DEFAULT_PATH_BUFFER_SZ;
  filter_copy_context.filter = filter;
  filter_copy_context.callback_context = context;

  // prerequisite for using filter_copy_helper is that child_num must be <
  // dst_parent->num_children, so we artificially bump up the num_chlidren
  // for the shadow root.
  assert(max_children(dst->shadow_root) > 0);
  dst->shadow_root->num_children = 1;

  filter_copy_helper_result_t filter_copy_result = filter_copy_helper(
      dst,
      &filter_copy_context,
      get_child_by_index(src->shadow_root, 0),
      dst->shadow_root,
      0);

  switch (filter_copy_result) {
    case FILTER_COPY_OK:
    case FILTER_COPY_OK_RECURSIVELY:
      dst->compacted = true;
      return dst;
    default:
      destroy_tree(dst);
      return NULL;
  }
}
