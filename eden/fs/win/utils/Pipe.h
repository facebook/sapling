/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <folly/portability/IOVec.h>
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

  size_t read(void* buffer, DWORD BytesToRead);
  size_t write(void* buffer, DWORD BytesToWrite);

  static size_t read(HANDLE handle, void* buffer, DWORD BytesToRead);

  static size_t write(HANDLE handle, void* buffer, DWORD BytesToWrite);
  static size_t writeiov(HANDLE handle, iovec* iov, int count);

 private:
  HANDLE readHandle_ = nullptr;
  HANDLE writeHandle_ = nullptr;
};
} // namespace eden
} // namespace facebook
