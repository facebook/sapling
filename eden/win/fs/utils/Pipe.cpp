/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */

#include "Pipe.h"
#include <folly/portability/IOVec.h>
#include <stdio.h>
#include <strsafe.h>
#include <iostream>
#include <memory>
#include <vector>
#include "folly/logging/xlog.h"

namespace facebook {
namespace edenwin {

// Pipe constructor will either use security attr or the inherit flag.
// If the security attribute is nullptr it will create one and will use the
// inherit flag for it.
Pipe::Pipe(PSECURITY_ATTRIBUTES securityAttr, bool inherit) {
  auto sec = SECURITY_ATTRIBUTES();
  if (securityAttr == nullptr) {
    sec.nLength = sizeof(sec);
    sec.bInheritHandle = inherit;
    sec.lpSecurityDescriptor = nullptr;
    securityAttr = &sec;
  }

  if (!CreatePipe(&readHandle, &writeHandle, securityAttr, NULL)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "CreatePipe failed\n");
  }
  XLOG(DBG5) << "Handle Created: Read: " << readHandle
             << " Write: " << writeHandle << std::endl;
}

Pipe::~Pipe() {
  if (readHandle) {
    CloseHandle(readHandle);
  }
  if (writeHandle) {
    CloseHandle(writeHandle);
  }
}

void Pipe::read(void* buffer, DWORD bytesToRead, LPDWORD bytesRead) {
  DWORD localBytesRead;
  if (!ReadFile(
          readHandle,
          buffer,
          bytesToRead,
          bytesRead ? bytesRead : &localBytesRead,
          nullptr)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "ReadFile failed");
  }
}

void Pipe::write(void* buffer, DWORD bytesToWrite, LPDWORD bytesWritten) {
  DWORD localBytesWritten;
  if (!WriteFile(
          writeHandle,
          buffer,
          bytesToWrite,
          bytesWritten ? bytesWritten : &localBytesWritten,
          nullptr)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "WriteFile failed");
  }
}

void Pipe::read(
    HANDLE handle,
    void* buffer,
    DWORD bytesToRead,
    LPDWORD bytesRead) {
  DWORD localBytesRead;
  if (!ReadFile(handle, buffer, bytesToRead, &localBytesRead, nullptr)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "ReadFile failed");
  }
  if (bytesRead) {
    *bytesRead = localBytesRead;
  }

  XLOG(DBG5) << "Pipe::read-- bytesToRead:" << bytesToRead << "bytesRead"
             << localBytesRead << std::endl;
}

size_t Pipe::writeiov(HANDLE handle, iovec* iov, int count) {
  DWORD localBytesWritten;

  for (int i = 0; i < count; i++) {
    if (!WriteFile(
            handle,
            iov[i].iov_base,
            iov[i].iov_len,
            &localBytesWritten,
            nullptr)) {
      throw std::system_error(
          GetLastError(), std::system_category(), "WriteFile failed");
    }
  }

  // TODO: localBytesWritten -  it should be sum of all the write ops
  return localBytesWritten;
}

void Pipe::write(
    HANDLE handle,
    void* buffer,
    DWORD bytesToWrite,
    LPDWORD bytesWritten) {
  DWORD localBytesWritten;
  if (!WriteFile(handle, buffer, bytesToWrite, &localBytesWritten, nullptr)) {
    throw std::system_error(
        GetLastError(), std::system_category(), "WriteFile failed");
  }

  FlushFileBuffers(handle);

  if (bytesWritten) {
    *bytesWritten = localBytesWritten;
  }

  XLOG(DBG5) << "Pipe::write-- bytesToWrite" << bytesToWrite << "bytesWritten"
             << localBytesWritten << std::endl;
}

} // namespace edenwin
} // namespace facebook
