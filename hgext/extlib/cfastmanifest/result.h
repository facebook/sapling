// Copyright 2016-present Facebook. All Rights Reserved.
//
// result.h: return types for publicly accessible methods.  this is
//           indirectly exposed through tree.h.
//
// no-check-code

#ifndef __FASTMANIFEST_RESULT_H__
#define __FASTMANIFEST_RESULT_H__

typedef enum {
  GET_PATH_OK,
  GET_PATH_NOT_FOUND,
  GET_PATH_WTF,
} get_path_code_t;

typedef struct _get_path_result_t {
  get_path_code_t code;
  const uint8_t* checksum;
  const uint8_t checksum_sz;
  const uint8_t flags;
} get_path_result_t;

typedef enum {
  ADD_UPDATE_PATH_OK,
  ADD_UPDATE_PATH_OOM,
  ADD_UPDATE_PATH_CONFLICT,
  ADD_UPDATE_PATH_WTF,
} add_update_path_result_t;

typedef enum {
  SET_METADATA_OK,
} set_metadata_result_t;

typedef enum {
  REMOVE_PATH_OK,
  REMOVE_PATH_WTF,
  REMOVE_PATH_NOT_FOUND,
} remove_path_result_t;

typedef enum {
  READ_FROM_FILE_OK,
  READ_FROM_FILE_OOM,

  // consult the err field in read_from_file_result_t for more details.
  READ_FROM_FILE_NOT_READABLE,

  // should nuke this file.  it doesn't parse logically.
  READ_FROM_FILE_WTF,

  // should nuke this file.  it might parse logically, but not on this host.
  READ_FROM_FILE_NOT_USABLE,
} read_from_file_code_t;
typedef struct _read_from_file_result_t {
  read_from_file_code_t code;
  int err;
  struct _tree_t* tree;
} read_from_file_result_t;

typedef enum {
  WRITE_TO_FILE_OK,
  WRITE_TO_FILE_OOM,
  WRITE_TO_FILE_WTF,
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

typedef enum {
  DIFF_OK,
  DIFF_OOM,
  DIFF_WTF,
} diff_result_t;

typedef struct _iterator_result_t {
  bool valid;
  const char* path;
  size_t path_sz;
  const uint8_t* checksum;
  uint8_t checksum_sz;
  uint8_t flags;
} iterator_result_t;

#endif /* #ifndef __FASTMANIFEST_RESULT_H__ */
