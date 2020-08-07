/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/portability/IOVec.h>
#include <folly/portability/SysTypes.h>
#include <folly/portability/Windows.h>

namespace facebook {
namespace eden {

class Pipe {
 public:
  Pipe();
  ~Pipe();
  HANDLE readHandle() {
    return readHandle_;
  }

  HANDLE writeHandle() {
    return writeHandle_;
  }

  void closeReadHandle() {
    if (readHandle_) {
      CloseHandle(readHandle_);
      readHandle_ = nullptr;
    }
  }

  void closeWriteHandle() {
    if (writeHandle_) {
      CloseHandle(writeHandle_);
      writeHandle_ = nullptr;
    }
  }

  //
  // Following read/write pipe functions return the number of bytes read or <0
  // on error
  //

  ssize_t read(void* buffer, size_t nbytes);
  ssize_t write(void* buffer, size_t nbytes);

  static ssize_t read(HANDLE handle, void* buffer, size_t nbytes);

  static ssize_t write(HANDLE handle, void* buffer, size_t nbytes);
  static ssize_t writevFull(HANDLE handle, iovec* iov, int count);

 private:
  HANDLE readHandle_ = nullptr;
  HANDLE writeHandle_ = nullptr;
};
} // namespace eden
} // namespace facebook
