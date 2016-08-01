// Copyright 2016-present Facebook. All Rights Reserved.
//
// cdatapack: 
//
// no-check-code

#ifndef CDATAPACK_CDATAPACK_H
#define CDATAPACK_CDATAPACK_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#define NODE_SZ 20

typedef uint32_t index_offset_t;
#define ntoh_index_offset ntohl
#define FULLTEXTINDEXMARK ((index_offset_t) -1)
#define NOBASEINDEXMARK   ((index_offset_t) -2)
typedef uint64_t data_offset_t;
#define ntoh_data_offset ntohll

struct _disk_index_entry_t;
struct _fanout_table_entry_t;

typedef struct _datapack_handle_t {
  int indexfd;
  int datafd;
  void* index_mmap;
  void* data_mmap;
  off_t index_file_sz;
  off_t data_file_sz;

  bool large_fanout;

  // this is the computed fanout table.
  struct _fanout_table_entry_t *fanout_table;

  // this points to the first index entry.
  struct _disk_index_entry_t* index_table;

  // this points to the entry one past the last.
  struct _disk_index_entry_t* index_end;

} datapack_handle_t;

/**
 * This represents a single entry in a delta chain.
 */
typedef struct _delta_chain_link_t {
  uint16_t filename_sz;
  const char *filename;
  const uint8_t *node;
  const uint8_t *deltabase_node;

  data_offset_t delta_sz;
  const uint8_t *delta;
} delta_chain_link_t;

/**
 * This represents an entire delta chain.
 */
typedef struct _delta_chain_t {
  delta_chain_link_t *delta_chain_links;
  size_t links_count;
} delta_chain_t;

/**
 * Open a datapack + index file.  The fanout table is read and processed at
 * this point.
 *
 * Returns a handle for subsequent operations.
 */
extern datapack_handle_t *open_datapack(
    char *indexfp, size_t indexfp_sz,
    char *datafp, size_t datafp_sz);

/**
 * Release a datapack + index file handle.
 */
extern void close_datapack(datapack_handle_t *);

/**
 * Retrieves a delta chain for a given node.
 */
extern delta_chain_t *getdeltachain(
    const datapack_handle_t *handle,
    uint8_t node[NODE_SZ]);

// this should really be private, but we need it for the cdatapack_dump tool.
extern const uint8_t *getdeltachainlink(
    const uint8_t *ptr, delta_chain_link_t *link);

#endif //CDATAPACK_CDATAPACK_H
