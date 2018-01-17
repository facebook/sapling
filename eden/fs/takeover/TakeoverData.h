/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <folly/File.h>
#include <folly/futures/Promise.h>
#include <memory>
#include <vector>

#include "eden/fs/fuse/FuseTypes.h"
#include "eden/fs/takeover/gen-cpp2/takeover_types.h"
#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class IOBuf;
class exception_wrapper;
} // namespace folly

namespace facebook {
namespace eden {

class SerializedFileHandleMap;

/**
 * TakeoverData contains the data exchanged between processes during
 * graceful mount point takeover.
 */
class TakeoverData {
 public:
  struct MountInfo {
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        const std::vector<AbsolutePath>& bindMountPaths,
        folly::File fd,
        fuse_init_out connInfo,
        SerializedFileHandleMap&& fileHandleMap,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          bindMounts{bindMountPaths},
          fuseFD{std::move(fd)},
          connInfo{connInfo},
          fileHandleMap{std::move(fileHandleMap)},
          inodeMap{std::move(inodeMap)} {}

    AbsolutePath mountPath;
    AbsolutePath stateDirectory;
    std::vector<AbsolutePath> bindMounts;
    folly::File fuseFD;
    fuse_init_out connInfo;
    SerializedFileHandleMap fileHandleMap;
    SerializedInodeMap inodeMap;
  };

  /**
   * Serialize the TakeoverData into a buffer that can be sent to a remote
   * process.
   *
   * This includes all data except for file descriptors.  The file descriptors
   * must be sent separately.
   */
  folly::IOBuf serialize();

  /**
   * Serialize an exception.
   */
  static folly::IOBuf serializeError(const folly::exception_wrapper& ew);

  /**
   * Deserialize the TakeoverData from a buffer.
   */
  static TakeoverData deserialize(const folly::IOBuf* buf);

  /**
   * The main eden lock file that prevents two edenfs processes from running at
   * the same time.
   */
  folly::File lockFile;

  /**
   * The thrift server socket.
   */
  folly::File thriftSocket;

  /**
   * The list of mount points.
   */
  std::vector<MountInfo> mountPoints;

  /**
   * The takeoverComplete promise will be fulfilled by the TakeoverServer code
   * once the TakeoverData has been sent to the remote process.
   */
  folly::Promise<folly::Unit> takeoverComplete;

 private:
  /**
   * Message type values.
   * If we ever need to include more information in the takeover data in the
   * future we can do so by adding new message types here, and deprecating the
   * older formats once we have upgraded all servers to use the new format.
   */
  enum MessageType : uint32_t {
    ERROR = 1,
    MOUNTS = 2,
  };

  /**
   * The length of the serialized header.
   * This is just a 4-byte message type field.
   */
  static constexpr uint32_t kHeaderLength = sizeof(uint32_t);
};

} // namespace eden
} // namespace facebook
