// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_disk.c: methods to persist to and restore from disk.
//
// no-check-code

#include <errno.h>
#include <memory.h>
#include <stdio.h>
#include <stdlib.h>

#include "checksum.h"
#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/portability/inet.h"
#include "node.h"
#include "tree_arena.h"

// FILE FORMAT
//
// UNLESS OTHERWISE NOTED, NUMERICAL FIELDS ARE IN HOST WORD ORDER.
//
// offset     length    description
// 0          9         fasttree\0
// 9          1         byte order (1 = little endian, 2 = big endian)
// 10         1         address size
// 11         1         <unused>
// 12         4         file format version
// 16         8         file length in bytes
// 24         4         header length in bytes
// 28         4         num_leaf_nodes (see tree.h)
// 32         size_t    consumed_memory (see tree.h)
// 32+size_t  ptrdiff_t offset to find the true root
// 32+size_t+ *         tree data
//  ptrdiff_t
//
// the arena must be allocated with at least <file length> - <header length>
// bytes.

typedef struct _v0_header_t {
#define MAGIC "fasttree"
  char magic[sizeof(MAGIC)];

#define BYTE_ORDER_LITTLE_ENDIAN 1
#define BYTE_ORDER_BIG_ENDIAN 2

  uint8_t byte_order;
  uint8_t address_size;

#define FILE_VERSION 0
  uint32_t file_version;

  uint64_t file_sz;
  uint32_t header_sz;
  uint32_t num_leaf_nodes;

  size_t consumed_memory;
  ptrdiff_t root_offset;
} v0_header_t;

#define LITTLE_ENDIAN_TEST_VALUE 0x01020304
/**
 * Returns true iff the host is little endian.
 */
static inline bool little_endian(void) {
  int foo = LITTLE_ENDIAN_TEST_VALUE;
  if (ntohl(foo) == LITTLE_ENDIAN_TEST_VALUE) {
    return false;
  }

  return true;
}

/**
 * Returns the size, in bytes, of the host pointer.
 */
static inline uint8_t host_pointer_size(void) {
  return sizeof(void*);
}

static inline size_t read_noint(FILE* fh, void* _buf, size_t nbytes) {
  char* buf = (char*)_buf;
  size_t read_so_far = 0;

  while (read_so_far < nbytes) {
    size_t read_this_iteration =
        fread(buf + read_so_far, 1, nbytes - read_so_far, fh);
    read_so_far += read_this_iteration;

    if (read_so_far != nbytes) {
      if (feof(fh) || ferror(fh) != EINTR) {
        // we reached the end of the file, or we received an error that's not
        // EINTR.
        break;
      }
    }
  }

  return read_so_far;
}

#define CHECKED_READ(fh, ptr, nbytes)                \
  {                                                  \
    size_t __nbytes = nbytes;                        \
    if (read_noint(fh, ptr, __nbytes) != __nbytes) { \
      result.code = READ_FROM_FILE_WTF;              \
      goto cleanup;                                  \
    }                                                \
  }
read_from_file_result_t read_from_file(char* fname, size_t fname_sz) {
  char* fname_dst = malloc(fname_sz + 1);
  if (fname_dst == NULL) {
    return COMPOUND_LITERAL(read_from_file_result_t){
        READ_FROM_FILE_OOM, 0, NULL};
  }
  memcpy(fname_dst, fname, fname_sz);
  fname_dst[fname_sz] = '\x00';

  read_from_file_result_t result = {0};

  FILE* fh = fopen(fname_dst, "rb");
  if (fh == NULL) {
    result.err = errno;
    result.code = READ_FROM_FILE_NOT_READABLE;
    goto cleanup;
  }

  v0_header_t header;

  CHECKED_READ(fh, &header, sizeof(v0_header_t));
  if (memcmp(header.magic, MAGIC, sizeof(MAGIC)) != 0) {
    result.code = READ_FROM_FILE_WTF;
    goto cleanup;
  }

  // endianness
  if (little_endian()) {
    if (header.byte_order != BYTE_ORDER_LITTLE_ENDIAN) {
      result.code = READ_FROM_FILE_NOT_USABLE;
      goto cleanup;
    }
  } else {
    if (header.byte_order != BYTE_ORDER_BIG_ENDIAN) {
      result.code = READ_FROM_FILE_NOT_USABLE;
      goto cleanup;
    }
  }

  // host pointer size
  if (header.address_size != host_pointer_size()) {
    result.code = READ_FROM_FILE_NOT_USABLE;
    goto cleanup;
  }

  // file version.
  if (header.file_version != FILE_VERSION) {
    result.code = READ_FROM_FILE_NOT_USABLE;
    goto cleanup;
  }

  // at this point, the file offset should == header_sz
  if (ftell(fh) != header.header_sz) {
    result.code = READ_FROM_FILE_WTF;
    goto cleanup;
  }

  if (header.file_sz - header.header_sz > SIZE_MAX) {
    result.code = READ_FROM_FILE_WTF;
    goto cleanup;
  }
  size_t arena_sz = (size_t)(header.file_sz - header.header_sz);

  // allocate the tree
  result.tree = alloc_tree_with_arena(arena_sz);
  if (result.tree == NULL) {
    result.code = READ_FROM_FILE_OOM;
    goto cleanup;
  }

  // read the tree
  CHECKED_READ(fh, result.tree->arena, arena_sz);

  // find the real root and parent it to shadow root.
  node_t* real_root =
      (node_t*)(((intptr_t)result.tree->arena) + header.root_offset);
  add_child(result.tree->shadow_root, real_root);

  // write all the stats into place.
  result.tree->arena_sz = arena_sz;
  result.tree->arena_free_start =
      (void*)((char*)result.tree->arena + result.tree->arena_sz);
  result.tree->compacted = true;
  result.tree->consumed_memory = header.consumed_memory;
  result.tree->num_leaf_nodes = header.num_leaf_nodes;

  result.code = READ_FROM_FILE_OK;

cleanup:
  if (result.code != READ_FROM_FILE_OK && result.tree != NULL) {
    destroy_tree(result.tree);
  }
  if (fh != NULL) {
    fclose(fh);
  }
  free(fname_dst);
  return result;
}

static inline size_t write_noint(FILE* fh, void* _buf, size_t nbytes) {
  char* buf = (char*)_buf;
  size_t written_so_far = 0;

  while (written_so_far < nbytes) {
    size_t written_this_iteration =
        fwrite(buf + written_so_far, 1, nbytes - written_so_far, fh);
    written_so_far += written_this_iteration;

    if (written_so_far != nbytes) {
      // came up short.  it has to be some sort of error.  if it's not EINTR,
      // we give up.
      if (ferror(fh) != EINTR) {
        break;
      }
    }
  }

  return written_so_far;
}

#define CHECKED_WRITE(fh, ptr, nbytes)                \
  {                                                   \
    size_t __nbytes = (size_t)nbytes;                 \
    if (write_noint(fh, ptr, __nbytes) != __nbytes) { \
      result = WRITE_TO_FILE_WTF;                     \
      goto cleanup;                                   \
    }                                                 \
  }
static write_to_file_result_t
write_compact_tree_to_file(tree_t* tree, char* fname, size_t fname_sz) {
  if (tree->compacted == false) {
    return WRITE_TO_FILE_WTF;
  }

  char* fname_dst = malloc(fname_sz + 1);
  if (fname_dst == NULL) {
    return WRITE_TO_FILE_OOM;
  }
  memcpy(fname_dst, fname, fname_sz);
  fname_dst[fname_sz] = '\x00';

  FILE* fh = fopen(fname_dst, "wb");
  write_to_file_result_t result;
  if (fh == NULL) {
    result = WRITE_TO_FILE_WTF;
    goto cleanup;
  }

  v0_header_t header;
  memset(&header, 0, sizeof(header)); // keeping valgrind happy.
  header.header_sz = sizeof(v0_header_t);
  size_t used_size = (char*)tree->arena_free_start - (char*)tree->arena;
  header.file_sz = header.header_sz + used_size;

  memcpy(header.magic, MAGIC, sizeof(MAGIC));
  header.byte_order =
      little_endian() ? BYTE_ORDER_LITTLE_ENDIAN : BYTE_ORDER_BIG_ENDIAN;
  header.address_size = host_pointer_size();

  header.file_version = FILE_VERSION;

  header.num_leaf_nodes = tree->num_leaf_nodes;
  header.consumed_memory = tree->consumed_memory;

  intptr_t real_root_ptr = (intptr_t)get_child_by_index(tree->shadow_root, 0);
  ptrdiff_t ptrdiff = real_root_ptr - ((intptr_t)tree->arena);

  header.root_offset = ptrdiff;

  CHECKED_WRITE(fh, &header, sizeof(v0_header_t));

  CHECKED_WRITE(fh, tree->arena, used_size);

  result = WRITE_TO_FILE_OK;

cleanup:
  if (fh != NULL) {
    fclose(fh);
  }
  free(fname_dst);
  return result;
}

/**
 * This is a highly implementation dependent mechanism to initialize the
 * padding bytes.  Otherwise valgrind will freak out over the uninitialized
 * padding bytes getting written to disk.
 */
static void initialize_unused_bytes(node_t* node) {
  // initializes any unused checksum bytes.
  memset(
      &node->checksum[node->checksum_sz],
      0,
      CHECKSUM_BYTES - node->checksum_sz);

  // flags for root nodes are not typically initialized
  if (node->type == TYPE_ROOT) {
    node->flags = 0;
  }

  // initialize the remaining bits in the bitfield
  node->unused = 0;

  void* name_start = &node->name;
  intptr_t start_address = (intptr_t)name_start;
  start_address += node->name_sz;
  intptr_t end_address = start_address + sizeof(ptrdiff_t) - 1;
  end_address &= ~((intptr_t)(sizeof(ptrdiff_t) - 1));

  // initializes the padding between the end of the name and the start of the
  // child pointers.
  memset((void*)start_address, 0, end_address - start_address);

  // find all the children and make sure their checksums are up-to-date.
  for (child_num_t ix = 0; ix < node->num_children; ix++) {
    node_t* child = get_child_by_index(node, ix);

    initialize_unused_bytes(child);
  }
}

/**
 * Writes a tree to a file.
 */
write_to_file_result_t write_to_file_helper(
    tree_t* tree,
    char* fname,
    size_t fname_sz,
    bool initialize_padding) {
  // update the checksums first.
  update_checksums(tree);

  if (tree->compacted) {
    if (initialize_padding) {
      initialize_unused_bytes(get_child_by_index(tree->shadow_root, 0));
    }
    return write_compact_tree_to_file(tree, fname, fname_sz);
  }

  // Note that there is a probably a significant opportunity for improving the
  // performance by using the bottom-up tree construction strategy used in
  // convert_from_flat(..) to write a non-compact tree straight to disk.  This
  // is the naive implementation that simply does a tree_copy to construct a
  // compact tree.
  tree_t* compact_copy = copy_tree(tree);
  if (compact_copy == NULL) {
    return WRITE_TO_FILE_OOM;
  }

  if (initialize_padding) {
    initialize_unused_bytes(get_child_by_index(compact_copy->shadow_root, 0));
  }
  write_to_file_result_t result =
      write_compact_tree_to_file(compact_copy, fname, fname_sz);

  destroy_tree(compact_copy);

  return result;
}

write_to_file_result_t
write_to_file(tree_t* tree, char* fname, size_t fname_sz) {
  return write_to_file_helper(tree, fname, fname_sz, false);
}
