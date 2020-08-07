/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "Pipe.h"
#include <folly/portability/IOVec.h>
#include <folly/portability/Windows.h>
#include <stdio.h>
#include <strsafe.h>
#include <iostream>
#include <memory>
#include <vector>
#include "eden/fs/win/utils/WinError.h"
#include "folly/logging/xlog.h"

namespace facebook {
namespace eden {

Pipe::Pipe() {
  auto sec = SECURITY_ATTRIBUTES();
  sec.nLength = sizeof(sec);
  sec.bInheritHandle = false;
  sec.lpSecurityDescriptor = nullptr;

  if (!CreatePipe(&readHandle_, &writeHandle_, &sec, 0)) {
    throw makeWin32ErrorExplicit(GetLastError(), "Failed to create a pipe");
  }
}

Pipe::~Pipe() {
  if (readHandle_) {
    CloseHandle(readHandle_);
  }
  if (writeHandle_) {
    CloseHandle(writeHandle_);
  }
}

ssize_t Pipe::read(HANDLE handle, void* buffer, size_t nbytes) {
  ssize_t bytesRead = 0;

  while (nbytes > 0) {
    DWORD read = 0;
    if (!ReadFile(
            handle,
            ((char*)buffer + bytesRead),
            folly::to_narrow(nbytes),
            &read,
            nullptr)) {
      return -1;
    }
    bytesRead += read;
    nbytes -= read;
  }

  return bytesRead;
}

ssize_t Pipe::write(HANDLE handle, void* buffer, size_t nbytes) {
  ssize_t bytesWritten = 0;

  while (nbytes > 0) {
    DWORD written = 0;
    if (!WriteFile(
            handle,
            (void*)((char*)buffer + bytesWritten),
            folly::to_narrow(nbytes),
            &written,
            nullptr)) {
      return -1;
    }
    bytesWritten += written;
    nbytes -= written;
  }

  return bytesWritten;
}

ssize_t Pipe::writevFull(HANDLE handle, iovec* iov, int count) {
  ssize_t bytesWritten = 0;

  for (int i = 0; i < count; i++) {
    auto written = write(handle, iov[i].iov_base, iov[i].iov_len);
    if (written < 0) {
      return written;
    }
    bytesWritten += written;
  }

  return bytesWritten;
}

ssize_t Pipe::read(void* buffer, size_t nbytes) {
  return read(readHandle_, buffer, nbytes);
}

ssize_t Pipe::write(void* buffer, size_t nbytes) {
  return write(writeHandle_, buffer, nbytes);
}

} // namespace eden
} // namespace facebook
