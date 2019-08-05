// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// cdatapack.c: Datapack implementation in C.
// no-check-code

// be64toh is only available if _DEFAULT_SOURCE is defined for glibc >= 2.19,
// or _BSD_SOURCE is defined for glibc < 2.19. These have to be defined before
// #include <features.h>. Macros testing glibc version are defined by
// features.h, so they cannot be used here.
#ifndef _DEFAULT_SOURCE
#define _DEFAULT_SOURCE
#endif /* ndef _DEFAULT_SOURCE */
#ifndef _BSD_SOURCE
#define _BSD_SOURCE
#endif /* ndef _BSD_SOURCE */

#include "lib/cdatapack/cdatapack.h"

#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <memory.h>

#define ntoh_data_offset ntohll

#if defined(__linux__)
#include <endian.h>
#define ntohll be64toh

// NOTE: we actually want MADV_FREE, because we only want to mark the page as
// eligible for immediate reuse while retaining the data.  however, the exciting
// centos6 machines we have too old of a version of linux for MADV_FREE.  LOL.
#define MADVISE_FREE_CODE MADV_DONTNEED
#define PAGE_SIZE 4096
#endif /* #if defined(__linux__) */

#if defined(__APPLE__)
#define MADVISE_FREE_CODE MADV_FREE
#endif /* #if defined(__APPLE__) */

#include <lz4.h>

#include "lib/clib/buffer.h"
#include "lib/clib/portability/inet.h"
#include "lib/clib/portability/mman.h"
#include "lib/clib/portability/unistd.h"

#define MAX_PAGED_IN_DATAPACK (1024 * 1024 * 1024)
#define VERSION 0
#define LARGE_FANOUT 0x80

/**
 * This is an exact representation of an index entry on disk.  Do not consume
 * the fields directly, as they may need processing.
 */
PACKEDSTRUCT(typedef struct _disk_index_entry_t {
  uint8_t node[NODE_SZ];

  // offset of the next element in the delta chain in the index file
  index_offset_t deltabase_index_offset;

  // offset and size of this current element in the delta chain in the data
  // file.
  data_offset_t data_offset;
  data_offset_t data_sz;
})
disk_index_entry_t;

/**
 * This represents offsets into the index indicating the range of a fanout
 * bucket.  This is calculated upon opening the file.
 */
typedef struct _fanout_table_entry_t {
  index_offset_t start_index;
  index_offset_t end_index;
} fanout_table_entry_t;

typedef enum {
  PACK_CHAIN_OK,
  PACK_CHAIN_NOT_FOUND,
  PACK_CHAIN_OOM,
} pack_chain_code_t;

/**
 * This is a chain of index entries.
 */
typedef struct _pack_chain_t {
  pack_chain_code_t code;
  pack_index_entry_t* pack_chain_links;
  size_t links_idx;
  size_t links_sz;
} pack_chain_t;

/**
 * This is an exact representation of an index file's header on disk.  Do not
 * consume the fields directly, as they may need processing.
 */
PACKEDSTRUCT(typedef struct _disk_index_header_t {
  uint8_t version;
  uint8_t config;
})
disk_index_header_t;

static void unpack_disk_deltachunk(
    const disk_index_entry_t* disk_deltachunk,
    pack_index_entry_t* packindex) {
  packindex->node = disk_deltachunk->node;
  packindex->data_offset = ntoh_data_offset(disk_deltachunk->data_offset);
  packindex->data_sz = ntoh_data_offset(disk_deltachunk->data_sz);
  packindex->deltabase_index_offset =
      ntoh_index_offset(disk_deltachunk->deltabase_index_offset);
}

static inline uint16_t get_fanout_index(
    const datapack_handle_t* handle,
    const uint8_t node[NODE_SZ]) {
  if (handle->large_fanout) {
    return (node[0] << 8) | node[1];
  } else {
    return node[0];
  }
}

/**
 * Finds a node using the index, and fills out the packindex pointer.
 * Returns true iff the node is found.
 */
bool find(
    const datapack_handle_t* handle,
    const uint8_t node[NODE_SZ],
    pack_index_entry_t* packindex) {
  uint16_t fanout_idx = get_fanout_index(handle, node);
  index_offset_t start = handle->fanout_table[fanout_idx].start_index;
  index_offset_t end = handle->fanout_table[fanout_idx].end_index;

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

static void backfill_fanout_entries(
    datapack_handle_t* handle,
    size_t fanout_idx_start,
    size_t fanout_idx_end,
    size_t last_offset,
    size_t new_offset) {
  if (last_offset == 0) {
    assert(fanout_idx_start == 0);
    // bucket0_idx is the only prior bucket that contains any nodes.
    uint16_t bucket0_idx =
        get_fanout_index(handle, handle->index_table[0].node);
    for (int ix = fanout_idx_start; ix < fanout_idx_end; ++ix) {
      if (ix == bucket0_idx) {
        handle->fanout_table[ix].start_index = 0;
        handle->fanout_table[ix].end_index = new_offset - 1;
      } else {
        handle->fanout_table[ix].start_index = 1;
        handle->fanout_table[ix].end_index = 0;
      }
    }
  } else {
    // The bucket at fanout_idx_start needs its end_index updated to the
    // current index.
    handle->fanout_table[fanout_idx_start].end_index = new_offset - 1;

    // All buckets from fanout_idx_start to the fanout_idx_end are empty and
    // don't have any nodes.
    for (int ix = fanout_idx_start + 1; ix < fanout_idx_end; ++ix) {
      handle->fanout_table[ix].start_index = 1;
      handle->fanout_table[ix].end_index = 0;
    }
  }
}

datapack_handle_t* open_datapack(
    const char* indexfp,
    size_t indexfp_sz,
    const char* datafp,
    size_t datafp_sz) {
  int indexfd = -1;
  int datafd = -1;
  datapack_handle_t* handle = NULL;
  char* buffer = NULL;

  handle = malloc(sizeof(datapack_handle_t));
  if (handle == NULL) {
    return NULL;
  }

  // can't just use memset because MAP_FAILED is the error result code, not
  // NULL.
  memset(handle, 0, sizeof(datapack_handle_t));
  handle->data_mmap = MAP_FAILED;
  handle->index_mmap = MAP_FAILED;

  buffer = malloc(1 + (indexfp_sz > datafp_sz ? indexfp_sz : datafp_sz));
  if (buffer == NULL) {
    handle->status = DATAPACK_HANDLE_OOM;
    goto error_cleanup;
  }

  memcpy(buffer, indexfp, indexfp_sz);
  buffer[indexfp_sz] = '\0';
  indexfd = open(buffer, O_RDONLY);
  if (indexfd < 0) {
    handle->status = DATAPACK_HANDLE_IO_ERROR;
    goto error_cleanup;
  }

  handle->index_file_sz = lseek(indexfd, 0, SEEK_END);
  lseek(indexfd, 0, SEEK_SET);

  memcpy(buffer, datafp, datafp_sz);
  buffer[datafp_sz] = '\0';
  datafd = open(buffer, O_RDONLY);
  if (datafd < 0) {
    handle->status = DATAPACK_HANDLE_IO_ERROR;
    goto error_cleanup;
  }

  handle->data_file_sz = lseek(datafd, 0, SEEK_END);
  lseek(datafd, 0, SEEK_SET);

  handle->index_mmap = mmap(
      NULL,
      (size_t)handle->index_file_sz,
      PROT_READ,
      MAP_PRIVATE,
      indexfd,
      (off_t)0);
  if (handle->index_mmap == MAP_FAILED) {
    handle->status = DATAPACK_HANDLE_MMAP_ERROR;
    goto error_cleanup;
  }
  close(indexfd);
  indexfd = -1;

  handle->data_mmap = mmap(
      NULL,
      (size_t)handle->data_file_sz,
      PROT_READ,
      MAP_PRIVATE,
      datafd,
      (off_t)0);
  if (handle->data_mmap == MAP_FAILED) {
    handle->status = DATAPACK_HANDLE_MMAP_ERROR;
    goto error_cleanup;
  }
  close(datafd);
  datafd = -1;

  // read the headers and ensure that the file length is at least somewhat
  // sane.
  if (handle->index_file_sz < sizeof(disk_index_header_t)) {
    handle->status = DATAPACK_HANDLE_CORRUPT;
    goto error_cleanup;
  }
  const disk_index_header_t* header =
      (const disk_index_header_t*)handle->index_mmap;
  if (header->version != ((const char*)handle->data_mmap)[0]) {
    // data and index disagree on version
    handle->status = DATAPACK_HANDLE_VERSION_MISMATCH;
    goto error_cleanup;
  }

  if (header->version > 1) {
    // unsupported version
    handle->status = DATAPACK_HANDLE_VERSION_MISMATCH;
    goto error_cleanup;
  }
  handle->version = header->version;

  handle->large_fanout = ((header->config & LARGE_FANOUT) != 0);
  int fanout_count = 1 << (handle->large_fanout ? 16 : 8);
  handle->fanout_table =
      (fanout_table_entry_t*)calloc(fanout_count, sizeof(fanout_table_entry_t));
  if (handle->fanout_table == NULL) {
    handle->status = DATAPACK_HANDLE_OOM;
    goto error_cleanup;
  }
  size_t index_offset = 0;
  if (handle->version == 1) {
    index_offset = 8;
  }
  handle->index_table =
      (disk_index_entry_t*)(((const char*)handle->index_mmap) + sizeof(disk_index_header_t) + index_offset + (sizeof(index_offset_t) * fanout_count));
  disk_index_entry_t* index_end =
      (disk_index_entry_t*)(((const char*)handle->index_mmap) + handle->index_file_sz);
  if (handle->index_table + 1 > index_end) {
    // ensure the file is at least big enough to include the fanout table
    // plus one entry.
    handle->status = DATAPACK_HANDLE_CORRUPT;
    goto error_cleanup;
  }

  // Convert the index data in the file into a fanout table allowing find()
  // to easily lookup the area where it needs to perform bisection.
  //
  // The index data is interpreted as follows:
  // - If the index is 0 we can't really tell if this bucket actually has data
  //   or not.  It might be empty, or it might have data from location 0 up to
  //   the next entry that has a non-zero index.
  //
  //   In order to figure out which bucket actually has data at offset 0
  //   we look at handle->index_table[0].node
  //
  // - If the index is non-zero but the same as the previous node, then no nodes
  //   are present starting with this fanout value.
  //
  // - If the index is different from the previous node, then this bucket has
  //   data starting from this node up to the next entry that has a non-zero
  //   bucket.
  index_offset_t* index =
      (index_offset_t*)(((const char*)handle->index_mmap) + sizeof(disk_index_header_t));
  // We don't know the end location yet for entries starting at need_end_idx
  size_t need_end_idx = 0;
  size_t last_idx = 0;

  for (int ix = 0; ix < fanout_count; ix++) {
    index_offset_t index_offset =
        ntoh_index_offset(index[ix]) / sizeof(disk_index_entry_t);
    if (index_offset == last_idx) {
      continue;
    }
    if (index_offset < last_idx) {
      handle->status = DATAPACK_HANDLE_CORRUPT;
      goto error_cleanup;
    }
    backfill_fanout_entries(handle, need_end_idx, ix, last_idx, index_offset);
    handle->fanout_table[ix].start_index = index_offset;
    need_end_idx = ix;
    last_idx = index_offset;
  }

  // Finish filling out data for the buckets from need_end_idx to the end
  index_offset_t end_offset = (index_offset_t)(index_end - handle->index_table);
  backfill_fanout_entries(
      handle, need_end_idx, fanout_count, last_idx, end_offset);

  handle->status = DATAPACK_HANDLE_OK;

  goto success_cleanup;

error_cleanup:

  if (handle->index_mmap != MAP_FAILED) {
    munmap(handle->index_mmap, (size_t)handle->index_file_sz);
  }

  if (handle->data_mmap != MAP_FAILED) {
    munmap(handle->data_mmap, (size_t)handle->data_file_sz);
  }

  if (indexfd != -1) {
    close(indexfd);
  }

  if (datafd != -1) {
    close(datafd);
  }

  free(handle->fanout_table);
  free(handle);
  handle = NULL;

success_cleanup:

  free(buffer);

  return handle;
}

void close_datapack(datapack_handle_t* handle) {
  munmap(handle->index_mmap, (size_t)handle->index_file_sz);
  munmap(handle->data_mmap, (size_t)handle->data_file_sz);
  free(handle->fanout_table);
  free(handle);
}

#define DEFAULT_PACK_CHAIN_CAPACITY 64
#define PACK_CHAIN_GROWTH_FACTOR 2.0
#define PACK_CHAIN_MINIMUM_GROWTH 1024
#define PACK_CHAIN_MAXIMUM_GROWTH 65536

#define PACK_CHAIN_EXPAND_TO_FIT(buffer, buffer_idx, buffer_sz) \
  expand_to_fit(                                                \
      buffer,                                                   \
      buffer_idx,                                               \
      buffer_sz,                                                \
      1,                                                        \
      sizeof(pack_index_entry_t),                               \
      PACK_CHAIN_GROWTH_FACTOR,                                 \
      PACK_CHAIN_MINIMUM_GROWTH,                                \
      PACK_CHAIN_MAXIMUM_GROWTH)

static pack_chain_t build_pack_chain(
    const datapack_handle_t* handle,
    const uint8_t node[NODE_SZ]) {
  pack_chain_t pack_chain;
  pack_chain.links_idx = 0;
  pack_chain.links_sz = DEFAULT_PACK_CHAIN_CAPACITY;
  pack_chain.pack_chain_links =
      malloc(pack_chain.links_sz * sizeof(pack_index_entry_t));
  if (pack_chain.pack_chain_links == NULL) {
    pack_chain.code = PACK_CHAIN_OOM;
    goto error_cleanup;
  }

  pack_index_entry_t entry;

  // find the first entry.
  if (find(handle, node, &entry) == false) {
    pack_chain.code = PACK_CHAIN_NOT_FOUND;
    goto error_cleanup;
  }

  if (PACK_CHAIN_EXPAND_TO_FIT(
          (void**)&pack_chain.pack_chain_links,
          pack_chain.links_idx,
          &pack_chain.links_sz) == false) {
    pack_chain.code = PACK_CHAIN_OOM;
    goto error_cleanup;
  }

  pack_chain.pack_chain_links[pack_chain.links_idx++] = entry;

  while (entry.deltabase_index_offset != FULLTEXTINDEXMARK &&
         entry.deltabase_index_offset != NOBASEINDEXMARK) {
    index_offset_t index_num =
        entry.deltabase_index_offset / sizeof(disk_index_entry_t);
    unpack_disk_deltachunk(&handle->index_table[index_num], &entry);

    if (PACK_CHAIN_EXPAND_TO_FIT(
            (void**)&pack_chain.pack_chain_links,
            pack_chain.links_idx,
            &pack_chain.links_sz) == false) {
      pack_chain.code = PACK_CHAIN_OOM;
      goto error_cleanup;
    }

    pack_chain.pack_chain_links[pack_chain.links_idx++] = entry;
  }
  pack_chain.code = PACK_CHAIN_OK;

  return pack_chain;

error_cleanup:
  free(pack_chain.pack_chain_links);

  return pack_chain;
}

static inline uint32_t load_le32(const uint8_t* d) {
  return d[0] | (d[1] << 8) | (d[2] << 16) | (d[3] << 24);
}

static inline int platform_madvise_away(void* ptr, size_t len) {
#if defined(__linux__)
  // linux madvise insists on being on page boundaries.
  intptr_t address = (intptr_t)ptr;
  intptr_t end_address = address + len;

  // round down the address to the nearest page.
  address = address & ~((intptr_t)(PAGE_SIZE - 1));

  // round up the end address to the nearest page.
  end_address += (PAGE_SIZE - 1);
  end_address = end_address & ~((intptr_t)(PAGE_SIZE - 1));

  return madvise((void*)address, (end_address - address), MADVISE_FREE_CODE);
#endif /* #if defined(__linux__) */
#if defined(__APPLE__)
  return madvise(ptr, len, MADVISE_FREE_CODE);
#endif /* #if defined(__APPLE__) */
#if defined(_MSC_VER)
  // There's no madvise in mman-win32
  errno = EINVAL;
  return -1;
#endif /* #if defined(_MSC_VER) */
}

get_delta_chain_link_result_t getdeltachainlink(
    const datapack_handle_t* handle,
    const uint8_t* ptr,
    delta_chain_link_t* link) {
  link->filename_sz = ntohs(*((uint16_t*)ptr));
  ptr += sizeof(uint16_t);

  link->filename = (const char*)ptr;
  ptr += link->filename_sz;

  link->node = ptr;
  ptr += NODE_SZ;

  link->deltabase_node = ptr;
  ptr += NODE_SZ;

  data_offset_t compressed_sz = ntohll(*((uint64_t*)ptr)) - sizeof(uint32_t);
  link->compressed_sz = compressed_sz;
  ptr += sizeof(data_offset_t);

  link->delta_sz = load_le32(ptr); /* LZ4 header */
  ptr += sizeof(uint32_t);
  link->compressed_buf = ptr; /* compressed_* exclude lz4 header */

  link->delta = NULL; /* call uncompressdeltachainlink to decompress it */
  ptr += compressed_sz;

  if (handle->version == 1) {
    // v1 has metadata block
    link->meta_sz = ntohl(*((uint32_t*)ptr));
    ptr += sizeof(uint32_t);
    link->meta = ptr;
    ptr += link->meta_sz;
  } else {
    link->meta_sz = 0;
    link->meta = NULL;
  }

  return COMPOUND_LITERAL(get_delta_chain_link_result_t){
      GET_DELTA_CHAIN_LINK_OK, ptr};
}

bool uncompressdeltachainlink(delta_chain_link_t* link) {
  if (link->delta != NULL || link->delta_sz == 0) {
    // previously decompressed or no content to decompress
    return true;
  }

  uint8_t* decompress_output = malloc((size_t)link->delta_sz);
  if (decompress_output == NULL) {
    // oom
    return false;
  }

  int32_t outbytes = LZ4_decompress_safe(
      (const char*)link->compressed_buf,
      (char*)decompress_output,
      (int)link->compressed_sz,
      (int32_t)link->delta_sz);
  if (outbytes != (int32_t)link->delta_sz) {
    // size mismatch
    free(decompress_output);
    return false;
  }

  link->delta = decompress_output;
  return true;
}

delta_chain_t getdeltachain(
    datapack_handle_t* handle,
    const uint8_t node[NODE_SZ]) {
  pack_chain_t pack_chain = build_pack_chain(handle, node);

  switch (pack_chain.code) {
    case PACK_CHAIN_NOT_FOUND:
      return COMPOUND_LITERAL(delta_chain_t){GET_DELTA_CHAIN_NOT_FOUND};

    case PACK_CHAIN_OOM:
      return COMPOUND_LITERAL(delta_chain_t){GET_DELTA_CHAIN_OOM};

    case PACK_CHAIN_OK:
      break;
  }

  delta_chain_t result;
  result.links_count = pack_chain.links_idx;
  result.delta_chain_links =
      malloc(result.links_count * sizeof(delta_chain_link_t));
  if (result.delta_chain_links == NULL) {
    result.code = GET_DELTA_CHAIN_OOM;
    goto error_cleanup;
  }

  for (int ix = 0; ix < pack_chain.links_idx; ix++) {
    const uint8_t* ptr = handle->data_mmap;
    ptr += pack_chain.pack_chain_links[ix].data_offset;
    const uint8_t* end = ptr + pack_chain.pack_chain_links[ix].data_sz;

    delta_chain_link_t* link = &result.delta_chain_links[ix];
    get_delta_chain_link_result_t next;

    next = getdeltachainlink(handle, ptr, link);
    if (!uncompressdeltachainlink(link)) {
      result.code = GET_DELTA_CHAIN_CORRUPT;
      goto error_cleanup;
    }

    switch (next.code) {
      case GET_DELTA_CHAIN_LINK_OK:
        break;

      case GET_DELTA_CHAIN_LINK_OOM:
        result.code = GET_DELTA_CHAIN_OOM;
        goto error_cleanup;

      case GET_DELTA_CHAIN_LINK_CORRUPT:
        result.code = GET_DELTA_CHAIN_CORRUPT;
        goto error_cleanup;
    }
    if (next.ptr > end) {
      result.code = GET_DELTA_CHAIN_CORRUPT;
      goto error_cleanup;
    }
  }

  for (int ix = 0; ix < pack_chain.links_idx; ix++) {
    const uint8_t* ptr = handle->data_mmap;
    ptr += pack_chain.pack_chain_links[ix].data_offset;
    const uint8_t* end = ptr + pack_chain.pack_chain_links[ix].data_sz;

    handle->paged_in_datapack_memory += (end - ptr);
  }

  if (handle->paged_in_datapack_memory > MAX_PAGED_IN_DATAPACK) {
    platform_madvise_away(handle->data_mmap, (size_t)handle->data_file_sz);
    handle->paged_in_datapack_memory = 0;
  }

  result.code = GET_DELTA_CHAIN_OK;

  goto cleanup;

error_cleanup:
  free(result.delta_chain_links);

cleanup:

  // free pack chain.
  free(pack_chain.pack_chain_links);

  return result;
}

void freedeltachain(delta_chain_t chain) {
  for (size_t ix = 0; ix < chain.links_count; ix++) {
    free((void*)chain.delta_chain_links[ix].delta);
  }
  free(chain.delta_chain_links);
}
