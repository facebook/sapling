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

#include "eden/fs/utils/PathFuncs.h"

namespace folly {
class IOBuf;
class exception_wrapper;
} // namespace folly

namespace facebook {
namespace eden {

/**
 * TakeoverData contains the data exchanged between processes during
 * graceful mount point takeover.
 */
class TakeoverData {
 public:
  struct MountInfo {
    MountInfo(
        AbsolutePathPiece p,
        const std::vector<AbsolutePath>& bindMountPaths,
        folly::File fd)
        : path{p}, bindMounts{bindMountPaths}, fuseFD{std::move(fd)} {}

    AbsolutePath path;
    std::vector<AbsolutePath> bindMounts;
    folly::File fuseFD;
  };
  struct HeaderInfo {
    uint32_t messageType{0};
    uint64_t bodyLength{0};
  };

  /**
   * The length of the serialized header.
   *
   * This is a 4-byte protocol identifier followed by the fields in the
   * HeaderInfo struct.
   */
  static constexpr uint32_t kHeaderLength =
      sizeof(uint32_t) + sizeof(uint32_t) + sizeof(uint64_t);

  /**
   * Serialize the TakeoverData into a buffer that can be sent to a remote
   * process.
   *
   * This includes all data except for file descriptors.  The file descriptors
   * must be sent separately.
   */
  std::unique_ptr<folly::IOBuf> serialize();

  /**
   * Serialize an exception.
   */
  static std::unique_ptr<folly::IOBuf> serializeError(
      const folly::exception_wrapper& ew);

  /**
   * Deserialize the header portion of a TakeoverData message.
   *
   * The input buffer should contain to only the message header data (and not
   * more data after this).
   */
  static HeaderInfo deserializeHeader(const folly::IOBuf* buf);

  /**
   * Deserialize the body portion of a TakeoverData message
   */
  static TakeoverData deserializeBody(
      const HeaderInfo& header,
      const folly::IOBuf* buf);

  /**
   * The main eden lock file that prevents two edenfs processes from running at
   * the same time.
   */
  folly::File lockFile;

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
  static constexpr uint32_t kMagicNumber = 0xede0ede1;

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
};

} // namespace eden
} // namespace facebook
