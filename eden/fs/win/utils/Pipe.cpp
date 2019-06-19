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
    throw makeWin32ErrorExplicit(GetLastError(), "Failed to create a pipe");
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

size_t Pipe::read(HANDLE handle, void* buffer, DWORD bytesToRead) {
  size_t bytesRead = 0;
  DWORD read = 0;
  DWORD remainingBytes = bytesToRead;

  while (remainingBytes > 0) {
    if (!ReadFile(
            handle,
            ((char*)buffer + bytesRead),
            remainingBytes,
            &read,
            nullptr)) {
      DWORD error = GetLastError();
      XLOGF(
          ERR,
          "Error while reading from the pipe : bytesRead {}, Error: {} : {}",
          bytesRead,
          error,
          win32ErrorToString(error));

      throw makeWin32ErrorExplicit(error, "Error while reading from the pipe");
    }
    bytesRead += read;
    remainingBytes -= read;
  }
  XLOG(DBG5) << "Pipe::read-- bytesToRead:" << bytesToRead << "bytesRead"
             << bytesRead << std::endl;

  return bytesRead;
}
size_t Pipe::write(HANDLE handle, void* buffer, DWORD bytesToWrite) {
  size_t bytesWritten = 0;
  DWORD written = 0;
  DWORD remainingBytes = bytesToWrite;

  while (remainingBytes > 0) {
    if (!WriteFile(
            handle,
            (void*)((char*)buffer + bytesWritten),
            remainingBytes,
            &written,
            nullptr)) {
      DWORD error = GetLastError();
      XLOGF(
          ERR,
          "Error while writing to the pipe : bytesWritten {}, {} : {}",
          bytesWritten,
          error,
          win32ErrorToString(error));

      throw makeWin32ErrorExplicit(error, "Error while writing to the pipe");
    }
    bytesWritten += written;
    remainingBytes -= written;
  }
  XLOG(DBG5) << "Pipe::Write-- bytesToWrite:" << bytesToWrite << "bytesWritten"
             << bytesWritten << std::endl;

  return bytesWritten;
}

size_t Pipe::writeiov(HANDLE handle, iovec* iov, int count) {
  size_t bytesWritten = 0;
  DWORD written = 0;

  for (int i = 0; i < count; i++) {
    written = write(handle, iov[i].iov_base, iov[i].iov_len);
    bytesWritten += written;
  }

  return bytesWritten;
}

size_t Pipe::read(void* buffer, DWORD bytesToRead) {
  return read(readHandle, buffer, bytesToRead);
}

size_t Pipe::write(void* buffer, DWORD bytesToWrite) {
  return write(writeHandle, buffer, bytesToWrite);
}

} // namespace eden
} // namespace facebook
