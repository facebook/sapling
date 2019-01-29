// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_iterator.c: declarations for traversing all the nodes of a tree
// in-order.
//
// no-check-code

#ifndef __FASTMANIFEST_TREE_ITERATOR_H__
#define __FASTMANIFEST_TREE_ITERATOR_H__

#include <stdbool.h>
#include <stdlib.h>

#include "node.h"

typedef struct _path_record_t {
  const node_t* node;
  size_t child_idx;

  // this is how much of the path was already present when we started walking
  // this node.  once we close this path, we should restore the iterator's
  // path_idx to this. value.
  size_t previous_path_idx;
} path_record_t;

struct _iterator_t {
  tree_t* copy;

  bool construct_paths;

  // track where we are in the iteration process.
  path_record_t* path_records;

  // this is where the next path record should be written to.
  size_t path_records_idx;

  // track the path, if path construction is requested.
  char* path;
  size_t path_idx;
  size_t path_sz;
};

#endif // #ifndef __FASTMANIFEST_TREE_ITERATOR_H__
