/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 *
 * This file is generated with cbindgen. Please run `./tools/cbindgen.sh` to
 * update this file.
 *
 * @generated SignedSource<<e9e6a479593f98741edd895c404f9a1e>>
 *
 */

// The generated functions are exported from this Rust library
// @dep=:edenapithin

#pragma once

#include <memory>
#include <functional>
#include <folly/Range.h>

// Manually added these forward declarations for out-of-crate opaque types.
// This is probably the wrong way to go about this, but I feel like I shouldn't
// need to write a `#[repr(transparent)]` wrapper type and go through the dance
// of using `unsafe { mem::transmute(foo) }` when I just need a forward delcaration anyway.
struct RustApiKey;
struct RustClient;
struct RustEdenApiError;
struct RustTreeEntry;
struct RustFileMetadata;
struct RustEdenApiServerError;
struct RustTreeChildEntry;
struct RustTreeEntry;
struct RustError;


#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

enum class RustFileType {
  Regular,
  Executable,
  Symlink,
};

template<typename T = void, typename E = void>
struct RustResult;

struct RustString;

template<typename T = void>
struct RustVec;

struct RustEdenApiClient {
  RustResult<RustClient, RustError> *ptr;
};

struct RustTreeEntryFetch {
  RustResult<RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>>, RustError> *ptr;
};

struct RustKey {
  const uint8_t *path;
  size_t path_len;
  uint8_t hgid[20];
};

struct RustTreeAttributes {
  bool manifest_blob;
  bool parents;
  bool child_metadata;
};

struct RustHgId {
  uint8_t _0[20];
};

struct RustParents {
  enum class Tag : uint8_t {
    None,
    One,
    Two,
  };

  struct RustOne_Body {
    uint8_t _0[20];
  };

  struct RustTwo_Body {
    uint8_t _0[20];
    uint8_t _1[20];
  };

  Tag tag;
  union {
    RustOne_Body one;
    RustTwo_Body two;
  };
};

struct RustContentId {
  uint8_t _0[32];
};

struct RustSha1 {
  uint8_t _0[20];
};

struct RustSha256 {
  uint8_t _0[32];
};

/// A wrapper type for a Box<String>. When into_raw_parts is stabilized, the Box / extra allocation
/// can be removed.
struct RustOwnedString {
  RustString *ptr;
};

extern "C" {

RustEdenApiClient rust_edenapi_client_new(const uint8_t *repository, size_t repository_len);

RustTreeEntryFetch rust_edenapi_trees_blocking(RustClient *client,
                                               const uint8_t *repo,
                                               size_t repo_len,
                                               const RustKey *keys,
                                               size_t keys_len,
                                               RustTreeAttributes attrs);

/// Methods for ApiKey
RustHgId rust_key_get_hgid(const RustApiKey *k);

size_t rust_key_get_path_len(const RustApiKey *k);

const uint8_t *rust_key_get_path(const RustApiKey *k);

bool rust_treeentry_has_key(const RustTreeEntry *entry);

const RustApiKey *rust_treeentry_get_key(const RustTreeEntry *entry);

bool rust_treeentry_has_data(const RustTreeEntry *entry);

const uint8_t *rust_treeentry_get_data(const RustTreeEntry *entry);

size_t rust_treeentry_get_len(const RustTreeEntry *entry);

bool rust_treeentry_has_parents(const RustTreeEntry *entry);

RustParents rust_treeentry_get_parents(const RustTreeEntry *entry);

bool rust_treeentry_has_children(const RustTreeEntry *entry);

bool rust_treeentry_get_children_len(const RustTreeEntry *entry);

const RustVec<RustResult<RustTreeChildEntry, RustEdenApiServerError>> *rust_treeentry_get_children(const RustTreeEntry *entry);

bool rust_treechildentry_is_file(const RustTreeChildEntry *entry);

const RustApiKey *rust_treechildentry_get_file_key(const RustTreeChildEntry *entry);

bool rust_treechildentry_has_file_metadata(const RustTreeChildEntry *entry);

const RustFileMetadata *rust_treechildentry_get_file_metadata(const RustTreeChildEntry *entry);

bool rust_treechildentry_is_directory(const RustTreeChildEntry *entry);

const RustApiKey *rust_treechildentry_get_directory_key(const RustTreeChildEntry *entry);

bool rust_filemetadata_has_revisionstore_flags(const RustFileMetadata *m);

bool rust_filemetadata_has_content_id(const RustFileMetadata *m);

bool rust_filemetadata_has_file_type(const RustFileMetadata *m);

bool rust_filemetadata_has_size(const RustFileMetadata *m);

bool rust_filemetadata_has_content_sha1(const RustFileMetadata *m);

bool rust_filemetadata_has_content_sha256(const RustFileMetadata *m);

uint64_t rust_filemetadata_get_revisionstore_flags(const RustFileMetadata *m);

RustContentId rust_filemetadata_get_content_id(const RustFileMetadata *m);

RustFileType rust_filemetadata_get_file_type(const RustFileMetadata *m);

uint64_t rust_filemetadata_get_size(const RustFileMetadata *m);

RustSha1 rust_filemetadata_get_content_sha1(const RustFileMetadata *m);

RustSha256 rust_filemetadata_get_content_sha256(const RustFileMetadata *m);

void rust_edenapiclient_free(RustEdenApiClient v);

void rust_treeentryfetch_free(RustTreeEntryFetch v);

uintptr_t rust_ownedstring_len(const RustOwnedString *s);

const uint8_t *rust_ownedstring_ptr(const RustOwnedString *s);

void rust_ownedstring_free(RustOwnedString v);

const RustTreeEntry *rust_result_treeentry_ok(const RustResult<RustTreeEntry, RustEdenApiServerError> *r);

bool rust_result_treeentry_is_err(const RustResult<RustTreeEntry, RustEdenApiServerError> *r);

RustOwnedString rust_result_treeentry_err_display(const RustResult<RustTreeEntry, RustEdenApiServerError> *r);

RustOwnedString rust_result_treeentry_err_debug(const RustResult<RustTreeEntry, RustEdenApiServerError> *r);

const RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>> *rust_result_entries_ok(const RustResult<RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>>, RustError> *r);

bool rust_result_entries_is_err(const RustResult<RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>>, RustError> *r);

RustOwnedString rust_result_entries_err_display(const RustResult<RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>>, RustError> *r);

RustOwnedString rust_result_entries_err_debug(const RustResult<RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>>, RustError> *r);

const RustClient *rust_result_client_ok(const RustResult<RustClient, RustError> *r);

bool rust_result_client_is_err(const RustResult<RustClient, RustError> *r);

RustOwnedString rust_result_client_err_display(const RustResult<RustClient, RustError> *r);

RustOwnedString rust_result_client_err_debug(const RustResult<RustClient, RustError> *r);

const RustTreeChildEntry *rust_result_treechildentry_ok(const RustResult<RustTreeChildEntry, RustEdenApiServerError> *r);

bool rust_result_treechildentry_is_err(const RustResult<RustTreeChildEntry, RustEdenApiServerError> *r);

RustOwnedString rust_result_treechildentry_err_display(const RustResult<RustTreeChildEntry, RustEdenApiServerError> *r);

RustOwnedString rust_result_treechildentry_err_debug(const RustResult<RustTreeChildEntry, RustEdenApiServerError> *r);

size_t rust_vec_treeentry_len(const RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>> *v);

const RustResult<RustTreeEntry, RustEdenApiServerError> *rust_vec_treeentry_get(const RustVec<RustResult<RustTreeEntry, RustEdenApiServerError>> *v,
                                                                                size_t idx);

size_t rust_vec_treechild_len(const RustVec<RustResult<RustTreeChildEntry, RustEdenApiServerError>> *v);

const RustResult<RustTreeChildEntry, RustEdenApiServerError> *rust_vec_treechild_get(const RustVec<RustResult<RustTreeChildEntry, RustEdenApiServerError>> *v,
                                                                                     size_t idx);

} // extern "C"
