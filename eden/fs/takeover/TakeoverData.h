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

#include "eden/fs/takeover/gen-cpp2/takeover_types.h"
#include "eden/fs/utils/FsChannelTypes.h"
#include "eden/fs/utils/FutureUnixSocket.h"
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/utils/UnixSocket.h"

namespace folly {
class IOBuf;
class exception_wrapper;
} // namespace folly

namespace facebook::eden {

// Holds the versions supported by this build.
// TODO(T104382350): The code is being migrated to use capability bits instead
// of version numbers as the former makes it less error prone to check for
// supported features by both the client and server. Currently, the protocol
// works by agreeing on a mutually supported version and then
// agreeing on a shared set of capabilities. We agree on a version first so
// that we can gracefully transition to a protocol that supports capability
// matching.

// Eventually we want to completely migrate to capability matching and stop
// checking the versions. But first, we need to capability matching to make it
// into a stable EdenFS build. Note "stable build" means we should never need to
// rollback before the change, so the capability matching build should be out
// for at least a month before we delete all the version code. If you are
// reading this comment after Sept 2022, this has reached stable.
extern const std::set<int32_t> kSupportedTakeoverVersions;

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
    // DEPRECATED This indicates we use our own invented format for sending
    // takeover data between the client and server. This was used in early
    // versions of the protocol. No longer Supported in any version of EdenFS
    CUSTOM_SERIALIZATION = 1 << 0,

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

    // Indicates that we use SerializedTakeoverResult instead of
    // SerializedTakeoverData to serialize and deserialize the data. This allows
    // us to pass non mount specific information in the takeover message.
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
  };
};

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

    // DEPRECATED: This is the protocol version supported by eden just prior
    // to this protocol versioning code being written
    kTakeoverProtocolVersionOne = 1,

    // TODO: deprecate versions 3-5 after capability matching is stable
    // roughly Feb 2022 T104382350.

    // This version introduced thrift encoding of the takeover structures.
    // It is nominally version 2 but named 3 here because VersionOne
    // responses don't include a version header, but do always respond
    // with either a word set to either 1 or 2.  To disambiguate things
    // we want to use word value 3 to indicate this new protocol, but
    // naming the symbol Version3 and assigning it the value 3 seemed
    // like too much of a headache, so we simply skip over using
    // version 2 to describe this next one.
    kTakeoverProtocolVersionThree = 3,

    // This version introduced an additional handshake before taking over
    // that is sent after the TakeoverData is ready but before actually
    // sending it. This is in order to make sure we only send the data if
    // the new process is healthy and able to receive, because otherwise
    // we would want to recover ourselves. While this does not change
    // the actual data format of the TakeoverData, it does change the
    // number of sends/receives, and this additional handshake would
    // break a server with this extra handshake talking to a client
    // without it
    kTakeoverProtocolVersionFour = 4,

    // This version introduced the ability to takeover NFS mounts.
    // This includes serializing the mountd socket as well as the
    // connected socket to the kernel for each of the mount points.
    kTakeoverProtocolVersionFive = 5,

    // This version introduced a more generic thrift struct for serialization
    // and allows us to only pass some of the file descriptors.
    kTakeoverProtocolVersionSix = 6,

    // This should be the last version. This version matches capabilities with
    // the takeover server and client. This ends needing to create any more
    // versions because you can just introduce new cababilities.
    kTakeoverProtocolVersionSeven = 7
    // there should be no more versions after this.
  };

  /**
   * Converts a supported version to the capabilities that version of the
   * protocol supports. This is used as a bridge between the version based
   * protocol we use now and the capability based protocol we would like to use
   * in future versions. See T104382350.
   */
  static uint64_t versionToCapabilites(int32_t version);

  /**
   * Converts a valid set of capabilities into the takeover version that
   * supports exactly those capabilities. This is used to "serialize" the
   * capabilities. Older versions of the protocol were version based instead of
   * capability based. So we "serialize" the capabilities as a version number.
   * Eventually we will migrate off versions, then we can get rid of this.
   */
  static int32_t capabilitesToVersion(uint64_t capabilities);

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
   * also checks that that set of capabilities is a valid combination (some
   * capabilities are required these days and some capabilities have
   * dependencies between them.)
   */
  static uint64_t computeCompatibleCapabilities(
      uint64_t capabilities,
      uint64_t supported);

  struct MountInfo {
    /**
     * Constructor for an NFS mount's MountInfo
     */
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        const std::vector<AbsolutePath>& bindMountPaths,
        NfsChannelData nfsChannelData,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          bindMounts{bindMountPaths},
          channelInfo{std::move(nfsChannelData)},
          inodeMap{std::move(inodeMap)} {}

    /**
     * Constructor for a Fuse mount's MountInfo
     */
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        const std::vector<AbsolutePath>& bindMountPaths,
        FuseChannelData fuseChannelData,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          bindMounts{bindMountPaths},
          channelInfo{std::move(fuseChannelData)},
          inodeMap{std::move(inodeMap)} {}

    /**
     * Constructor for a Projected FS mount's MountInfo
     */
    MountInfo(
        AbsolutePathPiece mountPath,
        AbsolutePathPiece stateDirectory,
        const std::vector<AbsolutePath>& bindMountPaths,
        ProjFsChannelData projfsChannelData,
        SerializedInodeMap&& inodeMap)
        : mountPath{mountPath},
          stateDirectory{stateDirectory},
          bindMounts{bindMountPaths},
          channelInfo{std::move(projfsChannelData)},
          inodeMap{std::move(inodeMap)} {}

    AbsolutePath mountPath;
    AbsolutePath stateDirectory;
    std::vector<AbsolutePath> bindMounts;

    std::variant<FuseChannelData, NfsChannelData, ProjFsChannelData>
        channelInfo;

    SerializedInodeMap inodeMap;
  };

  /**
   * Serialize the TakeoverData into a unix socket message.
   *
   * The current serialization format is a follows:
   * <32 bit version><32 bit header size><64 bit capabilities>
   * <variable length serialized state that we need to send for takeover>
   * The version, header size, and capabilities are considered the "header".
   * - the version at this point is included for legacy reasons. We use to
   * version our protocol everytime we made a change. but that led to a bunch
   * of confusing version checks in the code. So we moved to capabilities.
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
   * Determines if we should serialized NFS data given the protocol version
   * we are serializing with. i.e. should we send takeover data for NFS mount
   * points and should we send the mountd socket.
   */
  static bool shouldSerdeNFSInfo(uint32_t protocolVersionCapabilies);

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
   * Serialize data for any version that uses thrift serialization. This is
   * versions 3+.
   */
  folly::IOBuf serializeThrift(uint64_t protocolCapabilities);

  /**
   * Serialize an exception for any version that uses thrift serialization. This
   * is versions 3+.
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
   * Deserialize the TakeoverData from a buffer. We assume that we are only sent
   * mounts with mount protocols that we are able to parse.
   */
  static TakeoverData deserialize(
      uint64_t protocolCapabilities,
      folly::IOBuf* buf);

  /**
   * Deserialize the TakeoverData from a buffer for any version of the protocol
   * that uses thrift serialization. This is any version 3+.
   */
  static TakeoverData deserializeThrift(
      uint32_t protocolCapabilities,
      folly::IOBuf* buf);

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
      uint32_t protocolCapabilities,
      std::vector<SerializedMountInfo>& serializedMounts);

  /**
   * Generates an order for the general file descriptors to be sent in
   * sendmsg. "general file descriptors" does not include file descriptors
   * for mount points. This order may vary in length depending on which file
   * descriptors need to be sent in the given protocol capabilities.
   * This method is virtual to allow mocking it in tests.
   *
   * To manipulate this return type for testing set injectedFdOrderForTesting.
   */
  std::vector<FileDescriptorType> generateGeneralFdOrder(
      uint32_t protocolCapabilities);

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
  };

  /**
   * The length of the serialized header.
   * This is just a 4-byte message type field.
   */
  static constexpr uint32_t kHeaderLength = sizeof(uint32_t);
};

} // namespace facebook::eden
