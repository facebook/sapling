/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <memory>
#include <optional>
#include <vector>

#include <folly/File.h>
#include <folly/futures/Promise.h>
#include <folly/io/Cursor.h>

#include "eden/common/utils/FutureUnixSocket.h"
#include "eden/common/utils/PathFuncs.h"
#include "eden/common/utils/UnixSocket.h"
#include "eden/fs/config/MountProtocol.h"
#include "eden/fs/takeover/gen-cpp2/takeover_types.h"
#include "eden/fs/utils/FsChannelTypes.h"

namespace folly {
class IOBuf;
class exception_wrapper;
} // namespace folly

namespace facebook::eden {

// The protocol versions this build accepts. Only version 7 remains; features
// added since then are negotiated as capabilities, so the version exchange only
// serves to reject a peer that predates capability matching.
extern const std::set<int32_t> kSupportedTakeoverVersions;

// Every capability of version 7. Both peers must support all of them; a
// capability added later may be optional.
extern const uint64_t kSupportedCapabilities;

// TODO (T104724681): use a nicer representation for a bit set to combine these
// flags like watchman's OptionSet.
// Note that capabilities must be things that a new version is able to do, not
// things that that version can not do. Capabilities must be all positive so
// that we can find the intersection of the capabilities of the server and
// client to find all the capabilities they both support.
class TakeoverCapabilities {
 public:
  enum : uint64_t {
    // Bit 0 was CUSTOM_SERIALIZATION, the pre-thrift format.

    // Indicates this version of the protocol is able to serialize FUSE mount
    // points.
    FUSE = 1 << 1,

    // Indicates this version of the protocol uses thrift based serialization.
    // This means we use thrift to serialize our takeover data when sending it
    // between client and server. See the types defined in takeover.thrift.
    // This is used in all the modern takeover versions.
    THRIFT_SERIALIZATION = 1 << 2,

    // Indicates a ping will be sent by the server to the client before sending
    // takeover data. This handles client failure cases more gracefully.
    // This should be used in all modern takeover versions.
    PING = 1 << 3,

    // Indicates that the protocol includes the type of kernel module that will
    // be used for each mount point.
    // This should be used in all modern takeover versions.
    MOUNT_TYPES = 1 << 4,

    // Indicates this version of the protocol is able to serialize NFS mount
    // points
    NFS = 1 << 5,

    // Indicates that the data is serialized as a SerializedTakeoverResult,
    // which can carry information that is not specific to a mount.
    RESULT_TYPE_SERIALIZATION = 1 << 6,

    // Indicates that we will specify which fds we transfer during takeover and
    // what order they are in the takeover message instead of passing all of
    // them all the time in a hard coded order.
    ORDERED_FDS = 1 << 7,

    // does the mountd socket need to be sent.
    // Note this capability can not be used with out ORDERED_FDS.
    OPTIONAL_MOUNTD = 1 << 8,

    // Indicates that we will match capabilities of the server and client
    // to decide on a protocol. While we are switching over from versions to
    // capabilities, this means we match versions then match capabilities.
    // But eventually we will skip matching versions and just go straight to
    // matching capabilities.
    CAPABILITY_MATCHING = 1 << 9,

    // Indicates that we include the size of the header in the header itself.
    // This will allow us to more safely evolve the header in the future.
    INCLUDE_HEADER_SIZE = 1 << 10,

    // Indicates that we chunk the takeover data into multiple messages. This
    // allows us to send larger amounts of data in a single takeover.
    CHUNKED_MESSAGE = 1 << 11,
  };
};

/**
 * TakeoverData contains the data exchanged between processes during
 * graceful mount point takeover.
 */
class TakeoverData {
 public:
  enum : int32_t {
    // Never accepted. It is the version written in the header of an error
    // reported before capabilities were negotiated, and tests use it as an
    // incompatible peer.
    kTakeoverProtocolVersionNeverSupported = 0,

    // Versions 1 through 6 predate capability matching and are no longer
    // accepted. Version 7 is the last version: new features are capabilities.
    kTakeoverProtocolVersionSeven = 7
  };

  /**
   * Given a set of versions provided by a client, find the largest
   * version that is also present in the provided set of supported
   * versions.
   */
  static std::optional<int32_t> computeCompatibleVersion(
      const std::set<int32_t>& versions,
      const std::set<int32_t>& supported = kSupportedTakeoverVersions);

  /**
   * Finds the set of capabilities supported by both the server and client.
   * Throws if the intersection lacks any capability of version 7, which every
   * peer that gets this far must have.
   */
  static uint64_t computeCompatibleCapabilities(
      uint64_t capabilities,
      uint64_t supported);

  struct MountInfo {
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        FsChannelInfo channelInfo,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          channelInfo{std::move(channelInfo)},
          inodeMap{std::move(inodeMap)} {}

    /**
     * Constructor for an NFS mount's MountInfo
     */
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        NfsChannelData nfsChannelData,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          channelInfo{std::move(nfsChannelData)},
          inodeMap{std::move(inodeMap)} {}

    /**
     * Constructor for a Projected FS mount's MountInfo
     */
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        ProjFsChannelData projfsChannelData,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          channelInfo{std::move(projfsChannelData)},
          inodeMap{std::move(inodeMap)} {}

    MountProtocol getMountProtocol() const;

    AbsolutePath mountPath;
    AbsolutePath stateDirectory;

    FsChannelInfo channelInfo;

    SerializedInodeMap inodeMap;
  };

  struct ProtocolCapabilitiesAndTrimSize {
    uint64_t protocolCapabilities;
    size_t trimSize;
  };

  /**
   * Serialize the TakeoverData into a unix socket message.
   *
   * The serialization format is a follows:
   * <32 bit version><32 bit header size><64 bit capabilities>
   * <variable length serialized state that we need to send for takeover>
   * The version, header size, and capabilities are considered the "header".
   * - the version is always 7. It is kept so that a peer that only speaks an
   * older version recognizes the mismatch.
   * - the size of the header is the size of the header in bytes excluding the
   * version and the size value itself. For now the capabilities are the only
   * object included here, but having a size in the header makes it more safe to
   * evolve.
   * - the capabilities are the features available in the protocol that we are
   * using for takeover. These are an intersection of the server and clients
   * capabilities.
   * - the state we serialize is serialied in thrift structs as defined in
   * takeover.thrift.
   *
   * An error reported before capabilities were negotiated carries only a
   * version word of 0, followed by the serialized error.
   *
   * Note that we keep the capabilities outside of the serialized thrift data on
   * purpose. This allows us to migrate to a different serialization method as
   * we please.
   */
  void serialize(uint64_t protocolCapabilities, UnixSocket::Message& msg);

  /**
   * Serialize an exception.
   */
  static folly::IOBuf serializeError(
      uint64_t protocolCapabilities,
      const folly::exception_wrapper& ew);

  /**
   * Create a ping to send.
   */
  static folly::IOBuf serializePing();

  /**
   * Create a FIRST_CHUNK message to send.
   */
  static folly::IOBuf serializeFirstChunk();

  /**
   * Create a LAST_CHUNK message to send.
   */
  static folly::IOBuf serializeLastChunk();

  /**
   * Determine the protocol version of the serialized message in buf.
   *
   * Note this should only be called once. This will advance the buffer past
   * the header so that the data to deserialize is at the beginning of the
   * buffer.
   */
  static uint64_t getProtocolCapabilities(folly::IOBuf* buf);

  /**
   * Deserialize the TakeoverData from a UnixSocket msg.
   */
  static TakeoverData deserialize(UnixSocket::Message& msg);

  /**
   * Checks to see if a message is of type PING
   */
  static bool isPing(const folly::IOBuf* buf);

  /**
   * Checks to see if a message is of type FIRST_CHUNK. This is used to
   * determine if we should continue to read from the socket or not.
   */
  static bool isFirstChunk(const folly::IOBuf* buf);

  /**
   * Checks to see if a message is of type LAST_CHUNK. This is used to
   * determine if we should stop reading from the socket or not.
   */
  static bool isLastChunk(const folly::IOBuf* buf);

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
   * Server socket for the mountd.
   */
  std::optional<folly::File> mountdServerSocket;

  /**
   * Tracks whether mountd accepts were paused while preparing takeover data.
   * This is used only for local takeover recovery and is not serialized to the
   * taking-over process.
   */
  bool mountdAcceptsPaused{false};

  std::vector<FileDescriptorType> generalFDOrder;

  // allows manipulating the result of generateGeneralFdOrder in tests.
  // should not be set to anything other than std::nullopt production.
  std::optional<std::vector<FileDescriptorType>> injectedFdOrderForTesting;

  /**
   * The list of mount points.
   */
  std::vector<MountInfo> mountPoints;

  /**
   * The takeoverComplete promise will be fulfilled by the TakeoverServer code
   * once the TakeoverData has been sent to the remote process.
   */
  folly::Promise<std::optional<TakeoverData>> takeoverComplete;

 private:
  /**
   * Serialize the TakeoverData using the specified protocol capabilities into a
   * buffer that can be sent to a remote process.
   *
   * This includes all data except for file descriptors; these must be sent
   * separately.
   */
  folly::IOBuf serialize(uint64_t protocolCapabilities);

  /**
   * Serialize the <version|size|capability> header into the buffer.
   * The size of the header excludes the version and size field itself. For
   * now this only includes capabilities which are 8 bytes.
   */
  static void serializeHeader(
      uint64_t protocolCapabilities,
      folly::IOBufQueue& buf);

  /**
   * Serialize the header and the thrift encoded mounts, split into chunks of
   * at most FLAGS_maximumChunkSize bytes.
   */
  folly::IOBuf serializeThrift(uint64_t protocolCapabilities);

  /**
   * Serialize the header and a thrift encoded exception.
   */
  static folly::IOBuf serializeErrorThrift(
      uint64_t protocolCapabilities,
      const folly::exception_wrapper& ew);

  /*
   * Serialize the file descriptor in this TakeoverData instance for the given
   * file `type` into the list of `files` for the UnixSocket::Message.
   */
  void serializeFd(FileDescriptorType type, std::vector<folly::File>& files);

  /**
   * Deserialize the thrift encoded TakeoverData that follows the header.
   */
  static TakeoverData deserializeThrift(folly::IOBuf* buf);

  /**
   * Deserialize the file descriptor for file `type` from the
   * UnixSocket::Message `file` into this TakeoverData instance.
   */
  void deserializeFd(FileDescriptorType type, folly::File& file);

  /*
   * Deserialize the data on the mounts serialized in serializedMounts into
   * a TakeoverData object. This TakeoverData object will not yet have any of
   * the file descriptors or generic data filled in yet.
   */
  static TakeoverData deserializeThriftMounts(
      std::vector<SerializedMountInfo>& serializedMounts);

  /**
   * Determine the protocol version of the serialized message in buf.
   *
   * Note this doesn't trim the header from the buffer. This is because we
   * want to be able to read the header multiple times with this function.
   */
  static ProtocolCapabilitiesAndTrimSize getProtocolCapabilitiesWithoutTrim(
      folly::IOBuf* buf);

  /**
   * Generates an order for the general file descriptors to be sent in
   * sendmsg. "general file descriptors" does not include file descriptors
   * for mount points. The mountd socket is only included when present.
   *
   * To manipulate this return type for testing set injectedFdOrderForTesting.
   */
  std::vector<FileDescriptorType> generateGeneralFdOrder();

  /**
   * Message type values.
   * If we ever need to include more information in the takeover data in the
   * future we can do so by adding new message types here, and deprecating the
   * older formats once we have upgraded all servers to use the new format.
   */
  enum MessageType : uint32_t {
    ERROR = 1,
    MOUNTS = 2,
    PING = 3,
    LAST_CHUNK = 4,
    FIRST_CHUNK = 5,
  };

  /**
   * The length of the serialized header.
   * This is just a 4-byte message type field.
   */
  static constexpr uint32_t kHeaderLength = sizeof(uint32_t);
};

} // namespace facebook::eden
