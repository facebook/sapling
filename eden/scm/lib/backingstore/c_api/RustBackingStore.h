/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 *
 * This file is generated with cbindgen. Please run `./tools/cbindgen.sh` to
 * update this file.
 *
 * @generated SignedSource<<a391da59ccb8938a2cd04ed52e7f5689>>
 *
 */

// The generated functions are exported from this Rust library
// @dep=:backingstore

#pragma once

#include <memory>
#include <functional>
#include <folly/Range.h>

extern "C" void rust_cfallible_free_error(char *ptr);

// MSVC toolchain dislikes having template in `extern "C"` functions. So we will
// have to use void pointer here. Cbindgen does not support generating code like
// this since it's kinda a special case so we manually generate this struct.
struct RustCFallibleBase {
 void *value;
 char *error;
};

// Some Rust functions will have the return type `RustCFallibleBase`, and we
// have this convenient struct to help C++ code to consume the returned
// struct. This is the only way to use the returned `RustCFallibleBase` from
// Rust, and the user must provide a `Deleter` to correctly free the pointer
// returned from Rust.
template <typename T, typename Deleter = std::function<void(T*)>>
class RustCFallible {
private:
  std::unique_ptr<T, std::function<void(T*)>> ptr_;
  char* error_;

public:
  RustCFallible(RustCFallibleBase&& base, Deleter deleter)
      : ptr_(reinterpret_cast<T*>(base.value), deleter), error_(base.error) {}

  bool isError() const {
    return error_ != nullptr;
  }

  char* getError() {
    return error_;
  }

  T* get() {
    return ptr_.get();
  }

  std::unique_ptr<T, Deleter> unwrap() {
    return std::move(ptr_);
  }

  ~RustCFallible() {
    if (error_ != nullptr) {
      rust_cfallible_free_error(error_);
    }

    unwrap();
  }
};


#include <cstdarg>
#include <cstdint>
#include <cstdlib>
#include <ostream>
#include <new>

enum class RustTreeEntryType : uint8_t {
  Tree,
  RegularFile,
  ExecutableFile,
  Symlink,
};

struct RustBackingStore;

template<typename T = void>
struct RustVec;

struct RustCBytes {
  uint8_t *ptr;
  size_t len;
  RustVec<uint8_t> *vec;
folly::ByteRange asByteRange() const {
  return folly::ByteRange(ptr, len);
}

operator folly::ByteRange() const {
  return asByteRange();
}
};

struct RustRequest {
  const uint8_t *path;
  uintptr_t length;
  const uint8_t *node;
};

struct RustTreeEntry {
  RustCBytes hash;
  RustCBytes name;
  RustTreeEntryType ttype;
  uint64_t *size;
  RustCBytes *content_sha1;
};

struct RustTree {
  const RustTreeEntry *entries;
  /// This makes sure `entries` above is pointing to a valid memory.
  RustVec<RustTreeEntry> *entries_ptr;
  uintptr_t length;
  RustCBytes hash;
};

struct RustFileAuxData {
  uint64_t total_size;
  RustCBytes content_id;
  RustCBytes content_sha1;
  RustCBytes content_sha256;
};

extern "C" {

RustCFallibleBase rust_backingstore_new(const char *repository,
                                                          size_t repository_len,
                                                          bool aux_data,
                                                          bool allow_retries);

void rust_backingstore_free(RustBackingStore *store);

RustCFallibleBase rust_backingstore_get_blob(RustBackingStore *store,
                                                         const uint8_t *name,
                                                         uintptr_t name_len,
                                                         const uint8_t *node,
                                                         uintptr_t node_len,
                                                         bool local);

void rust_backingstore_get_blob_batch(RustBackingStore *store,
                                      const RustRequest *requests,
                                      uintptr_t size,
                                      bool local,
                                      void *data,
                                      void (*resolve)(void*, uintptr_t, RustCFallibleBase));

RustCFallibleBase rust_backingstore_get_tree(RustBackingStore *store,
                                                       const uint8_t *node,
                                                       uintptr_t node_len,
                                                       bool local);

void rust_backingstore_get_tree_batch(RustBackingStore *store,
                                      const RustRequest *requests,
                                      uintptr_t size,
                                      bool local,
                                      void *data,
                                      void (*resolve)(void*, uintptr_t, RustCFallibleBase));

RustCFallibleBase rust_backingstore_get_file_aux(RustBackingStore *store,
                                                                  const uint8_t *node,
                                                                  uintptr_t node_len,
                                                                  bool local);

void rust_backingstore_get_file_aux_batch(RustBackingStore *store,
                                          const RustRequest *requests,
                                          uintptr_t size,
                                          bool local,
                                          void *data,
                                          void (*resolve)(void*, uintptr_t, RustCFallibleBase));

void rust_tree_free(RustTree *tree);

void rust_file_aux_free(RustFileAuxData *aux);

void rust_backingstore_flush(RustBackingStore *store);

void rust_cbytes_free(RustCBytes *vec);

void rust_cfallible_free_error(char *ptr);

/// Returns a `CFallible` with success return value 1. This function is intended to be called from
/// C++ tests.
RustCFallibleBase rust_test_cfallible_ok();

void rust_test_cfallible_ok_free(uint8_t *val);

/// Returns a `CFallible` with error message "context: failure!". This function is intended to be called
/// from C++ tests.
RustCFallibleBase rust_test_cfallible_err();

RustCBytes rust_test_cbytes();

} // extern "C"
