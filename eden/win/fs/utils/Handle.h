/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#pragma once
#include "folly/logging/xlog.h"
#include "folly/portability/Windows.h"
/*
 * This is a generic base class to create a handle classes. The handle class
 * with make sure that handle is closed when it goes out of scope. To create a
 * new handle class define the handle traits with handle type, invalid value to
 * check if the handle is valid or not, plus an API to close it. The following
 * example handle traits class can be used for Win32 file handle:
 *
 * struct FileHandleTraits {
 * using Type = HANDLE;
 * static Type invalidHandleValue() noexcept {
 *   return INVALID_HANDLE_VALUE;
 * }
 * static void close(Type handle) noexcept {
 *   CloseHandle(handle);
 * }
 *};
 *
 * using FileHandle = HandleBase<FileHandleTraits>;
 *
 * The handle can be captured by the constructor if the are returned by an API.
 * For ex: FileHandle handle { apiThatReturnsTheHandle()};
 *
 * If the handle is returned by a function argument then we could use the set
 * API for it. Ex:
 * FileHandle handle;
 * apiThatReturnsTheHandleAsArgs(handle.set());
 *
 * When the handle goes out of scope it will call the traits close function from
 * the dtor to close the handle.
 *
 * This class has few more helper functions like:
 * reset() to reset the handle value to a new or invalid handle.
 * release() to close the existing handle before it goes out of scope.
 * a bool operator to check if the handle is valid.
 */
template <typename Traits>
class HandleBase {
 public:
  using Type = typename Traits::Type;

  explicit HandleBase(Type handle = Traits::invalidHandleValue()) noexcept
      : handle_(handle) {}

  // Forbidden copy constructor and assignment operator
  HandleBase(const HandleBase&) = delete;
  HandleBase& operator=(const HandleBase&) = delete;

  HandleBase(HandleBase&& other) noexcept : handle_(other.release()) {}
  HandleBase& operator=(HandleBase&& other) noexcept {
    if (this != &other) {
      reset(other.release());
    }

    return *this;
  }

  ~HandleBase() noexcept {
    close();
  }

  explicit operator bool() const noexcept {
    return (handle_ != Traits::invalidHandleValue());
  }

  Type get() const noexcept {
    return handle_;
  }

  Type* set() noexcept {
    DCHECK(handle_ == Traits::invalidHandleValue());
    return &handle_;
  }

  Type release() noexcept {
    Type handle = handle_;
    handle_ = Traits::invalidHandleValue();
    return handle;
  }

  void reset(Type value = Traits::invalidHandleValue()) noexcept {
    if ((handle_ != Traits::invalidHandleValue()) && (handle_ == value)) {
      XLOG(DFATAL) << "Trying to reset to the same handle - check if there are"
                      "multiple owners of the handle";
    }
    close();
    handle_ = value;
  }

 private:
  Type handle_;

  void close() noexcept {
    if (*this) {
      Traits::close(handle_);
    }
  }
};
