/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 *
 * This file is generated with cbindgen. Please run `./tools/cbindgen.sh` to
 * update this file.
 *
 * @generated SignedSource<<85582de8715286965cfc5e5472a8dab0>>
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
#include <new>

enum class RustTreeEntryType : uint8_t {
  Tree,
  RegularFile,
  ExecutableFile,
  Symlink,
};

struct RustBackingStore;

template<typename T>
struct RustVec;

struct RustCBytes {
  const uint8_t *ptr;
  size_t len;
  RustVec<uint8_t> *vec;
folly::ByteRange asByteRange() const {
  return folly::ByteRange(ptr, len);
}

operator folly::ByteRange() const {
  return asByteRange();
}
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
  RustVec<RustTreeEntry> *_entries;
  uintptr_t length;
  RustCBytes hash;
};

extern "C" {

void rust_backingstore_free(RustBackingStore *store);

RustCFallibleBase rust_backingstore_get_blob(RustBackingStore *store,
                                                         const uint8_t *name,
                                                         uintptr_t name_len,
                                                         const uint8_t *node,
                                                         uintptr_t node_len);

RustCFallibleBase rust_backingstore_get_tree(RustBackingStore *store,
                                                       const uint8_t *node,
                                                       uintptr_t node_len);

RustCFallibleBase rust_backingstore_new(const char *repository,
                                                          size_t repository_len);

void rust_cbytes_free(RustCBytes *vec);

void rust_cfallible_free_error(char *ptr);

RustCBytes rust_test_cbytes();

/// Returns a `CFallible` with error message "failure!". This function is intended to be called
/// from C++ tests.
RustCFallibleBase rust_test_cfallible_err();

/// Returns a `CFallible` with success return value 1. This function is intended to be called from
/// C++ tests.
RustCFallibleBase rust_test_cfallible_ok();

void rust_test_cfallible_ok_free(uint8_t *val);

void rust_tree_free(RustTree *tree);

} // extern "C"
