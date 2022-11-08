/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/Try.h>
#include <folly/portability/IOVec.h>
#include "eden/fs/utils/PathFuncs.h"

namespace facebook::eden {

/** Windows doesn't have equivalent bits for all of the various
 * open(2) flags, so we abstract it out here */
struct OpenFileHandleOptions {
  unsigned followSymlinks : 1; // O_NOFOLLOW
  unsigned closeOnExec : 1; // O_CLOEXEC
  unsigned metaDataOnly : 1; // avoid accessing file contents
  unsigned readContents : 1; // the read portion of O_RDONLY or O_RDWR
  unsigned writeContents : 1; // the write portion of O_WRONLY or O_RDWR
  unsigned create : 1; // O_CREAT
  unsigned exclusiveCreate : 1; // O_EXCL
  unsigned truncate : 1; // O_TRUNC
  // The posix mode values to use when creating a file.
  // Has no meaning on win32.  On posix systems, will be modified by umask(2).
  int createMode;

  // Convervative defaults won't follow symlinks and won't be inherited
  OpenFileHandleOptions()
      : followSymlinks(0),
        closeOnExec(1),
        metaDataOnly(0),
        readContents(0),
        writeContents(0),
        create(0),
        exclusiveCreate(0),
        truncate(0),
        createMode(0777) {}

  // Open an existing file for reading.
  // Does not follow symlinks.
  static inline OpenFileHandleOptions readFile() {
    OpenFileHandleOptions opts;
    opts.readContents = 1;
    return opts;
  }

  // Open a file for write, creating if needed
  // Does not follow symlinks.
  static inline OpenFileHandleOptions writeFile() {
    OpenFileHandleOptions opts;
    opts.readContents = 1;
    opts.writeContents = 1;
    opts.create = 1;
    return opts;
  }

  // Open a file so that it can be fstat'd
  static inline OpenFileHandleOptions queryFileInfo() {
    OpenFileHandleOptions opts;
    opts.metaDataOnly = 1;
    return opts;
  }

  // Open a directory for directory listing
  // Does not follow symlinks.
  static inline OpenFileHandleOptions openDir() {
    OpenFileHandleOptions opts;
    opts.readContents = 1;
    return opts;
  }
};

// Manages the lifetime of a system independent file descriptor.
// On POSIX systems this is a posix file descriptor.
// On Win32 systems this is a Win32 HANDLE object.
// It will close() the descriptor when it is destroyed.
class FileDescriptor {
 public:
  using system_handle_type =
#ifdef _WIN32
      // We track the HANDLE value as intptr_t to avoid needing
      // to pull in the windows header files all over the place;
      // this is consistent with the _get_osfhandle function in
      // the msvcrt library.
      intptr_t
#else
      int
#endif
      ;

  // Understanding what sort of object the descriptor references
  // is important in a number of situations on Windows systems.
  // This enum allows tracking that type along with the descriptor.
  enum class FDType {
    Unknown,
    Generic,
    Pipe,
    Socket,
  };

  // A value representing the canonical invalid handle
  // value for the system.
  static constexpr system_handle_type kInvalid = -1;

  // Normalizes invalid handle values to our canonical invalid handle value.
  // Otherwise, just returns the handle as-is.
  static system_handle_type normalizeHandleValue(system_handle_type h);

  // If the FDType is Unknown, probe it to determine its type
  static FDType resolveFDType(system_handle_type h, FDType fdType);

  ~FileDescriptor();

  // Default construct to an empty instance
  FileDescriptor() = default;

  // Construct a file descriptor object from an fd.
  // Will happily accept an invalid handle value without
  // raising an error; the FileDescriptor will simply evaluate as
  // false in a boolean context.
  explicit FileDescriptor(system_handle_type fd, FDType fdType);

  // Construct a file descriptor object from an fd.
  // If fd is invalid will throw a generic error with a message
  // constructed from the provided operation name and the current
  // errno value.
  FileDescriptor(system_handle_type fd, const char* operation, FDType fdType);

  // No copying
  FileDescriptor(const FileDescriptor&) = delete;
  FileDescriptor& operator=(const FileDescriptor&) = delete;

  FileDescriptor(FileDescriptor&& other) noexcept;
  FileDescriptor& operator=(FileDescriptor&& other) noexcept;

  // Attempt to duplicate the file descriptor.
  // If successful, returns a new descriptor referencing the same underlying
  // file/stream/socket.
  // On failure, throws an exception.
  FileDescriptor duplicate() const;

  // Closes the associated descriptor
  void close();

  // Stops tracking the descriptor, returning it to the caller.
  // The caller is then responsible for closing it.
  system_handle_type release();

  // In a boolean context, returns true if this object owns
  // a valid descriptor.
  explicit operator bool() const {
    return fd_ != kInvalid;
  }

  // Returns the underlying descriptor value
  inline system_handle_type systemHandle() const {
    return fd_;
  }

#ifndef _WIN32
  // Returns the descriptor value as a file descriptor.
  // This method is only present on posix systems to aid in
  // detecting non-portable use at compile time.
  inline int fd() const {
    return fd_;
  }
#else
  // Returns the descriptor value as a file handle.
  // This method is only present on win32 systems to aid in
  // detecting non-portable use at compile time.
  inline intptr_t handle() const {
    return fd_;
  }
#endif

  inline FDType fdType() const {
    return fdType_;
  }

  // Set the close-on-exec bit
  void setCloExec();
  void clearCloExec();

  // Enable non-blocking IO
  void setNonBlock();

  // Disable non-blocking IO
  void clearNonBlock();

  /** read(2), but yielding a Try for system independent error reporting */
  folly::Try<ssize_t> read(void* buf, int size) const;
  /** read(2), but will continue to read the full `size` parameter in
   * event of short reads or EINTR */
  folly::Try<ssize_t> readFull(void* buf, int size) const;
  folly::Try<ssize_t> readNoInt(void* buf, int size) const;
  folly::Try<ssize_t> readv(struct iovec* iov, size_t numIov) const;
  folly::Try<ssize_t> readvFull(struct iovec* iov, size_t numIov) const;

  /** write(2), but yielding a Try for system independent error reporting */
  folly::Try<ssize_t> write(const void* buf, int size) const;
  folly::Try<ssize_t> writeNoInt(const void* buf, int size) const;
  folly::Try<ssize_t> writeFull(const void* buf, int size) const;
  folly::Try<ssize_t> writev(struct iovec* iov, size_t numIov) const;
  folly::Try<ssize_t> writevFull(struct iovec* iov, size_t numIov) const;

  // Open a file descriptor on the supplied path using the specified
  // open options.  Will throw an exception on failure.
  static FileDescriptor open(
      AbsolutePathPiece path,
      OpenFileHandleOptions options);

  /**
   * Open the null device.
   */
  static FileDescriptor openNullDevice(OpenFileHandleOptions options);

 private:
  system_handle_type fd_{kInvalid};
  FDType fdType_{FDType::Unknown};

#ifdef _WIN32
  folly::Try<ssize_t>
  doVecOp(const struct iovec* iov, size_t numIov, bool isRead) const;
#endif
  folly::Try<ssize_t>
  wrapFull(void* buf, ssize_t size, bool isRead, bool onlyOnce) const;
  folly::Try<ssize_t> wrapvFull(struct iovec* iov, size_t numIov, bool isRead)
      const;
};

} // namespace facebook::eden
