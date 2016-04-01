// Copyright 2016-present Facebook. All Rights Reserved.
//
// result.h: return types for publicly accessible methods.  this is
//           indirectly exposed through tree.h.

#ifndef __FASTMANIFEST_RESULT_H__
#define __FASTMANIFEST_RESULT_H__

typedef enum {
  GET_PATH_OK,
  GET_PATH_NOT_FOUND,
  GET_PATH_WTF,
} get_path_code_t;

typedef struct _get_path_result_t {
  get_path_code_t code;
  struct _node_t* node;
} get_path_result_t;

typedef enum _add_update_path_result_t {
  ADD_UPDATE_PATH_OK,
  ADD_UPDATE_PATH_OOM,
  ADD_UPDATE_PATH_CONFLICT,
  ADD_UPDATE_PATH_WTF,
} add_update_path_result_t;

typedef enum _set_metadata_result_t {
  SET_METADATA_OK,
} set_metadata_result_t;

typedef enum _remove_path_result_t {
  REMOVE_PATH_OK,

} remove_path_result_t;

typedef enum _write_to_file_result_t {
  WRITE_TO_FILE_OK,

} write_to_file_result_t;

typedef enum {
  CONVERT_FROM_FLAT_OK,
  CONVERT_FROM_FLAT_OOM,
  CONVERT_FROM_FLAT_WTF,
} convert_from_flat_code_t;
typedef struct _convert_from_flat_result_t {
  convert_from_flat_code_t code;
  struct _tree_t* tree;
} convert_from_flat_result_t;

typedef enum {
  CONVERT_TO_FLAT_OK,
  CONVERT_TO_FLAT_OOM,
  CONVERT_TO_FLAT_WTF,
} convert_to_flat_code_t;
typedef struct _convert_to_flat_result_t {
  convert_to_flat_code_t code;
  char* flat_manifest;
  size_t flat_manifest_sz;
} convert_to_flat_result_t;

#endif /* #ifndef __FASTMANIFEST_RESULT_H__ */
