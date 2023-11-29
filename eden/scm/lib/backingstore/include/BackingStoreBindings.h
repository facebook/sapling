/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 *
 * This file is generated with cbindgen. Please run `./tools/cbindgen.sh` to
 * update this file.
 *
 * @generated SignedSource<<333733f8531f0405236cb8590401bcd8>>
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

extern "C" {

void sapling_cbytes_free(CBytes *vec);

void sapling_cfallible_free_error(char *ptr);

} // extern "C"

} // namespace sapling


namespace sapling {

/// The monomorphized version of `CFallible` used solely because MSVC
/// does not allow returning template functions from extern "C" functions.
struct CFallibleBase {
  void *value;
  char *error;
};

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
