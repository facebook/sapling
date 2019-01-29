//
// tree_dump: Load a tree from disk.  Then print all the node hashes along
// with the length of the name and the number of children.
//
// no-check-code

#include <stdlib.h>

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/convert.h"
#include "tests.h"

static char buffer[SHA1_BYTES * 2];

void print_subtree(node_t* node) {
  hexlify(node->checksum, node->checksum_sz, buffer);

  printf(
      "%.*s\t%d\t%d\n",
      node->checksum_sz * 2,
      buffer,
      node->name_sz,
      node->num_children);

  for (uint32_t ix = 0; ix < node->num_children; ix++) {
    print_subtree(get_child_by_index(node, ix));
  }
}

int main(int argc, char* argv[]) {
  if (argc < 2) {
    fprintf(stderr, "Usage: %s <tree-save-file>\n", argv[0]);
    exit(1);
  }

  read_from_file_result_t read_from_file_result =
      read_from_file(argv[1], strlen(argv[1]));
  if (read_from_file_result.code != READ_FROM_FILE_OK) {
    fprintf(stderr, "Unable to read tree file %s\n", argv[1]);
    exit(1);
  }

  tree_t* tree = read_from_file_result.tree;
  node_t* shadow_root = tree->shadow_root;
  node_t* real_root = get_child_by_index(shadow_root, 0);

  print_subtree(real_root);

  return 0;
}
