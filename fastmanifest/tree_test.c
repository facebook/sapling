// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_test.c: tests for core methods for tree creation and manipulation.

#include "node.h"
#include "tree.h"
#include "tests.h"

/**
 * Initializes a tree and verifies that the initial two nodes are created
 * correctly.
 */
void tree_init_test() {
  tree_t* tree = alloc_tree();
  node_t* shadow_root = tree->shadow_root;

  ASSERT(shadow_root != NULL);
  ASSERT(shadow_root->num_children == 1);

  node_t* real_root = get_child_by_index(shadow_root, 0);
  ASSERT(real_root != NULL);
  ASSERT(real_root->num_children == 0);

  ASSERT(tree->consumed_memory == shadow_root->block_sz + real_root->block_sz);
}

/**
 * Initializes a tree and adds a node.
 */
void tree_add_child() {
  tree_t* tree = alloc_tree();
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t result =
      add_or_update_path(tree, STRPLUSLEN("abc"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);
  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 1);
}

/**
 * Initializes a tree and adds a file and a directory containing a file.
 */
void tree_add_0_cousin_once_removed() {
  tree_t* tree = alloc_tree();
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t result;

  result = add_or_update_path(tree, STRPLUSLEN("ab"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  result = add_or_update_path(tree, STRPLUSLEN("abc/de"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  // verify the shadow root.
  ASSERT(tree->shadow_root->num_children == 1);

  // obtain the true root, verify that.
  node_t* real_root = get_child_by_index(tree->shadow_root, 0);

  // verify the real root.
  ASSERT(real_root->num_children == 2);

  // first child should be 'ab'
  node_t* root_first_child = get_child_by_index(real_root, 0);
  ASSERT(root_first_child->num_children == 0);
  ASSERT(root_first_child->type == TYPE_LEAF);
  ASSERT(name_compare("ab", 2, root_first_child) == 0);

  // second child should be 'abc'
  node_t* root_second_child = get_child_by_index(real_root, 1);
  ASSERT(root_second_child->num_children == 1);
  ASSERT(root_second_child->type == TYPE_IMPLICIT);
  ASSERT(name_compare("abc", 3, root_second_child) == 0);
}

/**
 * Initializes a tree and adds a long skinny branch.
 */
void tree_add_long_skinny_branch() {
  tree_t* tree = alloc_tree();
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t result;

  result = add_or_update_path(tree, STRPLUSLEN("ab"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  result = add_or_update_path(tree, STRPLUSLEN("abc/de"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  result = add_or_update_path(tree, STRPLUSLEN("abc/def/gh"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  result = add_or_update_path(tree, STRPLUSLEN("abc/def/ghi/jkl"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 4);
}

/**
 * Initializes a tree and adds a bushy branch.
 */
void tree_add_bushy_branch() {
  tree_t* tree = alloc_tree();
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t result;

  result = add_or_update_path(tree, STRPLUSLEN("ab"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);

  char tempbuffer[] = "abc/de?";

  for (int ix = 0; ix < 26; ix ++) {
    tempbuffer[6] = 'a' + ix;
    result = add_or_update_path(tree, STRPLUSLEN(tempbuffer),
        checksum, SHA1_BYTES, 0);
    ASSERT(result == ADD_UPDATE_PATH_OK);
  }

  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 27);
}

/**
 * Initializes a tree, adds a node (N0), and then adds a second node (N1) that
 * would require a directory be created where N0 is.
 */
void tree_add_conflict() {
  tree_t* tree = alloc_tree();
  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t result =
      add_or_update_path(tree, STRPLUSLEN("abc/def"),
          checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_OK);
  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 1);

  result = add_or_update_path(tree, STRPLUSLEN("abc/def/ghi"),
      checksum, SHA1_BYTES, 0);
  ASSERT(result == ADD_UPDATE_PATH_CONFLICT);
  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 1);
}

/**
 * Initializes a tree and attempt to retrieve a couple paths that are not there.
 */
void tree_get_empty() {
  tree_t* tree = alloc_tree();

  get_path_result_t result = get_path(tree, STRPLUSLEN("abc"));
  ASSERT(result.code == GET_PATH_NOT_FOUND);

  result = get_path(tree, STRPLUSLEN("abc/def"));
  ASSERT(result.code == GET_PATH_NOT_FOUND);
}

/**
 * Initializes a tree, adds a single path, and attempt to retrieve it.
 */
#define ADD_GET_SIMPLE_FLAGS 0x2e
void tree_add_get_simple() {
  tree_t* tree = alloc_tree();

  uint8_t checksum[SHA1_BYTES];

  for (int ix = 0; ix < SHA1_BYTES; ix ++) {
    checksum[ix] = (uint8_t) ix;
  }

  add_update_path_result_t add_result =
      add_or_update_path(tree, STRPLUSLEN("abc"),
                         checksum, SHA1_BYTES, ADD_GET_SIMPLE_FLAGS);
  ASSERT(add_result == ADD_UPDATE_PATH_OK);
  ASSERT(tree->compacted == false);
  ASSERT(tree->num_leaf_nodes == 1);

  get_path_result_t get_result = get_path(tree, STRPLUSLEN("abc"));
  ASSERT(get_result.code == GET_PATH_OK);
  ASSERT(get_result.node != NULL);
  ASSERT(get_result.node->checksum_valid == true);
  ASSERT(get_result.node->checksum_sz == SHA1_BYTES);
  ASSERT(memcmp(checksum, get_result.node->checksum, SHA1_BYTES) == 0);
  ASSERT(get_result.node->flags == ADD_GET_SIMPLE_FLAGS);

  get_result = get_path(tree, STRPLUSLEN("abc/def"));
  ASSERT(get_result.code == GET_PATH_NOT_FOUND);
  ASSERT(get_result.node == NULL);
}

int main(int argc, char* argv[]) {
  tree_init_test();
  tree_add_child();
  tree_add_0_cousin_once_removed();
  tree_add_long_skinny_branch();
  tree_add_bushy_branch();
  tree_add_conflict();
  tree_get_empty();
  tree_add_get_simple();
  return 0;
}
