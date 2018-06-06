// Copyright 2016-present Facebook. All Rights Reserved.
//
// checksum_test.c: tests for recalculating the checksums for intermediate
//                  nodes in a tree.
//
// no-check-code

#include "checksum.h"
#include "hgext/extlib/cfastmanifest/tree.h"
#include "node.h"
#include "tests.h"

static void test_empty_tree() {
  tree_t* tree = alloc_tree();

  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == false);

  update_checksums(tree);
  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == true);
}

typedef struct {
  char* path;
  bool expected_checksum_valid;
} path_checksum_t;

/**
 * Verify that when a path is added or removed, the affected paths have their
 * checksums invalidated.
 */
static void test_updates_reset_checksums() {
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix++) {
    checksum[ix] = (uint8_t)ix;
  }

  tree_t* tree = alloc_tree();

  char* paths_to_add[] = {
      "abc",
      "ab/def",
      "ab/defg/hi",
      "ab/defg/h/ij/kl",
      "ab/defg/h/ijk",
      "ab/defg/h/i/jkl/mn/op/qr",
      "ab/defg/h/i/jkl/mn/op/qrs",
  };
  const size_t num_paths = sizeof(paths_to_add) / sizeof(*paths_to_add);

  for (size_t ix = 0; ix < num_paths; ix++) {
    add_update_path_result_t add_result = add_or_update_path(
        tree, STRPLUSLEN(paths_to_add[ix]), checksum, SHA1_BYTES, 0);
    ASSERT(add_result == ADD_UPDATE_PATH_OK);
  }

  update_checksums(tree);
  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == true);

  ASSERT(
      add_or_update_path(
          tree, STRPLUSLEN("ab/defg/h/ijk"), checksum, SHA1_BYTES, 0) ==
      ADD_UPDATE_PATH_OK);

  path_checksum_t dirs_to_check_after_add[] = {
      {"abc", true},
      {"ab/", false},
      {"ab/defg/", false},
      {"ab/defg/h/", false},
      {"ab/defg/h/i/", true},
      {"ab/defg/h/i/jkl/", true},
      {"ab/defg/h/i/jkl/mn/", true},
      {"ab/defg/h/i/jkl/mn/op/", true},
      {"ab/defg/h/ij/", true},
  };
  size_t num_dirs =
      sizeof(dirs_to_check_after_add) / sizeof(*dirs_to_check_after_add);

  for (size_t ix = 0; ix < num_dirs; ix++) {
    get_path_unfiltered_result_t get_result =
        get_path_unfiltered(tree, STRPLUSLEN(dirs_to_check_after_add[ix].path));
    ASSERT(get_result.code == GET_PATH_OK);
    ASSERT(
        get_result.node->checksum_valid ==
        dirs_to_check_after_add[ix].expected_checksum_valid);
  }
  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == false);

  update_checksums(tree);
  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == true);

  ASSERT(
      remove_path(tree, STRPLUSLEN("ab/defg/h/i/jkl/mn/op/qrs")) ==
      REMOVE_PATH_OK);

  path_checksum_t dirs_to_check_after_remove[] = {
      {"abc", true},
      {"ab/", false},
      {"ab/defg/", false},
      {"ab/defg/h/", false},
      {"ab/defg/h/i/", false},
      {"ab/defg/h/i/jkl/", false},
      {"ab/defg/h/i/jkl/mn/", false},
      {"ab/defg/h/i/jkl/mn/op/", false},
      {"ab/defg/h/ij/", true},
  };
  num_dirs =
      sizeof(dirs_to_check_after_remove) / sizeof(*dirs_to_check_after_remove);

  for (size_t ix = 0; ix < num_dirs; ix++) {
    get_path_unfiltered_result_t get_result = get_path_unfiltered(
        tree, STRPLUSLEN(dirs_to_check_after_remove[ix].path));
    ASSERT(get_result.code == GET_PATH_OK);
    ASSERT(
        get_result.node->checksum_valid ==
        dirs_to_check_after_remove[ix].expected_checksum_valid);
  }
  ASSERT(get_child_by_index(tree->shadow_root, 0)->checksum_valid == false);
}

int main(int argc, char* argv[]) {
  test_empty_tree();
  test_updates_reset_checksums();

  return 0;
}
