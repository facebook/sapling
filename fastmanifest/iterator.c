// Copyright 2016-present Facebook. All Rights Reserved.
//
// iterator.c: implementation for traversing all the nodes of a tree in-order.

typedef struct _path_record_t {
  node_t* next_node;
  uint32_t next_child;
} path_record_t;

struct _iterator_t {
  tree_t* copy;
  uint16_t path_alloc_cnt;
};
