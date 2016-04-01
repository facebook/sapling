// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_copy_test.c: tests to verify methods to make a copy of a tree.
//
// no-check-code

#include "node.h"
#include "tests.h"
#include "tree.h"

void test_copy_empty() {
  tree_t* src = alloc_tree();
  tree_t* dst = copy(src);

  ASSERT(dst != NULL);
  ASSERT(dst->compacted == true);
  ASSERT(dst->num_leaf_nodes == 0);
  ASSERT(dst->consumed_memory == src->consumed_memory);

  destroy_tree(src);
  destroy_tree(dst);
}

void test_copy_empty_chain() {
  tree_t* original = alloc_tree();

  tree_t* src = original;

  for (int ix = 0; ix < 10; ix ++) {
    tree_t* dst = copy(src);

    ASSERT(dst != NULL);
    ASSERT(dst->compacted == true);
    ASSERT(dst->num_leaf_nodes == 0);
    ASSERT(dst->consumed_memory == src->consumed_memory);

    tree_t* old_src = src;
    src = dst;

    destroy_tree(old_src);
  }
}

typedef struct {
  char* path;
  size_t path_sz;
  uint8_t* checksum;
  uint8_t flags;
} copy_normal_tree_data_t;
#define COPY_NORMAL_TREE_DATA(path, checksum, flags)                      \
  (copy_normal_tree_data_t)                                               \
    {                                                                     \
      path,                                                               \
      sizeof(path) - 1,                                                   \
      (uint8_t*) checksum,                                                \
      flags,                                                              \
    }
void test_copy_normal_tree() {
  copy_normal_tree_data_t input[] = {
      COPY_NORMAL_TREE_DATA("abc",
          "\xe7\xf5\xdd\xad\x5e\x13\x86\x4e\x25\x30\x41\x3a\x69\x8e\x19\xd4\x25"
              "\xc8\x12\x02",
          0x23),
      COPY_NORMAL_TREE_DATA("ab/cde",
          "\x7c\x6a\x4b\x0a\x05\x91\x6c\x89\x9d\x8a\xe6\x38\xcf\x38\x93\x2e"
              "\x4f\x09\xed\x57",
          0x9b),
      COPY_NORMAL_TREE_DATA("abcd/ef",
          "\x3e\x4d\xf1\xe0\x46\x4a\x3e\xb9\x6b\x8d\x55\x6c\x3b\x6b\x00\xee"
              "\x4f\x77\x71\x9e",
          0xda),
      COPY_NORMAL_TREE_DATA("abcd/efg/hi",
          "\x98\x2f\x46\x90\xfe\xc1\xbc\xe0\x8b\xf7\xa5\x47\x65\xe3\xf4\x16"
              "\x5b\xf4\xba\x7c",
          0x44),
  };
  size_t input_sz = sizeof(input) / sizeof(copy_normal_tree_data_t) ;
  tree_t* src = alloc_tree();

  for (size_t ix = 0; ix < input_sz; ix ++) {
    add_update_path_result_t result =
        add_or_update_path(
            src,
            input[ix].path, input[ix].path_sz,
            input[ix].checksum, SHA1_BYTES,
            input[ix].flags);
    ASSERT(result == ADD_UPDATE_PATH_OK);
  }

  ASSERT(src->compacted == false);
  ASSERT(src->num_leaf_nodes == input_sz);

  tree_t* dst = copy(src);

  for (size_t ix = 0; ix < input_sz; ix ++) {
    get_path_result_t get_result =
        get_path(dst, input[ix].path, input[ix].path_sz);

    ASSERT(get_result.code == GET_PATH_OK);
    ASSERT(get_result.node->checksum_valid == true);
    ASSERT(get_result.node->checksum_sz == SHA1_BYTES);
    ASSERT(memcmp(
        get_result.node->checksum, input[ix].checksum, SHA1_BYTES) == 0);
    ASSERT(get_result.node->flags == input[ix].flags);
  }
}

int main(int argc, char* argv[]) {
  test_copy_empty();
  test_copy_empty_chain();
  test_copy_normal_tree();
}
