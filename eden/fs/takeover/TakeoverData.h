/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <folly/File.h>
#include <folly/futures/Promise.h>
#include <memory>
#include <optional>
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

// Holds the versions supported by this build.
extern const std::set<int32_t> kSupportedTakeoverVersions;

/**
 * TakeoverData contains the data exchanged between processes during
 * graceful mount point takeover.
 */
class TakeoverData {
 public:
  enum : int32_t {
    // The list of possible versions supported by the client
    // and server in this build of the code.  If/when we
    // bump the version we will retain support for the prior
    // version in both the client and server in order to
    // allow rolling back a new build.

    // This is a protocol version that we will never support.
    // It is included in this enum to reserve it and so that
    // we can use it in tests
    kTakeoverProtocolVersionNeverSupported = 0,

    // This is the protocol version supported by eden just prior
    // to this protocol versioning code being written
    kTakeoverProtocolVersionOne = 1,

    // This version introduced thrift encoding of the takeover structures.
    // It is nominally version 2 but named 3 here because VersionOne
    // responses don't include a version header, but do always respond
    // with either a word set to either 1 or 2.  To disambiguate things
    // we want to use word value 3 to indicate this new protocol, but
    // naming the symbol Version3 and assigning it the value 3 seemed
    // like too much of a headache, so we simply skip over using
    // version 2 to describe this next one.
    kTakeoverProtocolVersionThree = 3,
  };

  // Given a set of versions provided by a client, find the largest
  // version that is also present in the provided set of supported
  // versions.
  static std::optional<int32_t> computeCompatibleVersion(
      const std::set<int32_t>& versions,
      const std::set<int32_t>& supported = kSupportedTakeoverVersions);

  struct MountInfo {
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        const std::vector<AbsolutePath>& bindMountPaths,
        folly::File fd,
        fuse_init_out connInfo,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          bindMounts{bindMountPaths},
          fuseFD{std::move(fd)},
          connInfo{connInfo},
          inodeMap{std::move(inodeMap)} {}

    AbsolutePath mountPath;
    AbsolutePath stateDirectory;
    std::vector<AbsolutePath> bindMounts;
    folly::File fuseFD;
    fuse_init_out connInfo;
    SerializedInodeMap inodeMap;
  };

  /**
   * Serialize the TakeoverData into a buffer that can be sent to a remote
   * process.
   *
   * This includes all data except for file descriptors.  The file descriptors
   * must be sent separately.
   */
  folly::IOBuf serialize(int32_t protocolVersion);

  /**
   * Serialize an exception.
   */
  static folly::IOBuf serializeError(
      int32_t protocolVersion,
      const folly::exception_wrapper& ew);

  /**
   * Deserialize the TakeoverData from a buffer.
   */
  static TakeoverData deserialize(folly::IOBuf* buf);

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
   * Serialize data using version 1 of the takeover protocol.
   */
  folly::IOBuf serializeVersion1();

  /**
   * Serialize an exception using version 1 of the takeover protocol.
   */
  static folly::IOBuf serializeErrorVersion1(
      const folly::exception_wrapper& ew);

  /**
   * Deserialize the TakeoverData from a buffer using version 1 of the takeover
   * protocol.
   */
  static TakeoverData deserializeVersion1(folly::IOBuf* buf);

  /**
   * Serialize data using version 2 of the takeover protocol.
   */
  folly::IOBuf serializeVersion3();

  /**
   * Serialize an exception using version 2 of the takeover protocol.
   */
  static folly::IOBuf serializeErrorVersion3(
      const folly::exception_wrapper& ew);

  /**
   * Deserialize the TakeoverData from a buffer using version 2 of the takeover
   * protocol.
   */
  static TakeoverData deserializeVersion3(folly::IOBuf* buf);

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
