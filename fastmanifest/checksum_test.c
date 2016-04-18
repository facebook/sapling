// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum_test.c: tests for recalculating the checksums for intermediate
//                  nodes in a tree.
//
// no-check-code

#include "checksum.h"
#include "tree.h"

void test_empty_tree() {
  tree_t* tree = alloc_tree();
  update_checksums(tree);
}

int main(int argc, char *argv[]) {
  test_empty_tree();

  return 0;
}
