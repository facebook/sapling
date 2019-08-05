// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// cdatapack:
// no-check-code

#ifndef FBHGEXT_CDATAPACK_CDATAPACK_H
#define FBHGEXT_CDATAPACK_CDATAPACK_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

#include "lib/clib/portability/portability.h"

#define NODE_SZ 20

#define PACKSUFFIX ".datapack"
#define PACKSUFFIXLEN 9
#define INDEXSUFFIX ".dataidx"
#define INDEXSUFFIXLEN 8

typedef uint32_t index_offset_t;
#define ntoh_index_offset ntohl
#define FULLTEXTINDEXMARK ((index_offset_t)-1)
#define NOBASEINDEXMARK ((index_offset_t)-2)
typedef uint64_t data_offset_t;

struct _disk_index_entry_t;
struct _fanout_table_entry_t;

/**
 * This is a post-processed index entry.  The node pointer is valid only if
 * the handle that generated this entry hasn't been closed.
 *
 * This is the counterpart of disk_index_entry_t.
 */
typedef struct _pack_index_entry_t {
  const uint8_t* node;

  // offset and size of this current element in the delta chain in the data
  // file.
  data_offset_t data_offset;
  data_offset_t data_sz;

  // offset of the next element in the delta chain in the index file
  index_offset_t deltabase_index_offset;
} pack_index_entry_t;

typedef enum {
  DATAPACK_HANDLE_OK,
  DATAPACK_HANDLE_OOM,
  DATAPACK_HANDLE_IO_ERROR,
  DATAPACK_HANDLE_MMAP_ERROR,
  DATAPACK_HANDLE_CORRUPT,
  DATAPACK_HANDLE_VERSION_MISMATCH,
} datapack_handle_status_t;

typedef struct _datapack_handle_t {
  datapack_handle_status_t status;

  void* index_mmap;
  void* data_mmap;
  off_t index_file_sz;
  off_t data_file_sz;

  bool large_fanout;

  uint8_t version;

  // this is the computed fanout table.
  struct _fanout_table_entry_t* fanout_table;

  // this points to the first index entry.
  struct _disk_index_entry_t* index_table;

  size_t paged_in_datapack_memory;
} datapack_handle_t;

/**
 * This represents a single entry in a delta chain.
 */
typedef struct _delta_chain_link_t {
  uint16_t filename_sz;
  const char* filename;
  const uint8_t* node;
  const uint8_t* deltabase_node;

  data_offset_t compressed_sz;
  const uint8_t* compressed_buf;

  /* delta is (lazily) uncompressed from compressed_buf
   * allocated by uncompressdeltachainlink, and freed by caller */
  data_offset_t delta_sz;
  const uint8_t* delta;

  uint32_t meta_sz;
  const uint8_t* meta;
} delta_chain_link_t;

typedef enum {
  GET_DELTA_CHAIN_OK,
  GET_DELTA_CHAIN_OOM,
  GET_DELTA_CHAIN_NOT_FOUND,
  GET_DELTA_CHAIN_CORRUPT,
} get_delta_chain_code_t;

/**
 * This represents an entire delta chain.
 */
typedef struct _delta_chain_t {
  get_delta_chain_code_t code;
  delta_chain_link_t* delta_chain_links;
  size_t links_count;
} delta_chain_t;

/**
 * Open a datapack + index file.  The fanout table is read and processed at
 * this point.
 *
 * Returns a handle for subsequent operations.
 */
extern datapack_handle_t* open_datapack(
    const char* indexfp,
    size_t indexfp_sz,
    const char* datafp,
    size_t datafp_sz);

/**
 * Release a datapack + index file handle.
 */
extern void close_datapack(datapack_handle_t*);

/**
 * Finds a node using the index, and fills out the packindex pointer.
 * Returns true iff the node is found.
 */
extern bool find(
    const datapack_handle_t* handle,
    const uint8_t node[NODE_SZ],
    pack_index_entry_t* packindex);

/**
 * Retrieves a delta chain for a given node.
 */
extern delta_chain_t getdeltachain(
    datapack_handle_t* handle,
    const uint8_t node[NODE_SZ]);

extern void freedeltachain(delta_chain_t chain);

typedef enum {
  GET_DELTA_CHAIN_LINK_OK,
  GET_DELTA_CHAIN_LINK_OOM,
  GET_DELTA_CHAIN_LINK_CORRUPT,
} get_delta_chain_link_code_t;

/**
 * This represents an entire delta chain.
 */
typedef struct _get_delta_chain_link_result_t {
  get_delta_chain_link_code_t code;
  const uint8_t* ptr;
} get_delta_chain_link_result_t;

// this should really be private, but we need it for the cdatapack_dump tool.
extern get_delta_chain_link_result_t getdeltachainlink(
    const datapack_handle_t* handle,
    const uint8_t* ptr,
    delta_chain_link_t* link);
// caller is responsible for freeing link->delta.
extern bool uncompressdeltachainlink(delta_chain_link_t* link);

#endif // FBHGEXT_CDATAPACK_CDATAPACK_H
