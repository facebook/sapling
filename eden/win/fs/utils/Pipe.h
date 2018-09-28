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
#include <folly/portability/IOVec.h>
#include <folly/portability/Windows.h>

namespace facebook {
namespace edenwin {

class Pipe {
 public:
  HANDLE readHandle = nullptr;
  HANDLE writeHandle = nullptr;

  Pipe(PSECURITY_ATTRIBUTES securityAttr, bool inherit);
  ~Pipe();

  void read(void* buffer, DWORD BytesToRead, LPDWORD BytesRead = nullptr);
  void write(void* buffer, DWORD BytesToWrite, LPDWORD BytesWritten = nullptr);

  static void read(
      HANDLE handle,
      void* buffer,
      DWORD BytesToRead,
      LPDWORD BytesRead = nullptr);
  static void write(
      HANDLE handle,
      void* buffer,
      DWORD BytesToWrite,
      LPDWORD BytesWritten = nullptr);
  static size_t writeiov(HANDLE handle, iovec* iov, int count);
};
} // namespace edenwin
} // namespace facebook
