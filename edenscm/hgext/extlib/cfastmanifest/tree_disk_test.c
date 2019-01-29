// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_disk_test.c: tests to verify tree_disk
//
// no-check-code

#include <stdlib.h>
#include <unistd.h>

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "tests.h"

#define TMPFILE_TEMPLATE "/tmp/tree_disk_test.XXXXXX"

// this is defined in tree_disk.c.  it's public but not advertised.
extern write_to_file_result_t write_to_file_helper(
    tree_t* tree,
    char* fname,
    size_t fname_sz,
    bool initialize_padding);

static char* get_tempfile() {
  char* template = strdup(TMPFILE_TEMPLATE);
  ASSERT(template != NULL);

  memcpy(template, TMPFILE_TEMPLATE, sizeof(TMPFILE_TEMPLATE));
  int fd = mkstemp(template);
  ASSERT(fd != -1);

  close(fd);

  return template;
}

/**
 * A diff callback that should never be called.
 */
static void never_called_callback(
    const char* path,
    const size_t path_sz,
    const bool left_present,
    const uint8_t* left_checksum,
    const uint8_t left_checksum_sz,
    const uint8_t left_flags,
    const bool right_present,
    const uint8_t* right_checksum,
    const uint8_t right_checksum_sz,
    const uint8_t right_flags,
    void* context) {
  ASSERT(false);
}

static void save_load_empty_tree() {
  tree_t* tree = alloc_tree();

  char* tempfile = get_tempfile();
  write_to_file_result_t write_result =
      write_to_file_helper(tree, STRPLUSLEN(tempfile), true);
  ASSERT(write_result == WRITE_TO_FILE_OK);

  read_from_file_result_t read_result = read_from_file(STRPLUSLEN(tempfile));

  ASSERT(read_result.code == READ_FROM_FILE_OK);
  diff_result_t diff_result =
      diff_trees(tree, read_result.tree, false, never_called_callback, NULL);

  ASSERT(diff_result == DIFF_OK);
}

static void save_load_small_tree() {
  tree_t* tree = alloc_tree();

  add_to_tree_t toadd[] = {
      {STRPLUSLEN("abc"), 12345, 5},
      {STRPLUSLEN("ab/cdef/gh"), 64342, 55},
      {STRPLUSLEN("ab/cdef/ghi/jkl"), 51545, 57},
      {STRPLUSLEN("ab/cdef/ghi/jklm"), 54774, 12},
      {STRPLUSLEN("ab/cdef/ghi/jklmn"), 48477, 252},
      {STRPLUSLEN("a"), 577, 14},
  };

  add_to_tree(tree, toadd, sizeof(toadd) / sizeof(add_to_tree_t));

  char* tempfile = get_tempfile();
  write_to_file_result_t write_result =
      write_to_file_helper(tree, STRPLUSLEN(tempfile), true);
  ASSERT(write_result == WRITE_TO_FILE_OK);

  read_from_file_result_t read_result = read_from_file(STRPLUSLEN(tempfile));

  ASSERT(read_result.code == READ_FROM_FILE_OK);
  diff_result_t diff_result =
      diff_trees(tree, read_result.tree, false, never_called_callback, NULL);

  ASSERT(diff_result == DIFF_OK);
}

int main(int argc, char* argv[]) {
  save_load_empty_tree();
  save_load_small_tree();

  return 0;
}
