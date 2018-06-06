// Copyright 2016-present Facebook. All Rights Reserved.
//
// internal_result.h: result codes for internal APIs.  obviously, this is for
// internal use only.
//
// no-check-code

#ifndef FASTMANIFEST_INTERNAL_RESULT_H
#define FASTMANIFEST_INTERNAL_RESULT_H

#include <stdint.h>

typedef enum {
  ADD_CHILD_OK,
  ADD_CHILD_ILLEGAL_PARENT,
  ADD_CHILD_ILLEGAL_CHILD,
  CONFLICTING_ENTRY_PRESENT,
  NEEDS_LARGER_NODE,
} node_add_child_result_t;

typedef enum {
  REMOVE_CHILD_OK,
  REMOVE_CHILD_ILLEGAL_PARENT,
  REMOVE_CHILD_ILLEGAL_INDEX,
} node_remove_child_result_t;

typedef enum {
  ENLARGE_OK,
  ENLARGE_OOM,
  ENLARGE_ILLEGAL_PARENT,
  ENLARGE_ILLEGAL_INDEX,
} node_enlarge_child_capacity_code_t;
typedef struct _node_enlarge_child_capacity_result_t {
  node_enlarge_child_capacity_code_t code;
  struct _node_t* old_child;
  struct _node_t* new_child;
} node_enlarge_child_capacity_result_t;

typedef struct _node_search_children_result_t {
  struct _node_t* child;
  uint32_t child_num;
} node_search_children_result_t;

#endif // FASTMANIFEST_INTERNAL_RESULT_H
