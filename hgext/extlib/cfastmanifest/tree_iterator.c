// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_iterator.c: implementation for traversing all the nodes of a tree
// in-order.
//
// no-check-code

#include <stdlib.h>

#include "hgext/extlib/cfastmanifest/tree.h"
#include "node.h"
#include "path_buffer.h"
#include "tree_iterator.h"

#define DEFAULT_PATH_RECORDS_SZ 1024

iterator_t* create_iterator(const tree_t* tree, bool construct_paths) {
  iterator_t* result = malloc(sizeof(iterator_t));
  path_record_t* path_records =
      malloc(sizeof(path_record_t) * DEFAULT_PATH_RECORDS_SZ);
  char* path = malloc(DEFAULT_PATH_BUFFER_SZ);

  if (result == NULL || path_records == NULL || path == NULL ||
      (result->copy = copy_tree(tree)) == NULL) {
    goto fail;
  }

  // success!
  result->path_records = path_records;
  result->path_records_idx = 0;

  result->path = path;
  result->path_idx = 0;
  result->path_sz = DEFAULT_PATH_BUFFER_SZ;

  result->construct_paths = construct_paths;

  return result;

fail:
  if (result != NULL) {
    if (result->copy != NULL) {
      destroy_tree(result->copy);
    }
    free(result);
  }

  free(path_records);
  free(path);

  return NULL;
}

typedef enum {
  ITERATOR_FOUND,
  ITERATOR_NOT_FOUND,
  ITERATOR_OOM,
  ITERATOR_ERROR,
} iterator_progress_t;

static iterator_progress_t iterator_find_next(iterator_t* iterator) {
  if (iterator->path_records_idx == DEFAULT_PATH_RECORDS_SZ) {
    // we've traversed too deep.
    abort();
  }

  while (iterator->path_records_idx > 0) {
    size_t read_idx = iterator->path_records_idx - 1;

    if (iterator->path_records[read_idx].child_idx <
        iterator->path_records[read_idx].node->num_children) {
      if (!VERIFY_CHILD_NUM(iterator->path_records[read_idx].child_idx)) {
        return ITERATOR_ERROR;
      }

      node_t* candidate = get_child_by_index(
          iterator->path_records[read_idx].node,
          (child_num_t)iterator->path_records[read_idx].child_idx);

      if (iterator->construct_paths && candidate->type != TYPE_ROOT) {
        // if it's not a root node, we need to slap on the name.
        if (PATH_APPEND(
                &iterator->path,
                &iterator->path_idx,
                &iterator->path_sz,
                candidate->name,
                candidate->name_sz) == false) {
          return ITERATOR_OOM;
        }
      }

      // if it's a leaf node, we have the name already added to the path if
      // required.  remember where we are so we can continue.
      if (candidate->type == TYPE_LEAF) {
        return ITERATOR_FOUND;
      }

      // has to either be TYPE_IMPLICIT or TYPE_ROOT at this point.  set up
      // the next path record and descend into the directory.
      iterator->path_records[iterator->path_records_idx].node = candidate;
      iterator->path_records[iterator->path_records_idx].child_idx = 0;
      iterator->path_records[iterator->path_records_idx].previous_path_idx =
          iterator->path_idx;
      iterator->path_records_idx++;

      // start at the top of the while loop again.
      continue;
    }

    // done considering all the children at this level, pop off a path record
    // and continue.
    iterator->path_records_idx--;

    // if we have parents, we should restore the state
    if (iterator->path_records_idx > 0) {
      // path_record_idx is where we write the *next* record, so we have to go
      // back up one more record.
      size_t parent_idx = iterator->path_records_idx - 1;
      iterator->path_idx = iterator->path_records[parent_idx].previous_path_idx;
      iterator->path_records[parent_idx].child_idx++;
    }
  }

  return ITERATOR_NOT_FOUND;
}

iterator_result_t iterator_next(iterator_t* iterator) {
  // special case: if we haven't started iterating yet, then there will be no
  // path records.
  if (iterator->path_records_idx == 0) {
    // search for the first leaf node.
    const node_t* search_start =
        get_child_by_index(iterator->copy->shadow_root, 0);

    // record the progress into the iterator struct
    iterator->path_records[0].node = search_start;
    iterator->path_records[0].child_idx = 0;
    iterator->path_records[0].previous_path_idx = 0;

    // at the start, reads come from 0, writes go to 1.
    iterator->path_records_idx = 1;
  } else {
    size_t read_idx = iterator->path_records_idx - 1;
    iterator->path_records[read_idx].child_idx++;

    // truncate the path up to the last directory.
    iterator->path_idx = iterator->path_records[read_idx].previous_path_idx;
  }

  iterator_progress_t progress = iterator_find_next(iterator);

  iterator_result_t result;
  if (progress == ITERATOR_FOUND) {
    size_t read_idx = iterator->path_records_idx - 1;

    path_record_t* record = &iterator->path_records[read_idx];
    if (!VERIFY_CHILD_NUM(record->child_idx)) {
      abort();
    }
    node_t* child =
        get_child_by_index(record->node, (child_num_t)record->child_idx);

    result.valid = true;
    if (iterator->construct_paths) {
      result.path = iterator->path;
      result.path_sz = iterator->path_idx;
    } else {
      // strictly these shouldn't be necessary, because we only read these
      // fields if we succeed, and that code path does set the fields.  however,
      // gcc doesn't know that and throws a fit.
      result.path = NULL;
      result.path_sz = 0;
    }
    result.checksum = child->checksum;
    result.checksum_sz = child->checksum_sz;
    result.flags = child->flags;
  } else {
    result.valid = false;

    // strictly these shouldn't be necessary, because we only read these fields
    // if we succeed, and that code path does set the fields.  however, gcc
    // doesn't know that and throws a fit.
    result.path = NULL;
    result.path_sz = 0;
    result.checksum = NULL;
    result.checksum_sz = 0;
    result.flags = 0;
  }

  return result;
}

void destroy_iterator(iterator_t* iterator) {
  destroy_tree(iterator->copy);
  free(iterator->path_records);
  free(iterator->path);
  free(iterator);
}
