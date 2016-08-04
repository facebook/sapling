// Copyright 2016-present Facebook. All Rights Reserved.
//
// cdatapack.c: Datapack implementation in C.
//
// no-check-code

#include <errno.h>
#include <fcntl.h>
#include <memory.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <sys/mman.h>

#if defined(__linux__)
#include <endian.h>
#define ntohll be64toh
#endif /* #if defined(__linux__) */

#include <lz4.h>

#include "cdatapack.h"
#include "buffer.h"

/**
 * This is an exact representation of an index entry on disk.  Do not consume
 * the fields directly, as they may need processing.
 *
 * NOTE: this uses gcc's __attribute__((packed)) syntax to indicate a packed
 * data structure, which obviously has potential portability issues.
 */
typedef struct _disk_index_entry_t {
  uint8_t node[NODE_SZ];

  // offset of the next element in the delta chain in the index file
  index_offset_t deltabase_index_offset;

  // offset and size of this current element in the delta chain in the data
  // file.
  data_offset_t data_offset;
  data_offset_t data_sz;
} __attribute__((packed)) disk_index_entry_t;

/**
 * This represents offsets into the index indicating the range of a fanout
 * bucket.  This is calculated upon opening the file.
 */
typedef struct _fanout_table_entry_t {
  index_offset_t start_index;
  index_offset_t end_index;
} fanout_table_entry_t;

/**
 * This is a chain of index entries.
 */
typedef struct _pack_chain_t {
  pack_index_entry_t *pack_chain_links;
  size_t links_idx;
  size_t links_sz;
} pack_chain_t;

/**
 * This is an exact representation of an index file's header on disk.  Do not
 * consume the fields directly, as they may need processing.
 *
 * NOTE: this uses gcc's __attribute__((packed)) syntax to indicate a packed
 * data structure, which obviously has potential portability issues.
 */
typedef struct _disk_index_header_t {
#define VERSION 0
  uint8_t version;

#define LARGE_FANOUT 0x80
  uint8_t config;
} __attribute__((packed)) disk_index_header_t;

static void unpack_disk_deltachunk(
    const disk_index_entry_t *disk_deltachunk,
    pack_index_entry_t *packindex) {
  packindex->node = disk_deltachunk->node;
  packindex->data_offset = ntoh_data_offset(
      disk_deltachunk->data_offset);
  packindex->data_sz = ntoh_data_offset(
      disk_deltachunk->data_sz);
  packindex->deltabase_index_offset = ntoh_index_offset(
      disk_deltachunk->deltabase_index_offset);
}

/**
 * Finds a node using the index, and fills out the packindex pointer.
 * Returns true iff the node is found.
 */
bool find(
    const datapack_handle_t *handle,
    const uint8_t node[NODE_SZ],
    pack_index_entry_t *packindex) {
  uint16_t fanout_idx;
  if (handle->large_fanout) {
    uint16_t* fanout_idx_ptr = (uint16_t*) &node[0];
    fanout_idx = ntohs(*fanout_idx_ptr);
  } else {
    fanout_idx = node[0];
  }

  index_offset_t start = handle->fanout_table[fanout_idx].start_index /
                         sizeof(disk_index_entry_t);
  index_offset_t end = handle->fanout_table[fanout_idx].end_index /
                       sizeof(disk_index_entry_t);

  // indices are INCLUSIVE, so the search is <=
  while (start <= end) {
    index_offset_t middle = start + ((end - start) / 2);

    // peek at the hash at that location.
    int cmp = memcmp(node, handle->index_table[middle].node, NODE_SZ);
    if (cmp < 0) {
      if (middle == 0) {
        // don't wrap around.
        break;
      }
      end = middle - 1;
    } else if (cmp > 0) {
      start = middle + 1;
    } else {
      // exact match!
      unpack_disk_deltachunk(&handle->index_table[middle], packindex);

      return true;
    }
  }

  // nope, no good.
  return false;
}

datapack_handle_t *open_datapack(
    char *indexfp, size_t indexfp_sz,
    char *datafp, size_t datafp_sz) {
  datapack_handle_t *handle = NULL;
  char *buffer = NULL;

  handle = malloc(sizeof(datapack_handle_t));
  if (handle == NULL) {
    // TODO: at some future point in time, it might be nice to add some
    // better error reporting like we have in cfastmanifest.
    goto error_cleanup;
  }

  // can't just use memset because MAP_FAILED is the error result code, not
  // NULL.
  memset(handle, 0, sizeof(datapack_handle_t));
  handle->data_mmap = MAP_FAILED;
  handle->index_mmap = MAP_FAILED;

  buffer = malloc(1 + (indexfp_sz > datafp_sz ? indexfp_sz : datafp_sz));
  if (buffer == NULL) {
    goto error_cleanup;
  }

  memcpy(buffer, indexfp, indexfp_sz);
  buffer[indexfp_sz] = '\0';
  handle->indexfd = open(buffer, O_RDONLY);
  if (handle->indexfd < 0) {
    goto error_cleanup;
  }

  handle->index_file_sz = lseek(handle->indexfd, 0, SEEK_END);
  lseek(handle->indexfd, 0, SEEK_SET);

  memcpy(buffer, datafp, datafp_sz);
  buffer[datafp_sz] = '\0';
  handle->datafd = open(buffer, O_RDONLY);
  if (handle->datafd < 0) {
    goto error_cleanup;
  }

  handle->data_file_sz = lseek(handle->datafd, 0, SEEK_END);
  lseek(handle->datafd, 0, SEEK_SET);

  handle->index_mmap = mmap(NULL, (size_t) handle->index_file_sz, PROT_READ,
      MAP_FILE | MAP_PRIVATE, handle->indexfd, (off_t) 0);
  if (handle->index_mmap == MAP_FAILED) {
    int er = errno;
    (void) er;
    goto error_cleanup;
  }

  handle->data_mmap = mmap(NULL, (size_t) handle->data_file_sz, PROT_READ,
      MAP_FILE | MAP_PRIVATE, handle->datafd, (off_t) 0);
  if (handle->data_mmap == MAP_FAILED) {
    goto error_cleanup;
  }

  // read the headers and ensure that the file length is at least somewhat
  // sane.
  if (handle->index_file_sz < sizeof(disk_index_header_t)) {
    goto error_cleanup;
  }
  const disk_index_header_t *header = (const disk_index_header_t *)
      handle->index_mmap;
  if (header->version != VERSION) {
    goto error_cleanup;
  }
  handle->large_fanout = ((header->config & LARGE_FANOUT) != 0);
  int fanout_count = 1 << (handle->large_fanout ? 16 : 8);
  handle->fanout_table = (fanout_table_entry_t *) calloc(
      fanout_count, sizeof(fanout_table_entry_t));
  if (handle->fanout_table == NULL) {
    goto error_cleanup;
  }
  handle->index_table = (disk_index_entry_t *)
      (((const char *) handle->index_mmap) +
       sizeof(disk_index_header_t) +
       (sizeof(index_offset_t) * fanout_count));
  disk_index_entry_t *index_end = (disk_index_entry_t *)
      (((const char *) handle->index_mmap) + handle->index_file_sz);
  if (handle->index_table > index_end) {
    // ensure the file is at least big enough to include the fanout table.
    goto error_cleanup;
  }

  // build a clean and easy table to bisect.
  index_offset_t *index = (index_offset_t *)
      (((const char *) handle->index_mmap) +
       sizeof(disk_index_header_t));
  index_offset_t prev_index_offset = 0;
  int last_fanout_increment = 0;

  for (int ix = 0; ix < fanout_count; ix++) {
    index_offset_t index_offset = ntoh_index_offset(index[ix]);
    if (index_offset != prev_index_offset) {
      // backfill the start & end offsets
      for (int jx = last_fanout_increment; jx < ix; jx ++) {
        index_offset_t written_index;

        if (prev_index_offset == 0) {
          // this is an unfortunate case because we cannot tell the
          // difference between an empty fanout entry and the fanout
          // entry for the first index entry.  they will both show '0'.
          // therefore, if prev_index_offset is 0, we have to bisect from 0.
          written_index = 0;
        } else {
          written_index = index_offset;
        }

        // fill the "start" except for the last time we changed the index
        // offset.
        if (jx != last_fanout_increment) {
          handle->fanout_table[jx].start_index = written_index;
        }
        handle->fanout_table[jx].end_index = index_offset;
      }

      handle->fanout_table[ix].start_index = index_offset;
      last_fanout_increment = ix;

      prev_index_offset = index_offset;
    }
  }

  // we may need to backfill the remaining offsets.
  index_offset_t last_offset = (index_offset_t)
      ((index_end - handle->index_table - 1) * sizeof(disk_index_entry_t));
  for (int jx = last_fanout_increment; jx < fanout_count; jx ++) {
    // fill the "start" except for the last time we changed the index
    // offset.
    if (jx != last_fanout_increment) {
      handle->fanout_table[jx].start_index = last_offset;
    }
    handle->fanout_table[jx].end_index = last_offset;
  }

  goto success_cleanup;

error_cleanup:

  if (handle->index_mmap != MAP_FAILED) {
    munmap(handle->index_mmap, handle->index_file_sz);
  }

  if (handle->data_mmap != MAP_FAILED) {
    munmap(handle->data_mmap, handle->data_file_sz);
  }

  if (handle && handle->indexfd != 0) {
    close(handle->indexfd);
  }

  if (handle && handle->datafd != 0) {
    close(handle->datafd);
  }

  if (handle != NULL) {
    free(handle->fanout_table);
  }

  free(handle);

  handle = NULL;

success_cleanup:

  free(buffer);

  return handle;
}

void close_datapack(datapack_handle_t *handle) {
  munmap(handle->index_mmap, handle->index_file_sz);
  munmap(handle->data_mmap, handle->data_file_sz);
  close(handle->indexfd);
  close(handle->datafd);
  free(handle->fanout_table);
  free(handle);
}

#define DEFAULT_PACK_CHAIN_CAPACITY         64
#define PACK_CHAIN_GROWTH_FACTOR            2.0
#define PACK_CHAIN_MINIMUM_GROWTH           1024
#define PACK_CHAIN_MAXIMUM_GROWTH           65536

#define PACK_CHAIN_EXPAND_TO_FIT(buffer, buffer_idx, buffer_sz)               \
  expand_to_fit(buffer, buffer_idx, buffer_sz,                                \
      1, sizeof(pack_index_entry_t),                                          \
      PACK_CHAIN_GROWTH_FACTOR,                                               \
      PACK_CHAIN_MINIMUM_GROWTH,                                              \
      PACK_CHAIN_MAXIMUM_GROWTH)

static pack_chain_t *build_pack_chain(
    const datapack_handle_t *handle,
    const uint8_t node[NODE_SZ]) {
  pack_chain_t *result = malloc(sizeof(pack_chain_t));
  result->links_idx = 0;
  result->links_sz = DEFAULT_PACK_CHAIN_CAPACITY;
  result->pack_chain_links = malloc(
      result->links_sz * sizeof(pack_index_entry_t));
  // TODO: error handling.

  pack_index_entry_t entry;

  // find the first entry.
  if (find(handle, node, &entry) == false) {
    return NULL;
  }

  PACK_CHAIN_EXPAND_TO_FIT(
      (void **)&result->pack_chain_links,
      result->links_idx,
      &result->links_sz);
  // TODO: yeah, this desperately needs some error handling.

  result->pack_chain_links[result->links_idx++] = entry;

  while (entry.deltabase_index_offset != FULLTEXTINDEXMARK &&
         entry.deltabase_index_offset != NOBASEINDEXMARK) {
    unpack_disk_deltachunk(
        &handle->index_table[entry.deltabase_index_offset], &entry);

    PACK_CHAIN_EXPAND_TO_FIT(
        (void **)&result->pack_chain_links,
        result->links_idx,
        &result->links_sz);
    // TODO: yeah, this desperately needs some error handling.

    result->pack_chain_links[result->links_idx++] = entry;
  }

  return result;
}

static inline uint32_t load_le32(const uint8_t *d) {
  return d[0] | (d[1] << 8) | (d[2] << 16) | (d[3] << 24);
}

const uint8_t *getdeltachainlink(
    const uint8_t *ptr, delta_chain_link_t *link) {
  link->filename_sz = ntohs(*((uint16_t *) ptr));
  ptr += sizeof(uint16_t);

  link->filename = (const char *) ptr;
  ptr += link->filename_sz;

  link->node = ptr;
  ptr += NODE_SZ;

  link->deltabase_node = ptr;
  ptr += NODE_SZ;

  data_offset_t compressed_sz = ntohll(*((uint64_t *) ptr)) - sizeof(uint32_t);
  ptr += sizeof(data_offset_t);

  link->delta_sz = load_le32(ptr);
  ptr += sizeof(uint32_t);

  uint8_t *decompress_output = malloc(link->delta_sz);
  // TODO: error handling!

  uint32_t outbytes = LZ4_decompress_fast(
      (const char *) ptr,
      (char *) decompress_output,
      (int32_t) link->delta_sz);
  // TODO: error handling
  (void) outbytes;
  link->delta = decompress_output;

  ptr += compressed_sz;

  return ptr;
}

delta_chain_t *getdeltachain(
    const datapack_handle_t *handle,
    const uint8_t node[NODE_SZ]) {
  pack_chain_t *pack_chain = build_pack_chain(handle, node);

  if (pack_chain == NULL) {
    return NULL;
  }

  delta_chain_t *delta_chain = malloc(sizeof(delta_chain_t));
  delta_chain->links_count = pack_chain->links_idx;
  delta_chain->delta_chain_links = malloc(
      delta_chain->links_count * sizeof(delta_chain_link_t));
  // TODO: error handling


  for (int ix = 0; ix < pack_chain->links_idx; ix ++) {
    const uint8_t *ptr = handle->data_mmap;
    ptr += pack_chain->pack_chain_links[ix].data_offset;
    const uint8_t *end = ptr +
        pack_chain->pack_chain_links[ix].data_sz;

    delta_chain_link_t *link = &delta_chain->delta_chain_links[ix];

    ptr = getdeltachainlink(ptr, link);

    if (ptr > end) {
      abort();
    }
  }

  // free pack chain.
  if (pack_chain != NULL) {
    free(pack_chain->pack_chain_links);
    free(pack_chain);
  }

  return delta_chain;
}

void freedeltachain(delta_chain_t *chain) {
  free(chain->delta_chain_links);
  free(chain);
}
