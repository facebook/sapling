/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/utils/FileDescriptor.h"
#include <fcntl.h>
#include <folly/FileUtil.h>
#include <folly/String.h>
#include <folly/portability/SysUio.h>
#ifndef _WIN32
#include <folly/portability/Unistd.h>
#endif
#include <system_error>

using folly::make_exception_wrapper;
using folly::Try;

namespace facebook::eden {

FileDescriptor::~FileDescriptor() {
  close();
}

FileDescriptor::system_handle_type FileDescriptor::normalizeHandleValue(
    system_handle_type h) {
  if (folly::kIsWindows) {
    // Windows uses both 0 and INVALID_HANDLE_VALUE as invalid handle values.
    if (h == FileDescriptor::kInvalid || h == 0) {
      return FileDescriptor::kInvalid;
    }
  } else {
    // Posix defines -1 to be an invalid value, but we'll also recognize and
    // normalize any negative descriptor value.
    if (h < 0) {
      return FileDescriptor::kInvalid;
    }
  }
  return h;
}

FileDescriptor::FileDescriptor(
    FileDescriptor::system_handle_type fd,
    FDType fdType)
    : fd_(normalizeHandleValue(fd)), fdType_(resolveFDType(fd, fdType)) {}

FileDescriptor::FileDescriptor(
    FileDescriptor::system_handle_type fd,
    const char* operation,
    FDType fdType)
    : fd_(normalizeHandleValue(fd)), fdType_(resolveFDType(fd, fdType)) {
  if (fd_ == kInvalid) {
    throw std::system_error(
        errno,
        std::generic_category(),
        std::string(operation) + ": " + folly::errnoStr(errno));
  }
}

FileDescriptor::FileDescriptor(FileDescriptor&& other) noexcept
    : fd_(other.release()), fdType_(other.fdType_) {}

FileDescriptor& FileDescriptor::operator=(FileDescriptor&& other) noexcept {
  close();
  fd_ = other.fd_;
  fdType_ = other.fdType_;
  other.fd_ = kInvalid;
  return *this;
}

void FileDescriptor::close() {
  if (fd_ != kInvalid) {
#ifndef _WIN32
    folly::closeNoInt(fd_);
#else
    if (fdType_ == FDType::Socket) {
      ::closesocket(fd_);
    } else {
      CloseHandle((HANDLE)fd_);
    }
#endif
    fd_ = kInvalid;
  }
}

FileDescriptor FileDescriptor::duplicate() const {
#ifndef _WIN32
  return FileDescriptor(::dup(fd_), "FileDescriptor::duplicate", fdType_);
#else
  HANDLE newHandle = INVALID_HANDLE_VALUE;
  auto proc = GetCurrentProcess();
  if (DuplicateHandle(
          proc,
          (HANDLE)fd_,
          proc,
          &newHandle,
          0, // dwDesiredAccess
          FALSE, // bInheritHandle
          DUPLICATE_SAME_ACCESS)) {
    return FileDescriptor(reinterpret_cast<intptr_t>(newHandle), fdType_);
  }
  throw std::system_error(
      GetLastError(), std::system_category(), "FileDescriptor::duplicate");
#endif
}

FileDescriptor::system_handle_type FileDescriptor::release() {
  system_handle_type result = fd_;
  fd_ = kInvalid;
  return result;
}

FileDescriptor::FDType FileDescriptor::resolveFDType(
    FileDescriptor::system_handle_type fd,
    FDType fdType) {
  if (normalizeHandleValue(fd) == kInvalid) {
    return FDType::Unknown;
  }

  if (fdType != FDType::Unknown) {
    return fdType;
  }

#ifdef _WIN32
  if (GetFileType((HANDLE)fd) == FILE_TYPE_PIPE) {
    // It may be a pipe or a socket.
    // We can decide by asking for the underlying pipe
    // information; anonymous pipes are implemented on
    // top of named pipes so it is fine to use this function:
    DWORD flags = 0;
    DWORD out = 0;
    DWORD in = 0;
    DWORD inst = 0;
    if (GetNamedPipeInfo((HANDLE)fd, &flags, &out, &in, &inst) != 0) {
      return FDType::Pipe;
    }

    // We believe it to be a socket managed by winsock because it wasn't
    // a pipe.  However, when using pipes between WSL and native win32
    // we get here and the handle isn't recognized by winsock either.
    // Let's ask it for the error associated with the handle; if winsock
    // disavows it then we know it isn't a pipe or a socket, but we don't
    // know precisely what it is.
    int err = 0;
    int errsize = sizeof(err);
    if (::getsockopt(
            fd,
            SOL_SOCKET,
            SO_ERROR,
            reinterpret_cast<char*>(&err),
            &errsize) &&
        WSAGetLastError() == WSAENOTSOCK) {
      return FDType::Generic;
    }

    return FDType::Socket;
  }
#endif
  return FDType::Generic;
}

void FileDescriptor::setCloExec() {
#ifndef _WIN32
  (void)fcntl(fd_, F_SETFD, FD_CLOEXEC);
#endif
}

void FileDescriptor::clearCloExec() {
#ifndef _WIN32
  (void)fcntl(fd_, F_SETFD, fcntl(fd_, F_GETFD) & ~FD_CLOEXEC);
#endif
}

void FileDescriptor::setNonBlock() {
#ifndef _WIN32
  (void)fcntl(fd_, F_SETFL, fcntl(fd_, F_GETFL) | O_NONBLOCK);
#else
  if (fdType_ == FDType::Socket) {
    u_long mode = 1;
    (void)::ioctlsocket(fd_, FIONBIO, &mode);
  }
#endif
}

void FileDescriptor::clearNonBlock() {
#ifndef _WIN32
  (void)fcntl(fd_, F_SETFL, fcntl(fd_, F_GETFL) & ~O_NONBLOCK);
#else
  if (fdType_ == FDType::Socket) {
    u_long mode = 0;
    (void)::ioctlsocket(fd_, FIONBIO, &mode);
  }
#endif
}

folly::Try<ssize_t> FileDescriptor::readFull(void* buf, int size) const {
  return wrapFull(buf, size, /*isRead=*/true, /*onlyOnce=*/false);
}

folly::Try<ssize_t> FileDescriptor::readNoInt(void* buf, int size) const {
  return wrapFull(buf, size, /*isRead=*/true, /*onlyOnce=*/true);
}

folly::Try<ssize_t> FileDescriptor::writeFull(const void* buf, int size) const {
  return wrapFull(
      const_cast<void*>(buf), size, /*isRead=*/false, /*onlyOnce=*/false);
}

folly::Try<ssize_t> FileDescriptor::writeNoInt(const void* buf, int size)
    const {
  return wrapFull(
      const_cast<void*>(buf), size, /*isRead=*/false, /*onlyOnce=*/true);
}

folly::Try<ssize_t> FileDescriptor::readvFull(struct iovec* iov, size_t numIov)
    const {
  return wrapvFull(iov, numIov, true);
}

folly::Try<ssize_t> FileDescriptor::writevFull(struct iovec* iov, size_t numIov)
    const {
  return wrapvFull(iov, numIov, false);
}

Try<ssize_t> FileDescriptor::read(void* buf, int size) const {
#ifndef _WIN32
  auto result = ::read(fd_, buf, size);
  if (result == -1) {
    int errcode = errno;
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(errcode, std::generic_category()), "read"));
  }
  return Try<ssize_t>(result);
#else
  if (fdType_ == FDType::Socket) {
    auto result = ::recv(fd_, static_cast<char*>(buf), size, 0);
    if (result == -1) {
      int errcode = WSAGetLastError();
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(errcode, std::system_category()), "recv"));
    }
    return Try<ssize_t>(result);
  }

  DWORD result = 0;
  if (!ReadFile((HANDLE)fd_, buf, size, &result, nullptr)) {
    auto err = GetLastError();
    if (err == ERROR_BROKEN_PIPE) {
      // Translate broken pipe on read to EOF
      result = 0;
    } else {
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(err, std::system_category()), "ReadFile"));
    }
  }
  return Try<ssize_t>(result);
#endif
}

#ifdef _WIN32
namespace {
// WSABUF is logically equivalent to iovec but has the pointer
// and size members in the opposite order.
// This little helper translates from iovec to WSABUF so that we
// can more easily pass through to the winsock functions.
std::vector<WSABUF> iovecToWsaBuf(struct iovec* iov, size_t numIov) {
  std::vector<WSABUF> bufs;
  bufs.reserve(numIov);
  for (size_t i = 0; i < numIov; ++i) {
    WSABUF buf{};
    buf.len = static_cast<ULONG>(iov[i].iov_len);
    buf.buf = reinterpret_cast<CHAR*>(iov[i].iov_base);
    bufs.push_back(buf);
  }
  return bufs;
}
} // namespace
#endif

Try<ssize_t> FileDescriptor::readv(struct iovec* iov, size_t numIov) const {
#ifndef _WIN32
  auto result = ::readv(fd_, iov, numIov);
  if (result == -1) {
    int errcode = errno;
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(errcode, std::generic_category()), "readv"));
  }
  return Try<ssize_t>(result);
#else
  if (fdType_ == FDType::Socket) {
    DWORD len = 0;

    auto bufs = iovecToWsaBuf(iov, numIov);

    if (WSARecv(fd_, bufs.data(), (DWORD)bufs.size(), &len, 0, NULL, NULL) ==
        SOCKET_ERROR) {
      int errcode = WSAGetLastError();
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(errcode, std::system_category()), "WSARecv"));
    }
    return Try<ssize_t>(len);
  }

  return doVecOp(iov, numIov, true);
#endif
}

Try<ssize_t> FileDescriptor::write(const void* buf, int size) const {
#ifndef _WIN32
  auto result = ::write(fd_, buf, size);
  if (result == -1) {
    int errcode = errno;
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(errcode, std::generic_category()), "write"));
  }
  return Try<ssize_t>(result);
#else
  if (fdType_ == FDType::Socket) {
    auto result = ::send(fd_, static_cast<const char*>(buf), size, 0);
    if (result == -1) {
      int errcode = WSAGetLastError();
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(errcode, std::system_category()), "send"));
    }
    return Try<ssize_t>(result);
  }
  DWORD result = 0;
  if (!WriteFile((HANDLE)fd_, buf, size, &result, nullptr)) {
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(GetLastError(), std::system_category()), "WriteFile"));
  }
  return Try<ssize_t>(result);
#endif
}

folly::Try<ssize_t> FileDescriptor::writev(struct iovec* iov, size_t numIov)
    const {
#ifndef _WIN32
  auto result = ::writev(fd_, iov, numIov);
  if (result == -1) {
    int errcode = errno;
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(errcode, std::generic_category())));
  }
  return Try<ssize_t>(result);
#else
  if (fdType_ == FDType::Socket) {
    DWORD len = 0;

    auto bufs = iovecToWsaBuf(iov, numIov);

    if (WSASend(fd_, bufs.data(), (DWORD)bufs.size(), &len, 0, NULL, NULL) ==
        SOCKET_ERROR) {
      int errcode = WSAGetLastError();
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(errcode, std::system_category()), "WSASend"));
    }
    return Try<ssize_t>(len);
  }

  return doVecOp(iov, numIov, false);
#endif
}

folly::Try<ssize_t> FileDescriptor::wrapFull(
    void* buf,
    ssize_t count,
    bool isRead,
    bool onlyOnce) const {
  char* b = static_cast<char*>(buf);
  ssize_t totalBytes = 0;
  do {
    Try<ssize_t> opResult = isRead ? read(b, count) : write(b, count);

    if (auto ex = opResult.tryGetExceptionObject<std::system_error>()) {
      if (ex->code() == std::error_code(EINTR, std::generic_category())) {
        continue;
      }
    }
    if (opResult.hasException()) {
      return opResult;
    }

    auto r = opResult.value();
    if (isRead && r == 0) {
      // EOF
      break;
    }

    totalBytes += r;
    b += r;
    count -= r;

    if (onlyOnce) {
      break;
    }
  } while (count);

  return Try<ssize_t>(totalBytes);
}

folly::Try<ssize_t>
FileDescriptor::wrapvFull(struct iovec* iov, size_t count, bool isRead) const {
  ssize_t totalBytes = 0;
  ssize_t r;
  do {
    Try<ssize_t> opResult = isRead
        ? readv(iov, std::min<size_t>(count, folly::kIovMax))
        : writev(iov, std::min<size_t>(count, folly::kIovMax));

    if (auto ex = opResult.tryGetExceptionObject<std::system_error>()) {
      if (ex->code() == std::error_code(EINTR, std::generic_category())) {
        continue;
      }
    }
    if (opResult.hasException()) {
      return opResult;
    }

    r = opResult.value();
    if (r == 0) {
      // EOF
      break;
    }

    totalBytes += r;
    while (r != 0 && count != 0) {
      if (r >= ssize_t(iov->iov_len)) {
        r -= ssize_t(iov->iov_len);
        ++iov;
        --count;
      } else {
        iov->iov_base = static_cast<char*>(iov->iov_base) + r;
        iov->iov_len -= r;
        r = 0;
      }
    }
  } while (count);

  return Try<ssize_t>(totalBytes);
}

#ifdef _WIN32
// Shamelessly borrowed from folly/portability/SysUio.cpp:doVecOperation.
// Win32 provides ReadFileScatter and WriteFileGather functions, but those
// operate on multiples of the system page size and operate asynchronously
// which makes them doubly unsuitable for use in emulating readv/writev.
folly::Try<ssize_t> FileDescriptor::doVecOp(
    const struct iovec* iov,
    size_t count,
    bool isRead) const {
  if (!count) {
    return Try<ssize_t>(0);
  }
  if (count > folly::kIovMax) {
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(EINVAL, std::generic_category())));
  }

  // We only need to worry about locking if the file descriptor is
  // a regular file.  We can't lock regions of pipes or sockets.
  bool shouldLock = fdType_ == FDType::Generic;
  if (shouldLock && !LockFile((HANDLE)fd_, 0, 0, 0xffffffff, 0xffffffff)) {
    auto err = GetLastError();
    return Try<ssize_t>(make_exception_wrapper<std::system_error>(
        std::error_code(err, std::system_category()), "LockFile"));
  }
  SCOPE_EXIT {
    if (shouldLock) {
      UnlockFile((HANDLE)fd_, 0, 0, 0xffffffff, 0xffffffff);
    }
  };

  ssize_t bytesProcessed = 0;
  size_t curIov = 0;
  void* curBase = iov[0].iov_base;
  size_t curLen = iov[0].iov_len;
  while (curIov < count) {
    Try<ssize_t> opResult;
    if (isRead) {
      opResult = read(curBase, curLen);
      if (opResult.hasValue() && opResult.value() == 0 && curLen != 0) {
        break; // End of File
      }
    } else {
      opResult = write(curBase, curLen);
      // Write of zero bytes is fine.
    }

    if (opResult.hasException()) {
      return opResult;
    }

    ssize_t res = opResult.value();

    if (size_t(res) == curLen) {
      curIov++;
      if (curIov < count) {
        curBase = iov[curIov].iov_base;
        curLen = iov[curIov].iov_len;
      }
    } else {
      curBase = (void*)((char*)curBase + res);
      curLen -= res;
    }

    if (bytesProcessed + res < 0) {
      // Overflow
      return Try<ssize_t>(make_exception_wrapper<std::system_error>(
          std::error_code(EINVAL, std::generic_category())));
    }
    bytesProcessed += res;
  }

  return Try<ssize_t>(bytesProcessed);
}
#endif

FileDescriptor FileDescriptor::open(
    AbsolutePathPiece path,
    OpenFileHandleOptions opts) {
#ifndef _WIN32
  int flags = (!opts.followSymlinks ? O_NOFOLLOW : 0) |
      (opts.closeOnExec ? O_CLOEXEC : 0) |
#ifdef O_PATH
      (opts.metaDataOnly ? O_PATH : 0) |
#endif
      ((opts.readContents && opts.writeContents)
           ? O_RDWR
           : (opts.writeContents      ? O_WRONLY
                  : opts.readContents ? O_RDONLY
                                      : 0)) |
      (opts.create ? O_CREAT : 0) | (opts.exclusiveCreate ? O_EXCL : 0) |
      (opts.truncate ? O_TRUNC : 0);

  auto fd = ::open(path.copy().c_str(), flags, opts.createMode);
  if (fd == -1) {
    int err = errno;
    throw std::system_error(
        err, std::generic_category(), folly::to<std::string>("open: ", path));
  }
  return FileDescriptor(fd, FileDescriptor::FDType::Unknown);
#else // _WIN32
  DWORD access = 0, share = 0, create = 0, attrs = 0;
  DWORD err;
  auto sec = SECURITY_ATTRIBUTES();

  if (path == "/dev/null"_abspath) {
    path = "NUL:"_abspath;
  }

  auto wpath = path.wide();

  if (opts.metaDataOnly) {
    access = 0;
  } else {
    if (opts.writeContents) {
      access |= GENERIC_WRITE;
    }
    if (opts.readContents) {
      access |= GENERIC_READ;
    }
  }

  // We want more posix-y behavior by default
  share = FILE_SHARE_DELETE | FILE_SHARE_READ | FILE_SHARE_WRITE;

  sec.nLength = sizeof(sec);
  sec.bInheritHandle = TRUE;
  if (opts.closeOnExec) {
    sec.bInheritHandle = FALSE;
  }

  if (opts.create && opts.exclusiveCreate) {
    create = CREATE_NEW;
  } else if (opts.create && opts.truncate) {
    create = CREATE_ALWAYS;
  } else if (opts.create) {
    create = OPEN_ALWAYS;
  } else if (opts.truncate) {
    create = TRUNCATE_EXISTING;
  } else {
    create = OPEN_EXISTING;
  }

  attrs = FILE_FLAG_POSIX_SEMANTICS | FILE_FLAG_BACKUP_SEMANTICS;
  if (!opts.followSymlinks) {
    attrs |= FILE_FLAG_OPEN_REPARSE_POINT;
  }

  FileDescriptor file(
      reinterpret_cast<intptr_t>(CreateFileW(
          wpath.c_str(), access, share, &sec, create, attrs, nullptr)),
      FileDescriptor::FDType::Unknown);
  err = GetLastError();
  if (!file) {
    throw std::system_error(
        err,
        std::system_category(),
        folly::to<std::string>("CreateFileW for openFileHandle: ", path));
  }

  return file;
#endif
}

} // namespace facebook::eden
