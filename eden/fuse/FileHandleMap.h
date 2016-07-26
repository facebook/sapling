/*
 *  Copyright (c) 2016, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Synchronized.h>
#include <unordered_map>

namespace facebook {
namespace eden {
namespace fusell {

class FileHandle;
class DirHandle;
class FileHandleBase;

/** Keeps track of file handle numbers and their associate FileHandleBase
 *
 * This class allows us to manage the overall set of open file and directory
 * handles.  It provides a way to assign a file handle number that is
 * usable by the kernel to refer to an instance of a file handle, and
 * a way to map that number and return a shared_ptr to the associated
 * file handle.
 *
 * During a hot upgrade we intend to use this mapping to pass information
 * on to the replacement child process, although that functionality has
 * not yet been written.
 */
class FileHandleMap {
 public:
  /** Returns the FileHandleBase associated with a file handle number.
   * Can throw EBADF if the file handle is not one that is tracked by this map.
   */
  std::shared_ptr<FileHandleBase> getGenericFileHandle(uint64_t fh);

  /** Returns the FileHandle associated with a file handle number.
   * Can throw EBADF if the file handle is not tracked by this map,
   * or EISDIR if the handle is a DirHandle instead of a FileHandle. */
  std::shared_ptr<FileHandle> getFileHandle(uint64_t fh);

  /** Returns the DirHandle associated with a file handle number.
   * Can throw EBADF if the file handle is not tracked by this map,
   * or ENOTDIR if the handle is a FileHandle instead of a DirHandle. */
  std::shared_ptr<DirHandle> getDirHandle(uint64_t dh);

  /** Assigns a file handle number for the given instance.
   * Repeated calls with the same instance should not happen (it's not
   * how fuse works) and will return a different file handle number
   * each time.
   * In some situations, it may not be possible to assign a number
   * in a reasonable number of attempts and EMFILE will be thrown.
   **/
  uint64_t recordHandle(std::shared_ptr<FileHandleBase> fh);

  /** Delete the association from the fh to a file handle instance.
   * Throws EBADF if the file handle is not tracked by this map, or
   * EISDIR if the instance is a DirHandle rather than a FileHandle.
   * On success, returns the instance. */
  std::shared_ptr<FileHandle> forgetFileHandle(uint64_t fh);

  /** Delete the association from the fh to a dir handle instance.
   * Throws EBADF if the file handle is not tracked by this map, or
   * ENOTDIR if the instance is a FileHandle rather than a DirHandle.
   * On success, returns the instance. */
  std::shared_ptr<DirHandle> forgetDirHandle(uint64_t dh);

 private:
  std::shared_ptr<FileHandleBase> forgetGenericHandle(uint64_t fh);

  folly::Synchronized<
      std::unordered_map<uint64_t, std::shared_ptr<FileHandleBase>>>
      handles_;
};
}
}
}
