/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 *
 * This file is generated with cbindgen. Please run `./tools/cbindgen.sh` to
 * update this file.
 *
 * @generated SignedSource<<acd2733514d3f5e17ff03431ac01b0a3>>
 *
 */

// The generated functions are exported from this Rust library
// @dep=//eden/scm/lib/backingstore:backingstore


#pragma once

#include <stdint.h>
#include <memory>
#include <string_view>
#include <folly/Range.h>

namespace sapling {

struct BackingStore;

struct FileAuxData;

struct Tree;

template<typename T = void>
struct Vec;

struct CBytes {
  uint8_t *ptr;
  size_t len;
  Vec<uint8_t> *vec;
  folly::ByteRange asByteRange() const {
    return folly::ByteRange(ptr, len);
  }

  operator folly::ByteRange() const {
    return asByteRange();
  }
};

struct Request {
  const uint8_t *node;
};

template<typename T>
struct Slice {
  const T *ptr;
  size_t len;
  template <typename Q = T>
  Slice(std::enable_if_t<std::is_same_v<Q, uint8_t>, std::string_view> sv) noexcept
    : ptr{reinterpret_cast<const uint8_t*>(sv.data())}, len{sv.size()} {}

  Slice(folly::Range<const T*> range) noexcept
    : ptr{range.data()}, len{range.size()} {}
};

/// The monomorphized version of `CFallible` used solely because MSVC
/// does not allow returning template functions from extern "C" functions.
struct CFallibleBase {
  void *value;
  char *error;
};

extern "C" {

void sapling_file_aux_free(FileAuxData *aux);

void sapling_cbytes_free(CBytes *vec);

void sapling_cfallible_free_error(char *ptr);

void sapling_backingstore_get_tree_batch(BackingStore *store,
                                         Slice<Request> requests,
                                         bool local,
                                         void *data,
                                         void (*resolve)(void*, uintptr_t, CFallibleBase));

void sapling_backingstore_get_blob_batch(BackingStore *store,
                                         Slice<Request> requests,
                                         bool local,
                                         void *data,
                                         void (*resolve)(void*, uintptr_t, CFallibleBase));

void sapling_backingstore_get_file_aux_batch(BackingStore *store,
                                             Slice<Request> requests,
                                             bool local,
                                             void *data,
                                             void (*resolve)(void*, uintptr_t, CFallibleBase));

/// Returns a `CFallible` with success return value 1. This function is intended to be called from
/// C++ tests.
CFallibleBase sapling_test_cfallible_ok();

void sapling_test_cfallible_ok_free(uint8_t *val);

/// Returns a `CFallible` with error message "context: failure!". This function is intended to be called
/// from C++ tests.
CFallibleBase sapling_test_cfallible_err();

CBytes sapling_test_cbytes();

void sapling_tree_free(Tree *tree);

} // extern "C"

} // namespace sapling


namespace sapling {

// Some Rust functions will have the return type `CFallibleBase`, and we
// have this convenient struct to help C++ code to consume the returned
// struct. This is the only way to use the returned `CFallibleBase` from
// Rust, and the user must provide a `Deleter` to correctly free the pointer
// returned from Rust.
template <typename T, void(*dtor)(T*)>
class CFallible {
public:
  struct Deleter {
    void operator()(T* ptr) {
      dtor(ptr);
    }
  };
  using Ptr = std::unique_ptr<T, Deleter>;

  explicit CFallible(CFallibleBase&& base)
    : ptr_{reinterpret_cast<T*>(base.value)}, error_{base.error} {}

  ~CFallible() {
    if (error_) {
      sapling_cfallible_free_error(error_);
    }
  }

  bool isError() const {
    return error_ != nullptr;
  }

  char* getError() {
    return error_;
  }

  T* get() {
    return ptr_.get();
  }

  Ptr unwrap() {
    return std::move(ptr_);
  }

private:
  Ptr ptr_;
  char* error_;
};

}
