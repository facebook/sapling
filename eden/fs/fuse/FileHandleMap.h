/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once
#include <folly/Range.h>
#include <folly/Synchronized.h>
#include <unordered_map>
#include "eden/fs/fuse/FuseTypes.h"

namespace facebook {
namespace eden {

class DirHandle;
class FileHandle;
class FileHandleBase;
class SerializedFileHandleMap;

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
  uint64_t recordHandle(
      std::shared_ptr<FileHandleBase> fh,
      InodeNumber inodeNumber);

  /** Records a file handle mapping when deserializing the map.
   * This is required to ensure that we record the correct mapping
   * when bootstrapping the map during a graceful restart. */
  void recordHandle(
      std::shared_ptr<FileHandleBase> fh,
      InodeNumber inodeNumber,
      uint64_t number);

  /** Delete the association from the fh to a handle instance.
   * Throws EBADF if the file handle is not tracked by this map.
   * On success, returns the instance. */
  std::shared_ptr<FileHandleBase> forgetGenericHandle(uint64_t fh);

  /** Serializes the current file handle mapping to its corresponding
   * thrift data structure representation.  This method is destructive;
   * it will clear the contents of the FileHandleMap, decrementing the
   * reference counts on the FileHandle objects, allowing them to
   * be destroyed even though they have not been closed through FUSE
   * APIs. */
  SerializedFileHandleMap serializeMap();

 private:
  struct HandleEntry {
    std::shared_ptr<FileHandleBase> handle;
    InodeNumber inodeNumber;
  };
  folly::Synchronized<std::unordered_map<uint64_t, HandleEntry>> handles_;
};

} // namespace eden
} // namespace facebook
