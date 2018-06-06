// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_convert_test.c: tests for methods to convert flat manifests to and
//                      from a tree.
//
// no-check-code

#include "hgext/extlib/cfastmanifest/tree.h"
#include "node.h"
#include "tests.h"

#define SIMPLE_CONVERT_INPUT                           \
  "abc\000b80de5d138758541c5f05265ad144ab9fa86d1db\n"  \
  "def\000f6d864039d10a8934d0d581d342780298aa9fb28l\n" \
  "ghi\0000f421b102b0baa760a5d4c5759f339cfc1f7d01b\n"

void test_simple_convert() {
  char input[] = SIMPLE_CONVERT_INPUT;
  size_t size = sizeof(input) - 1; // exempt the final null

  convert_from_flat_result_t convert_result = convert_from_flat(input, size);

  ASSERT(convert_result.code == CONVERT_FROM_FLAT_OK);

  tree_t* tree = convert_result.tree;
  ASSERT(tree->compacted == true);
  ASSERT(tree->num_leaf_nodes == 3);

  get_path_result_t get_result;

  get_result = get_path(tree, STRPLUSLEN("abc"));
  ASSERT(get_result.code == GET_PATH_OK);
  ASSERT(get_result.checksum_sz == SHA1_BYTES);
  ASSERT(
      memcmp(
          get_result.checksum,
          "\xb8\x0d\xe5\xd1\x38\x75\x85\x41\xc5\xf0\x52\x65\xad\x14\x4a\xb9\xfa"
          "\x86\xd1"
          "\xdb",
          SHA1_BYTES) == 0);
  ASSERT(get_result.flags == 0);

  get_result = get_path(tree, STRPLUSLEN("def"));
  ASSERT(get_result.code == GET_PATH_OK);
  ASSERT(get_result.checksum_sz == SHA1_BYTES);
  ASSERT(
      memcmp(
          get_result.checksum,
          "\xf6\xd8\x64\x03\x9d\x10\xa8\x93\x4d\x0d\x58\x1d\x34\x27\x80\x29\x8a"
          "\xa9\xfb\x28",
          SHA1_BYTES) == 0);
  ASSERT(get_result.flags == 'l');

  get_result = get_path(tree, STRPLUSLEN("ghi"));
  ASSERT(get_result.code == GET_PATH_OK);
  ASSERT(get_result.checksum_sz == SHA1_BYTES);
  ASSERT(
      memcmp(
          get_result.checksum,
          "\x0f\x42\x1b\x10\x2b\x0b\xaa\x76\x0a\x5d\x4c\x57\x59\xf3\x39\xcf\xc1"
          "\xf7\xd0\x1b",
          SHA1_BYTES) == 0);
  ASSERT(get_result.flags == 0);

  destroy_tree(convert_result.tree);
}

#define CONVERT_TREE_INPUT                                      \
  "abc\0007a091c781cf86fc5b7c2e93eb9f233c4220026a2\n"           \
  "abcd/efg\000f33dcd6a4ef633eb1fa02ec72cb76c4043390a50\n"      \
  "abcd/efgh/ijk\000b6fb5f7b2f3b499ad04b6e97f78904d5314ec690\n" \
  "abcd/efghi\00042aece97c3e7db21fbc7559918aba6b6e925a64d\n"    \
  "abcdefghi\000c4c71e7b43d108fb869c28107c39d21c166be837\n"

#define GET_TEST(tree, path_const, expected_result)                        \
  {                                                                        \
    get_path_result_t get_result = get_path(tree, STRPLUSLEN(path_const)); \
    ASSERT(get_result.code == expected_result);                            \
  }

void test_convert_tree() {
  char input[] = CONVERT_TREE_INPUT;
  size_t size = sizeof(input) - 1; // exempt the final null

  convert_from_flat_result_t convert_result = convert_from_flat(input, size);

  ASSERT(convert_result.code == CONVERT_FROM_FLAT_OK);

  tree_t* tree = convert_result.tree;
  ASSERT(tree->compacted == true);
  ASSERT(tree->num_leaf_nodes == 5);

  GET_TEST(tree, "abc", GET_PATH_OK);
  GET_TEST(tree, "abcd/efg", GET_PATH_OK);
  GET_TEST(tree, "abcd/efgh/ijk", GET_PATH_OK);
  GET_TEST(tree, "abcd/efghi", GET_PATH_OK);
  GET_TEST(tree, "abcdefghi", GET_PATH_OK);
  GET_TEST(tree, "abcdefghij", GET_PATH_NOT_FOUND);

  destroy_tree(convert_result.tree);
}

#define CONVERT_BIDIRECTIONALLY_INPUT                              \
  "abc\0007a091c781cf86fc5b7c2e93eb9f233c4220026a2\n"              \
  "abcd/efg\000f33dcd6a4ef633eb1fa02ec72cb76c4043390a50\n"         \
  "abcd/efgh/ijk/lm\000b6fb5f7b2f3b499ad04b6e97f78904d5314ec690\n" \
  "abcd/efghi\00042aece97c3e7db21fbc7559918aba6b6e925a64d\n"       \
  "abcdefghi\000c4c71e7b43d108fb869c28107c39d21c166be837\n"

void test_convert_bidirectionally() {
  char input[] = CONVERT_BIDIRECTIONALLY_INPUT;
  size_t size = sizeof(input) - 1; // exempt the final null

  convert_from_flat_result_t from_result = convert_from_flat(input, size);

  ASSERT(from_result.code == CONVERT_FROM_FLAT_OK);

  tree_t* tree = from_result.tree;

  convert_to_flat_result_t to_result = convert_to_flat(tree);
  ASSERT(to_result.flat_manifest_sz == size);
  ASSERT(
      memcmp(input, to_result.flat_manifest, to_result.flat_manifest_sz) == 0);
}

// this was exposed in #11145050
void test_remove_after_convert_from_flat() {
  convert_from_flat_result_t convert_result = convert_from_flat("", 0);

  ASSERT(convert_result.code == CONVERT_FROM_FLAT_OK);

  tree_t* tree = convert_result.tree;

  add_to_tree_t toadd[] = {
      {STRPLUSLEN("abc"), 12345, 5},
  };

  add_to_tree(tree, toadd, sizeof(toadd) / sizeof(add_to_tree_t));
  remove_path(tree, STRPLUSLEN("abc"));

  convert_to_flat_result_t to_result = convert_to_flat(tree);
  ASSERT(to_result.code == CONVERT_TO_FLAT_OK);
  ASSERT(to_result.flat_manifest_sz == 0);
}

void test_empty_convert_to_flat() {
  tree_t* empty_tree = alloc_tree();
  convert_to_flat_result_t to_result = convert_to_flat(empty_tree);
  ASSERT(to_result.flat_manifest_sz == 0);
}

int main(int argc, char* argv[]) {
  test_simple_convert();
  test_convert_tree();
  test_convert_bidirectionally();
  test_remove_after_convert_from_flat();
  test_empty_convert_to_flat();

  return 0;
}
