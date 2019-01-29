// Copyright 2016-present Facebook. All Rights Reserved.
//
// tree_convert.c: methods to convert flat manifests to and from a tree.
//
// no-check-code

#include <stdlib.h>
#include <sys/types.h>

#include "edenscm/hgext/extlib/cfastmanifest/tree.h"
#include "lib/clib/buffer.h"
#include "lib/clib/convert.h"
#include "lib/clib/portability/portability.h"
#include "edenscm/mercurial/compat.h"
#include "path_buffer.h"
#include "tree_arena.h"

#define MAX_FOLDER_DEPTH 1024
#define DEFAULT_CHILDREN_CAPACITY 4096

#define BUFFER_GROWTH_FACTOR 1.2
#define BUFFER_MINIMUM_GROWTH 1048576
#define BUFFER_MAXIMUM_GROWTH (32 * 1024 * 1024)

#define CONVERT_EXPAND_TO_FIT(buffer, buffer_idx, buffer_sz, input_sz) \
  expand_to_fit(                                                       \
      (void**)buffer,                                                  \
      buffer_idx,                                                      \
      buffer_sz,                                                       \
      input_sz,                                                        \
      sizeof(char),                                                    \
      BUFFER_GROWTH_FACTOR,                                            \
      BUFFER_MINIMUM_GROWTH,                                           \
      BUFFER_MAXIMUM_GROWTH)

typedef struct _open_folder_t {
  const char* subfolder_name;
  /* this is a reference to the flat
   * manifest's memory.  we do not own
   * this memory, and we must copy it
   * before the conversion completes. */
  size_t subfolder_name_sz;

  // readers may wonder why we store a relative pointer.  this is because
  // storing node_t* pointers is UNSAFE.  they are allocated on the arena, and
  // can be moved at a moment's notice.  the only thing that's safe to do is to
  // store an offset from the start of the arena.
  ptrdiff_t closed_children_prealloc[DEFAULT_CHILDREN_CAPACITY];

  ptrdiff_t* closed_children;
  size_t closed_children_count;
  size_t closed_children_capacity;

  bool in_use;
} open_folder_t;

typedef struct _from_flat_state_t {
  tree_t* tree;
  open_folder_t folders[MAX_FOLDER_DEPTH];
  size_t open_folder_count;
} from_flat_state_t;

typedef struct _to_flat_state_t {
  const tree_t* tree;
  char* dirpath_build_buffer;
  size_t dirpath_build_buffer_idx;
  size_t dirpath_build_buffer_sz;

  char* output_buffer;
  size_t output_buffer_idx;
  size_t output_buffer_sz;
} to_flat_state_t;

/**
 * Returns <0 if (`name`, `name_sz`) is lexicographically less than the name in
 * folder.
 *
 * Returns =0 if (`name`, `name_sz`) is lexicographically equal to the name in
 * folder.
 *
 * Returns >0 if (`name`, `name_sz`) is lexicographically greater than the name
 * in folder.
 */
static inline int folder_name_compare(
    const char* name,
    size_t name_sz,
    const open_folder_t* folder) {
  size_t min_sz = (name_sz < folder->subfolder_name_sz)
      ? name_sz
      : folder->subfolder_name_sz;
  ssize_t sz_compare = name_sz - folder->subfolder_name_sz;

  int cmp = strncmp(name, folder->subfolder_name, min_sz);
  if (cmp) {
    return cmp;
  } else if (sz_compare < 0) {
    return -1;
  } else if (sz_compare > 0) {
    return 1;
  } else {
    return 0;
  }
}

static void init_open_folder(open_folder_t* folder) {
  folder->in_use = false;
  folder->closed_children = folder->closed_children_prealloc;
  folder->closed_children_count = 0;
  folder->closed_children_capacity = DEFAULT_CHILDREN_CAPACITY;
}

static from_flat_state_t* init_from_state(size_t flat_sz) {
  from_flat_state_t* state = malloc(sizeof(from_flat_state_t));
  if (state == NULL) {
    return NULL;
  }

  for (int ix = 0; ix < MAX_FOLDER_DEPTH; ix++) {
    init_open_folder(&state->folders[ix]);
  }
  state->open_folder_count = 0;

  state->tree = alloc_tree_with_arena(flat_sz * 2);

  return state;
}

/**
 * Adds a child to a folder, expanding it as needed.
 */
static bool folder_add_child(
    from_flat_state_t* state,
    open_folder_t* folder,
    node_t* child) {
  if (folder->closed_children_count + 1 == folder->closed_children_capacity) {
    // time to expand the folder
    size_t new_capacity = folder->closed_children_capacity * 2;

    // is the current zone the prealloc zone?  if so, we need to allocate a new
    // zone.
    if (folder->closed_children == folder->closed_children_prealloc) {
      folder->closed_children = malloc(sizeof(ptrdiff_t) * new_capacity);
      if (folder->closed_children == NULL) {
        return false;
      }

      // copy over.
      memcpy(
          folder->closed_children,
          folder->closed_children_prealloc,
          sizeof(ptrdiff_t) * folder->closed_children_count);
    } else {
      // realloc
      folder->closed_children =
          realloc(folder->closed_children, sizeof(ptrdiff_t) * new_capacity);
      if (folder->closed_children == NULL) {
        return false;
      }
    }

    folder->closed_children_capacity = new_capacity;
  }

  // we need to store the delta between the start of the arena and the child.
  intptr_t arena_start = (intptr_t)state->tree->arena;
  intptr_t child_start = (intptr_t)child;
  folder->closed_children[folder->closed_children_count] =
      child_start - arena_start;
  folder->closed_children_count++;

  return true;
}

typedef enum {
  CLOSE_FOLDER_OK,
  CLOSE_FOLDER_OOM,
} close_folder_code_t;
typedef struct _close_folder_result_t {
  close_folder_code_t code;
  node_t* node;
} close_folder_result_t;

/**
 * Close the folder at index `folder_index`.  This may require closing nested
 * folders.  If folder_index is > 0, then add the closed folder to its parent.
 * If the folder_index is 0, it is responsibility of the caller to attach the
 * returned node to the shadow root.
 */
static close_folder_result_t close_folder(
    from_flat_state_t* state,
    size_t folder_index) {
  open_folder_t* folder = &state->folders[folder_index];
  assert(folder->in_use == true);

  if (folder_index < MAX_FOLDER_DEPTH - 1) {
    // maybe a nested folder needs to be closed?
    if (state->folders[folder_index + 1].in_use) {
      // yup, it needs to be closed.
      close_folder_result_t close_folder_result =
          close_folder(state, folder_index + 1);

      if (close_folder_result.code != CLOSE_FOLDER_OK) {
        return COMPOUND_LITERAL(close_folder_result_t){close_folder_result.code,
                                                       NULL};
      }
    }
  }

  // allocate a node and set it up.
  arena_alloc_node_result_t arena_alloc_node_result = arena_alloc_node(
      state->tree,
      folder->subfolder_name,
      folder->subfolder_name_sz,
      folder->closed_children_count);

  if (arena_alloc_node_result.code == ARENA_ALLOC_OOM) {
    return COMPOUND_LITERAL(close_folder_result_t){CLOSE_FOLDER_OOM, NULL};
  }
  node_t* node = arena_alloc_node_result.node;
  node->type = TYPE_IMPLICIT;
  // we must initialize flags to a known value, even if it's not used
  // because it participates in checksum calculation.
  node->flags = 0;
  if (!VERIFY_CHILD_NUM(folder->closed_children_count)) {
    abort();
  }
  // this is a huge abstraction violation, but it allows us to use
  // `set_child_by_index`, which is significantly more efficient.
  node->num_children = (child_num_t)folder->closed_children_count;

  // node is set up.  now add all the children!
  intptr_t arena_start = (intptr_t)state->tree->arena;
  for (size_t ix = 0; ix < folder->closed_children_count; ix++) {
    ptrdiff_t child_offset = (intptr_t)folder->closed_children[ix];
    intptr_t address = arena_start + child_offset;

    set_child_by_index(node, ix, (node_t*)address);
  }

  init_open_folder(folder); // zap the folder so it can be reused.
  state->open_folder_count--;

  // attach to parent folder if it's not the root folder.
  assert(folder_index == state->open_folder_count);
  if (folder_index > 0) {
    open_folder_t* parent_folder = &state->folders[folder_index - 1];
    if (folder_add_child(state, parent_folder, node) == false) {
      return COMPOUND_LITERAL(close_folder_result_t){CLOSE_FOLDER_OOM, NULL};
    }
  }

  return COMPOUND_LITERAL(close_folder_result_t){CLOSE_FOLDER_OK, node};
}

typedef enum {
  PROCESS_PATH_OK,
  PROCESS_PATH_OOM,
  PROCESS_PATH_CORRUPT,
} process_path_code_t;
typedef struct _process_path_result_t {
  process_path_code_t code;
  // the following are only set when the code is `PROCESS_PATH_OK`.
  node_t* node; // do *NOT* save this pointer.
                // immediately do what is needed with
                // this pointer and discard.  the reason
                // is that it's part of the arena, and
                // can be moved if the arena is resized.
  size_t bytes_consumed; // this is the number of bytes consumed,
                         // including the null pointer.
} process_path_result_t;

/**
 * Process a null-terminated path, closing any directories and building the
 * nodes as needed, and opening the new directories to support the current path.
 *
 * Once the proper set of folders are open, create a node and write it into
 * the folder.
 */
static process_path_result_t
process_path(from_flat_state_t* state, const char* path, size_t max_len) {
  size_t path_scan_index;
  size_t current_path_start;
  size_t open_folder_index;

  // match as many path components as we can
  for (path_scan_index = 0, current_path_start = 0, open_folder_index = 0;
       path[path_scan_index] != 0;
       path_scan_index++) {
    if (path_scan_index == max_len) {
      return COMPOUND_LITERAL(process_path_result_t){
          PROCESS_PATH_CORRUPT, NULL, 0};
    }

    // check for a path separator.
    if (path[path_scan_index] != '/') {
      continue;
    }
    size_t path_len =
        path_scan_index + 1 /* to include the / */ - current_path_start;

    bool open_new_folder = true;

    // check if the *next* open folder is valid, and if it matches the path
    // component we just found.
    if (open_folder_index + 1 < state->open_folder_count) {
      if (folder_name_compare(
              &path[current_path_start],
              path_len,
              &state->folders[open_folder_index + 1]) == 0) {
        // we found the folder we needed, so we can just reuse it.
        open_new_folder = false;
        open_folder_index++;
      } else {
        close_folder_result_t close_folder_result =
            close_folder(state, open_folder_index + 1);
        if (close_folder_result.code == CLOSE_FOLDER_OOM) {
          return COMPOUND_LITERAL(process_path_result_t){
              PROCESS_PATH_OOM, NULL, 0};
        }
      }
    }

    if (open_new_folder == true) {
      // if we're opening a new folder, that means there should be no child
      // folders open.
      assert(state->open_folder_count == open_folder_index + 1);
      open_folder_index++;
      state->open_folder_count++;
      open_folder_t* folder = &state->folders[open_folder_index];

      assert(folder->in_use == false);
      assert(folder->closed_children == folder->closed_children_prealloc);
      assert(folder->closed_children_count == 0);

      // link the name in.  remember, we don't own the memory!!
      folder->in_use = true;
      folder->subfolder_name = &path[current_path_start];
      folder->subfolder_name_sz = path_len;
    }

    // path starts after the /
    current_path_start = path_scan_index + 1;
  }

  // close path components that are not matched, building their nodes.
  if (open_folder_index + 1 < state->open_folder_count) {
    close_folder_result_t close_folder_result =
        close_folder(state, open_folder_index + 1);
    if (close_folder_result.code == CLOSE_FOLDER_OOM) {
      return COMPOUND_LITERAL(process_path_result_t){PROCESS_PATH_OOM, NULL, 0};
    }
  }

  // build a node for the remaining path (which should just be the
  // filename).  add it to the currently open folder.
  arena_alloc_node_result_t arena_alloc_node_result = arena_alloc_node(
      state->tree,
      &path[current_path_start],
      path_scan_index - current_path_start,
      0);

  if (arena_alloc_node_result.code == ARENA_ALLOC_OOM) {
    return COMPOUND_LITERAL(process_path_result_t){PROCESS_PATH_OOM, NULL, 0};
  }

  arena_alloc_node_result.node->type = TYPE_LEAF;

  // jam the new node into the currently open folder.
  open_folder_t* folder = &state->folders[open_folder_index];
  folder_add_child(state, folder, arena_alloc_node_result.node);

  return COMPOUND_LITERAL(process_path_result_t){
      PROCESS_PATH_OK, arena_alloc_node_result.node, path_scan_index + 1};
}

static convert_from_flat_result_t convert_from_flat_helper(
    from_flat_state_t* state,
    char* manifest,
    size_t manifest_sz) {
  // open the root directory node.
  open_folder_t* folder = &state->folders[0];
  folder->subfolder_name = "/";
  folder->subfolder_name_sz = 1;
  folder->in_use = true;
  state->open_folder_count++;

  for (size_t ptr = 0; ptr < manifest_sz;) {
    // filename is up to the first null.
    process_path_result_t pp_result =
        process_path(state, &manifest[ptr], manifest_sz - ptr);

    switch (pp_result.code) {
      case PROCESS_PATH_OOM:
        return COMPOUND_LITERAL(convert_from_flat_result_t){
            CONVERT_FROM_FLAT_OOM, NULL};
      case PROCESS_PATH_CORRUPT:
        return COMPOUND_LITERAL(convert_from_flat_result_t){
            CONVERT_FROM_FLAT_WTF, NULL};
      case PROCESS_PATH_OK:
        break;
    }

    assert(pp_result.code == PROCESS_PATH_OK);
    node_t* node = pp_result.node;
    ptr += pp_result.bytes_consumed;
    size_t remaining = manifest_sz - ptr;
    if (remaining <= SHA1_BYTES * 2) {
      // not enough characters for the checksum and the NL.  well, that's a
      // fail.
      return COMPOUND_LITERAL(convert_from_flat_result_t){CONVERT_FROM_FLAT_WTF,
                                                          NULL};
    }

    if (unhexlify(&manifest[ptr], SHA1_BYTES * 2, node->checksum) == false) {
      return COMPOUND_LITERAL(convert_from_flat_result_t){CONVERT_FROM_FLAT_WTF,
                                                          NULL};
    }
    node->checksum_sz = SHA1_BYTES;
    node->checksum_valid = true;
    ptr += SHA1_BYTES * 2;

    // is the next character a NL?  if so, then we're done.  otherwise, retrieve
    // it as the flags field.
    if (manifest[ptr] != '\n') {
      node->flags = manifest[ptr];
      ptr++;
    } else {
      node->flags = 0;
    }
    ptr++;

    state->tree->num_leaf_nodes++;
  }

  // close the root folder.
  close_folder_result_t close_result = close_folder(state, 0);
  if (close_result.code == CLOSE_FOLDER_OOM) {
    return COMPOUND_LITERAL(convert_from_flat_result_t){CONVERT_FROM_FLAT_OOM,
                                                        NULL};
  }

  close_result.node->type = TYPE_ROOT;
  add_child(state->tree->shadow_root, close_result.node);

  return COMPOUND_LITERAL(convert_from_flat_result_t){CONVERT_FROM_FLAT_OK,
                                                      state->tree};
}

static convert_to_flat_code_t convert_to_flat_iterator(
    to_flat_state_t* state,
    const node_t* node) {
  assert(node->type == TYPE_IMPLICIT || node->type == TYPE_ROOT);

  for (uint32_t ix = 0; ix < node->num_children; ix++) {
    node_t* child = get_child_by_index(node, ix);

    if (child->type == TYPE_LEAF) {
      size_t space_needed = state->dirpath_build_buffer_idx + child->name_sz +
          1 /* null character */ + (SHA1_BYTES * 2) +
          (child->flags != '\000' ? 1 : 0) + 1 /* NL */;

      if (CONVERT_EXPAND_TO_FIT(
              &state->output_buffer,
              state->output_buffer_idx,
              &state->output_buffer_sz,
              space_needed) == false) {
        return CONVERT_TO_FLAT_OOM;
      }

      // copy the dirpath over to the output buffer.
      memcpy(
          &state->output_buffer[state->output_buffer_idx],
          state->dirpath_build_buffer,
          state->dirpath_build_buffer_idx);
      state->output_buffer_idx += state->dirpath_build_buffer_idx;

      // copy the filename over to the output buffer.
      memcpy(
          &state->output_buffer[state->output_buffer_idx],
          child->name,
          child->name_sz);
      state->output_buffer_idx += child->name_sz;

      // copy the filename over to the output buffer.
      state->output_buffer[state->output_buffer_idx] = '\000';
      state->output_buffer_idx++;

      // transcribe the sha over.
      hexlify(
          child->checksum,
          SHA1_BYTES,
          &state->output_buffer[state->output_buffer_idx]);
      state->output_buffer_idx += (SHA1_BYTES * 2);

      if (child->flags != '\000') {
        state->output_buffer[state->output_buffer_idx] = child->flags;
        state->output_buffer_idx++;
      }

      state->output_buffer[state->output_buffer_idx] = '\n';
      state->output_buffer_idx++;

      assert(state->output_buffer_idx < state->output_buffer_sz);
    } else {
      // save the old value...
      size_t previous_dirpath_build_buffer_idx =
          state->dirpath_build_buffer_idx;

      if (PATH_APPEND(
              &state->dirpath_build_buffer,
              &state->dirpath_build_buffer_idx,
              &state->dirpath_build_buffer_sz,
              child->name,
              child->name_sz) == false) {
        return CONVERT_TO_FLAT_OOM;
      }

      convert_to_flat_iterator(state, child);

      state->dirpath_build_buffer_idx = previous_dirpath_build_buffer_idx;
    }
  }

  return CONVERT_TO_FLAT_OK;
}

static convert_to_flat_code_t convert_to_flat_helper(
    to_flat_state_t* state,
    const tree_t* tree) {
  // get the real root.
  node_t* shadow_root = tree->shadow_root;
  if (shadow_root->num_children != 1) {
    return CONVERT_TO_FLAT_WTF;
  }

  node_t* real_root = get_child_by_index(shadow_root, 0);

  return convert_to_flat_iterator(state, real_root);
}

convert_from_flat_result_t convert_from_flat(
    char* manifest,
    size_t manifest_sz) {
  from_flat_state_t* state = init_from_state(manifest_sz);

  if (state->tree == NULL) {
    free(state);
    state = NULL;
  }
  if (state == NULL) {
    return COMPOUND_LITERAL(convert_from_flat_result_t){CONVERT_FROM_FLAT_OOM,
                                                        NULL};
  }

  convert_from_flat_result_t result =
      convert_from_flat_helper(state, manifest, manifest_sz);

  if (result.code != CONVERT_FROM_FLAT_OK) {
    free(state->tree);
  }
  free(state);

  return result;
}

convert_to_flat_result_t convert_to_flat(tree_t* tree) {
  to_flat_state_t state;
  state.dirpath_build_buffer = malloc(DEFAULT_PATH_BUFFER_SZ);
  state.dirpath_build_buffer_idx = 0;
  state.dirpath_build_buffer_sz = DEFAULT_PATH_BUFFER_SZ;

  // guestimate as to how much space we need.  this could probably be
  // fine-tuned a bit.
  state.output_buffer = malloc(tree->consumed_memory);
  state.output_buffer_idx = 0;
  state.output_buffer_sz = tree->consumed_memory;

  convert_to_flat_code_t result = CONVERT_TO_FLAT_OOM;
  if (state.dirpath_build_buffer != NULL && state.output_buffer != NULL) {
    result = convert_to_flat_helper(&state, tree);
  }

  free(state.dirpath_build_buffer);

  if (result != CONVERT_TO_FLAT_OK) {
    // free the buffer if any error occurred.
    free(state.output_buffer);
    return COMPOUND_LITERAL(convert_to_flat_result_t){result, NULL, 0};
  } else {
    return COMPOUND_LITERAL(convert_to_flat_result_t){
        CONVERT_TO_FLAT_OK, state.output_buffer, state.output_buffer_idx};
  }
}
