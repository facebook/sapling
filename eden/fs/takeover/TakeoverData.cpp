/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/takeover/TakeoverData.h"

#include <memory>
#include <stdexcept>
#include <variant>

#include <fmt/format.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>
#include "folly/Likely.h"

#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/common/utils/Bug.h"
#include "eden/common/utils/Throw.h"
#include "eden/common/utils/UnixSocket.h"

using apache::thrift::CompactSerializer;
using folly::IOBuf;
using std::string;

/**
 * Maximum size of a chunked message in bytes.
 * Message chunking happens after TakeoverData serialization.
 *
 * This 512 MB size is chosen based on the following calculations:
 * If we want to run a graceful restart for 30 million inodes, the TakeoverData
 * size would be approximately ~5GB. Multiplying this number by 2, we estimate
 * the maximum Takeover size to be ~10GB. With a chunk size of 0.5GB, this
 * results in 20 chunks. Since we use recursion to send TakeoverData, a
 * recursion depth of 20 is safe and won't cause a stack overflow.
 * The chunk size should be large enough to avoid the overhead of
 * sending multiple chunks, and smaller than 1 GB (the maximum data length that
 * we chose for transferring eden TakeoverData over UnixSocket).
 */
DEFINE_int64(
    maximumChunkSize,
    512 * 1024 * 1024, // 512 MB
    "Maximum size of a chunked message in bytes");

namespace facebook::eden {
#ifndef _WIN32
namespace {

/**
 * Determines the mount protocol for the mount point encoded in the mountInfo.
 */
TakeoverMountProtocol getTakeoverMountProtocol(
    const TakeoverData::MountInfo& mountInfo) {
  if (std::holds_alternative<FuseChannelData>(mountInfo.channelInfo)) {
    return TakeoverMountProtocol::FUSE;
  } else if (std::holds_alternative<NfsChannelData>(mountInfo.channelInfo)) {
    return TakeoverMountProtocol::NFS;
  }
  throwf<std::runtime_error>(
      "unrecognized mount protocol {} for mount: {}",
      mountInfo.channelInfo.index(),
      mountInfo.mountPath);
}

} // namespace
#endif

MountProtocol TakeoverData::MountInfo::getMountProtocol() const {
  if (std::holds_alternative<FuseChannelData>(channelInfo)) {
    return MountProtocol::FUSE;
  } else if (std::holds_alternative<NfsChannelData>(channelInfo)) {
    return MountProtocol::NFS;
  } else if (std::holds_alternative<ProjFsChannelData>(channelInfo)) {
    return MountProtocol::PRJFS;
  }

  throwf<std::runtime_error>(
      "unrecognized mount protocol {} for mount: {}",
      channelInfo.index(),
      mountPath);
}

#ifndef _WIN32

const std::set<int32_t> kSupportedTakeoverVersions{
    TakeoverData::kTakeoverProtocolVersionThree,
    TakeoverData::kTakeoverProtocolVersionFour,
    TakeoverData::kTakeoverProtocolVersionFive,
    TakeoverData::kTakeoverProtocolVersionSix,
    TakeoverData::kTakeoverProtocolVersionSeven};

const uint64_t kSupportedCapabilities = TakeoverCapabilities::FUSE |
    TakeoverCapabilities::MOUNT_TYPES | TakeoverCapabilities::PING |
    TakeoverCapabilities::THRIFT_SERIALIZATION | TakeoverCapabilities::NFS |
    TakeoverCapabilities::RESULT_TYPE_SERIALIZATION |
    TakeoverCapabilities::ORDERED_FDS | TakeoverCapabilities::OPTIONAL_MOUNTD |
    TakeoverCapabilities::CAPABILITY_MATCHING |
    TakeoverCapabilities::INCLUDE_HEADER_SIZE |
    TakeoverCapabilities::CHUNKED_MESSAGE;

std::optional<int32_t> TakeoverData::computeCompatibleVersion(
    const std::set<int32_t>& versions,
    const std::set<int32_t>& supported) {
  std::optional<int32_t> best;

  for (auto& version : versions) {
    if (best.has_value() && best.value() > version) {
      // No better than the current best
      continue;
    }
    if (std::find(supported.begin(), supported.end(), version) ==
        supported.end()) {
      // Not supported
      continue;
    }
    best = version;
  }
  return best;
}

uint64_t TakeoverData::computeCompatibleCapabilities(
    const uint64_t capabilities,
    const uint64_t supported) {
  auto compatible = supported & capabilities;
  // assert that the server and client are in some way compatible.
  // The basics are that they share some serialization method so that they can
  // converse. These days we only support thrift serialization.
  if ((compatible & TakeoverCapabilities::THRIFT_SERIALIZATION) == 0) {
    throw std::runtime_error{
        "The client and the server do not share a common takeover protocol "
        "implementation."};
  }
  if ((compatible & TakeoverCapabilities::OPTIONAL_MOUNTD) &&
      ((compatible & TakeoverCapabilities::ORDERED_FDS) == 0)) {
    throw std::runtime_error{
        "Optional mountd can not be used without ordered file descriptors"};
  }

  return compatible;
}

uint64_t TakeoverData::versionToCapabilities(int32_t version) {
  switch (version) {
    case kTakeoverProtocolVersionNeverSupported:
      return 0;
    case kTakeoverProtocolVersionOne:
      return TakeoverCapabilities::CUSTOM_SERIALIZATION |
          TakeoverCapabilities::FUSE;
    case kTakeoverProtocolVersionThree:
      return TakeoverCapabilities::FUSE |
          TakeoverCapabilities::THRIFT_SERIALIZATION;
    case kTakeoverProtocolVersionFour:
      return TakeoverCapabilities::FUSE |
          TakeoverCapabilities::THRIFT_SERIALIZATION |
          TakeoverCapabilities::PING;
    case kTakeoverProtocolVersionFive:
      return TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
          TakeoverCapabilities::PING |
          TakeoverCapabilities::THRIFT_SERIALIZATION |
          TakeoverCapabilities::NFS;
    case kTakeoverProtocolVersionSix:
      return TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
          TakeoverCapabilities::PING |
          TakeoverCapabilities::THRIFT_SERIALIZATION |
          TakeoverCapabilities::NFS |
          TakeoverCapabilities::RESULT_TYPE_SERIALIZATION |
          TakeoverCapabilities::ORDERED_FDS |
          TakeoverCapabilities::OPTIONAL_MOUNTD;
    case kTakeoverProtocolVersionSeven:
      return TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
          TakeoverCapabilities::PING |
          TakeoverCapabilities::THRIFT_SERIALIZATION |
          TakeoverCapabilities::NFS |
          TakeoverCapabilities::RESULT_TYPE_SERIALIZATION |
          TakeoverCapabilities::ORDERED_FDS |
          TakeoverCapabilities::OPTIONAL_MOUNTD |
          TakeoverCapabilities::CAPABILITY_MATCHING |
          TakeoverCapabilities::INCLUDE_HEADER_SIZE |
          TakeoverCapabilities::CHUNKED_MESSAGE;
  }
  throwf<std::runtime_error>("Unsupported version: {}", version);
}

int32_t TakeoverData::capabilitiesToVersion(uint64_t capabilities) {
  if (capabilities == 0) {
    return kTakeoverProtocolVersionNeverSupported;
  }
  if (capabilities ==
      (TakeoverCapabilities::CUSTOM_SERIALIZATION |
       TakeoverCapabilities::FUSE)) {
    return kTakeoverProtocolVersionOne;
  }
  if (capabilities ==
      (TakeoverCapabilities::FUSE |
       TakeoverCapabilities::THRIFT_SERIALIZATION)) {
    return kTakeoverProtocolVersionThree;
  }
  if (capabilities ==
      (TakeoverCapabilities::FUSE | TakeoverCapabilities::THRIFT_SERIALIZATION |
       TakeoverCapabilities::PING)) {
    return kTakeoverProtocolVersionFour;
  }

  if (capabilities ==
      (TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
       TakeoverCapabilities::PING | TakeoverCapabilities::THRIFT_SERIALIZATION |
       TakeoverCapabilities::NFS)) {
    return kTakeoverProtocolVersionFive;
  }

  if (capabilities ==
      (TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
       TakeoverCapabilities::PING | TakeoverCapabilities::THRIFT_SERIALIZATION |
       TakeoverCapabilities::NFS |
       TakeoverCapabilities::RESULT_TYPE_SERIALIZATION |
       TakeoverCapabilities::ORDERED_FDS |
       TakeoverCapabilities::OPTIONAL_MOUNTD)) {
    return kTakeoverProtocolVersionSix;
  }

  if (capabilities ==
      (TakeoverCapabilities::FUSE | TakeoverCapabilities::MOUNT_TYPES |
       TakeoverCapabilities::PING | TakeoverCapabilities::THRIFT_SERIALIZATION |
       TakeoverCapabilities::NFS |
       TakeoverCapabilities::RESULT_TYPE_SERIALIZATION |
       TakeoverCapabilities::ORDERED_FDS |
       TakeoverCapabilities::OPTIONAL_MOUNTD |
       TakeoverCapabilities::CAPABILITY_MATCHING |
       TakeoverCapabilities::INCLUDE_HEADER_SIZE |
       TakeoverCapabilities::CHUNKED_MESSAGE)) {
    return kTakeoverProtocolVersionSeven;
  }

  throwf<std::runtime_error>(
      "Unsupported combination of capabilities: {}", capabilities);
}

bool TakeoverData::shouldSerdeNFSInfo(uint32_t protocolCapabilities) {
  // 4 and below know nothing of NFS mounts. we introduce NFS support in version
  // 5 and expect to continue to support NFS mounts beyond version 5.
  return protocolCapabilities & TakeoverCapabilities::NFS;
}

std::vector<FileDescriptorType> TakeoverData::generateGeneralFdOrder(
    uint32_t protocolCapabilities) {
  if (UNLIKELY(injectedFdOrderForTesting.has_value())) {
    return injectedFdOrderForTesting.value();
  }

  std::vector<FileDescriptorType> fileDescriptorOrder{
      FileDescriptorType::LOCK_FILE, FileDescriptorType::THRIFT_SOCKET};
  if (shouldSerdeNFSInfo(protocolCapabilities) &&
      (!(protocolCapabilities & TakeoverCapabilities::OPTIONAL_MOUNTD) ||
       mountdServerSocket != std::nullopt)) {
    fileDescriptorOrder.push_back(FileDescriptorType::MOUNTD_SOCKET);
  }
  return fileDescriptorOrder;
}

void TakeoverData::serializeFd(
    FileDescriptorType type,
    std::vector<folly::File>& files) {
  folly::File* fileToSerialize{nullptr};
  switch (type) {
    case FileDescriptorType::LOCK_FILE:
      fileToSerialize = &lockFile;
      break;
    case FileDescriptorType::THRIFT_SOCKET:
      fileToSerialize = &thriftSocket;
      break;
    case FileDescriptorType::MOUNTD_SOCKET:
      fileToSerialize = &mountdServerSocket.value();
      break;
    default:
      throwf<std::runtime_error>(
          "Unexpected FileDescriptorType {}", fmt::underlying(type));
  }

  XLOGF(
      DBG7,
      "serializing file type: {} fd: {}",
      fmt::underlying(type),
      fileToSerialize->fd());
  files.push_back(std::move(*fileToSerialize));
}

void TakeoverData::deserializeFd(FileDescriptorType type, folly::File& file) {
  XLOGF(
      DBG7,
      "deserializing file type: {} fd: {}",
      fmt::underlying(type),
      file.fd());
  switch (type) {
    case FileDescriptorType::LOCK_FILE:
      lockFile = std::move(file);
      return;
    case FileDescriptorType::THRIFT_SOCKET:
      thriftSocket = std::move(file);
      return;
    case FileDescriptorType::MOUNTD_SOCKET:
      mountdServerSocket = std::move(file);
      return;
    default:
      throwf<std::runtime_error>(
          "Unexpected FileDescriptorType {}", fmt::underlying(type));
  }
}

void TakeoverData::serialize(
    uint64_t protocolCapabilities,
    UnixSocket::Message& msg) {
  if (protocolCapabilities & TakeoverCapabilities::ORDERED_FDS) {
    // note this needs to happen before serialization of the msg because we
    // serialize the order into the message.
    generalFDOrder = generateGeneralFdOrder(protocolCapabilities);
  }
  msg.data = serialize(protocolCapabilities);
  if (protocolCapabilities & TakeoverCapabilities::ORDERED_FDS) {
    for (auto& fdType : generalFDOrder) {
      serializeFd(fdType, msg.files);
    }
  } else {
    msg.files.push_back(std::move(lockFile));
    msg.files.push_back(std::move(thriftSocket));

    if (shouldSerdeNFSInfo(protocolCapabilities)) {
      XLOGF(DBG7, "serializing mountd socket: {}", mountdServerSocket->fd());
      msg.files.push_back(std::move(mountdServerSocket.value()));
    }
  }

  for (auto& mount : mountPoints) {
    if (auto fuseData = std::get_if<FuseChannelData>(&mount.channelInfo)) {
      msg.files.push_back(std::move(fuseData->fd));
    } else if (auto nfsData = std::get_if<NfsChannelData>(&mount.channelInfo)) {
      msg.files.push_back(std::move(nfsData->nfsdSocketFd));
    } else {
      throw std::runtime_error("Unexpected Channel Type");
    }
  }
}

IOBuf TakeoverData::serialize(uint64_t protocolCapabilities) {
  XCHECK(protocolCapabilities & TakeoverCapabilities::THRIFT_SERIALIZATION)
      << fmt::format(
             "Asked to serialize takeover data in unsupported format. "
             "Capabilities: {}",
             protocolCapabilities);
  return serializeThrift(protocolCapabilities);
}

folly::IOBuf TakeoverData::serializeError(
    uint64_t protocolCapabilities,
    const folly::exception_wrapper& ew) {
  XCHECK(
      protocolCapabilities & TakeoverCapabilities::THRIFT_SERIALIZATION ||
      protocolCapabilities == 0)
      << fmt::format(
             "Asked to serialize takeover data in unsupported format. "
             "Capabilities: {}",
             protocolCapabilities);
  // We allow NeverSupported in the error case so that we don't
  // end up erroring out in the version mismatch error
  // reporting case.

  return serializeErrorThrift(protocolCapabilities, ew);
}

bool TakeoverData::isPing(const IOBuf* buf) {
  if (buf->length() == sizeof(uint32_t)) {
    folly::io::Cursor cursor(buf);
    auto messageType = cursor.readBE<uint32_t>();
    return messageType == MessageType::PING;
  }
  return false;
}

folly::IOBuf TakeoverData::serializePing() {
  IOBuf buf(IOBuf::CREATE, kHeaderLength);
  folly::io::Appender app(&buf, 0);
  app.writeBE<uint32_t>(MessageType::PING);
  XLOGF(DBG8, "Serialized ping message");
  return buf;
}

bool TakeoverData::isFirstChunk(const IOBuf* buf) {
  if (buf->length() == sizeof(uint32_t)) {
    folly::io::Cursor cursor(buf);
    auto messageType = cursor.readBE<uint32_t>();
    return messageType == MessageType::FIRST_CHUNK;
  }
  return false;
}

folly::IOBuf TakeoverData::serializeFirstChunk() {
  IOBuf buf(IOBuf::CREATE, kHeaderLength);
  folly::io::Appender app(&buf, 0);
  app.writeBE<uint32_t>(MessageType::FIRST_CHUNK);
  XLOGF(DBG8, "Serialized first chunk message");
  return buf;
}

bool TakeoverData::isLastChunk(const IOBuf* buf) {
  if (buf->length() == sizeof(uint32_t)) {
    folly::io::Cursor cursor(buf);
    auto messageType = cursor.readBE<uint32_t>();
    return messageType == MessageType::LAST_CHUNK;
  }
  return false;
}

folly::IOBuf TakeoverData::serializeLastChunk() {
  IOBuf buf(IOBuf::CREATE, kHeaderLength);
  folly::io::Appender app(&buf, 0);
  app.writeBE<uint32_t>(MessageType::LAST_CHUNK);
  XLOGF(DBG8, "Serialized last chunk message");
  return buf;
}

TakeoverData TakeoverData::deserialize(UnixSocket::Message& msg) {
  auto capabilities = TakeoverData::getProtocolCapabilities(&msg.data);

  auto data = TakeoverData::deserialize(capabilities, &msg.data);
  // when we serialize the mountd socket we have three general files instead
  // of two
  const auto mountPointFilesOffset =
      capabilities & TakeoverCapabilities::ORDERED_FDS
      ? data.generalFDOrder.size()
      : shouldSerdeNFSInfo(capabilities) ? 3
                                         : 2;

  // Add 2 here for the lock file and the thrift socket
  if (data.mountPoints.size() + mountPointFilesOffset != msg.files.size()) {
    throw_<std::runtime_error>(
        "received ",
        data.mountPoints.size(),
        " mount paths, but ",
        msg.files.size(),
        " FDs (including the lock file FD)");
  }
  if (capabilities & TakeoverCapabilities::ORDERED_FDS) {
    uint32_t filesIndex = 0;
    for (auto fileType : data.generalFDOrder) {
      data.deserializeFd(fileType, msg.files.at(filesIndex));
      ++filesIndex;
    }
  } else {
    data.lockFile = std::move(msg.files[0]);
    data.thriftSocket = std::move(msg.files[1]);
    if (shouldSerdeNFSInfo(capabilities)) {
      data.mountdServerSocket = std::move(msg.files[2]);
      XLOGF(
          DBG1, "Deserialized mountd Socket {}", data.mountdServerSocket->fd());
    }
  }
  for (size_t n = 0; n < data.mountPoints.size(); ++n) {
    auto& mountInfo = data.mountPoints[n];
    if (auto fuseData = std::get_if<FuseChannelData>(&mountInfo.channelInfo)) {
      fuseData->fd = std::move(msg.files[n + mountPointFilesOffset]);
    } else if (
        auto nfsData = std::get_if<NfsChannelData>(&mountInfo.channelInfo)) {
      nfsData->nfsdSocketFd = std::move(msg.files[n + mountPointFilesOffset]);
    } else {
      throw std::runtime_error("Unexpected Channel Type");
    }
  }
  return data;
}

uint64_t TakeoverData::getProtocolCapabilities(IOBuf* buf) {
  ProtocolCapabilitiesAndTrimSize result =
      getProtocolCapabilitiesWithoutTrim(buf);

  // trim the buffer to the real data
  buf->trimStart(result.trimSize);

  return result.protocolCapabilities;
}

TakeoverData::ProtocolCapabilitiesAndTrimSize
TakeoverData::getProtocolCapabilitiesWithoutTrim(IOBuf* buf) {
  // We need to probe the data to see which version we have
  folly::io::Cursor cursor(buf);

  auto version = cursor.readBE<uint32_t>();

  // We don't trim the buffer here but the caller should know the trimSize
  // and trim the buffer to get to the real data if needed.
  uint32_t trimSize = 0;

  switch (version) {
    case kTakeoverProtocolVersionNeverSupported:
      // we put this here so that we can test incompatible versions and the
      // error can be deserialized
    case kTakeoverProtocolVersionThree:
    case kTakeoverProtocolVersionFour:
    case kTakeoverProtocolVersionFive:
    case kTakeoverProtocolVersionSix:
      trimSize = sizeof(uint32_t);
      return {versionToCapabilities(version), trimSize};
    case kTakeoverProtocolVersionSeven: {
      // version 7 and above should support INCLUDE_HEADER_SIZE and
      // CAPABILITY_MATCHING but we check those assumptions to make this more
      // clear.
      auto versionBasedCapabilities = versionToCapabilities(version);
      auto expected_capabilities = TakeoverCapabilities::INCLUDE_HEADER_SIZE |
          TakeoverCapabilities::CAPABILITY_MATCHING;
      if ((versionBasedCapabilities & expected_capabilities) !=
          expected_capabilities) {
        throw std::runtime_error(fmt::format(
            "Expected version {:x} to support capability matching and "
            "including header size, but it doesn't: {:x}",
            version,
            versionBasedCapabilities));
      }

      // for now the size of the header should just be 8 because it only
      // includes the size of the capabilities.
      std::uint32_t header_size = cursor.readBE<uint32_t>();
      if (header_size != sizeof(uint64_t)) {
        throw std::runtime_error(fmt::format(
            "Invalid takeover header size {:x}, expected {:x}. version: {:x}",
            header_size,
            sizeof(uint64_t),
            version));
      }

      uint64_t capabilities = cursor.readBE<uint64_t>();

      // The header contains the version, header size and header size
      // bytes (currently header size bytes equals 8 and only contain the
      // capabilities). Then the trimSize is the size of the version, header
      // size and capabilities.
      trimSize = sizeof(uint32_t) + sizeof(uint32_t) + header_size;

      return {capabilities, trimSize};
    }
    default:
      throw std::runtime_error(fmt::format(
          "Unrecognized TakeoverData response starting with {:x}", version));
  }
}

TakeoverData TakeoverData::deserialize(
    uint64_t protocolCapabilities,
    IOBuf* buf) {
  XCHECK(
      protocolCapabilities & TakeoverCapabilities::THRIFT_SERIALIZATION ||
      protocolCapabilities == 0)
      << fmt::format(
             "Asked to serialize takeover data in unsupported format. "
             "Capabilities: {}",
             protocolCapabilities);

  return deserializeThrift(protocolCapabilities, buf);
}

bool canSerDeMountType(
    uint64_t protocolCapabilities,
    TakeoverMountProtocol mountProtocol) {
  switch (mountProtocol) {
    case TakeoverMountProtocol::FUSE:
      return protocolCapabilities & TakeoverCapabilities::FUSE;
    case TakeoverMountProtocol::NFS:
      return protocolCapabilities & TakeoverCapabilities::NFS;
    case TakeoverMountProtocol::UNKNOWN:
      return false;
  }
  return false;
}

void checkCanSerDeMountType(
    uint64_t protocolCapabilities,
    TakeoverMountProtocol mountProtocol,
    folly::StringPiece mountPath) {
  if (!canSerDeMountType(protocolCapabilities, mountProtocol)) {
    throwf<std::runtime_error>(
        "protocol does not support serializing/deserializing this type of "
        "mounts. protocol capabilities: {}. problem mount: {}. mount protocol:"
        " {}",
        protocolCapabilities,
        mountPath,
        fmt::underlying(mountProtocol));
  }
}

void TakeoverData::serializeHeader(
    uint64_t protocolCapabilities,
    folly::IOBufQueue& buf) {
  folly::io::QueueAppender appender(&buf, 0);
  int32_t versionToAdvertize = capabilitiesToVersion(protocolCapabilities);
  // first word is the protocol version. previous versions of EdenFS do not
  // know how to deserialize version 4 because they assume that protocol 4
  // uses protocol 3 serialization. We need to do this funkiness for rollback
  // safety.
  if (versionToAdvertize == kTakeoverProtocolVersionFour) {
    versionToAdvertize = kTakeoverProtocolVersionThree;
  }
  appender.writeBE<uint32_t>(versionToAdvertize);
  if (protocolCapabilities & TakeoverCapabilities::INCLUDE_HEADER_SIZE) {
    appender.writeBE<uint32_t>(sizeof(uint64_t));
  }
  if (protocolCapabilities & TakeoverCapabilities::CAPABILITY_MATCHING) {
    appender.writeBE<uint64_t>(protocolCapabilities);
  }
}

IOBuf TakeoverData::serializeThrift(uint64_t protocolCapabilities) {
  folly::IOBufQueue bufQ{folly::IOBufQueue::cacheChainLength()};

  serializeHeader(protocolCapabilities, bufQ);

  std::vector<SerializedMountInfo> serializedMounts;
  serializedMounts.reserve(mountPoints.size());
  for (const auto& mount : mountPoints) {
    auto mountProtocol = getTakeoverMountProtocol(mount);

    checkCanSerDeMountType(
        protocolCapabilities, mountProtocol, mount.mountPath.view());

    SerializedMountInfo serializedMount;

    serializedMount.mountPath() = mount.mountPath.asString();
    serializedMount.stateDirectory() = mount.stateDirectory.asString();

    if (auto fuseChannelInfo =
            std::get_if<FuseChannelData>(&mount.channelInfo)) {
      // Stuffing the fuse connection information in as a binary
      // blob because we know that the endianness of the target
      // machine must match the current system for a graceful
      // takeover, and it saves us from re-encoding an operating
      // system specific struct into a thrift file.
      serializedMount.connInfo() = std::string{
          reinterpret_cast<const char*>(&fuseChannelInfo->connInfo),
          sizeof(fuseChannelInfo->connInfo)};
    }

    serializedMount.inodeMap() = mount.inodeMap;

    serializedMount.mountProtocol() = mountProtocol;

    serializedMounts.emplace_back(std::move(serializedMount));
  }

  if (protocolCapabilities & TakeoverCapabilities::RESULT_TYPE_SERIALIZATION) {
    // depending on if RESULT_TYPE_SERIALIZATION is set we might use either of
    // these types to serialize.

    SerializedTakeoverInfo serialized;
    serialized.mounts() = std::move(serializedMounts);
    if (protocolCapabilities & TakeoverCapabilities::ORDERED_FDS) {
      serialized.fileDescriptors() = generalFDOrder;
    }
    SerializedTakeoverResult result;
    result.takeoverData() = serialized;

    CompactSerializer::serialize(result, &bufQ);
  } else {
    SerializedTakeoverData serialized;
    serialized.mounts() = std::move(serializedMounts);

    CompactSerializer::serialize(serialized, &bufQ);
  }

  if (protocolCapabilities & TakeoverCapabilities::CHUNKED_MESSAGE) {
    // At this point, bufQ can be a chain of multiple IOBufs because of how
    // serializeHeader and the mounts' serialization are implemented.
    // To efficiently split it into chunks later, we need to coalesce all the
    // chains into one. This simplifies the process of splitting the data into
    // chunks of a maximum size (FLAGS_maximumChunkSize).

    // Move the buffer queue to an IOBuf, and leaving buffer queue empty.
    auto tempBuf = bufQ.move();
    // Coalesce the chains within the IOBuf into a single chain.
    tempBuf->coalesce();
    // Append the now single-chain IOBuf back to the buffer queue.
    bufQ.append(std::move(tempBuf));

    auto chunk = bufQ.splitAtMost(FLAGS_maximumChunkSize);
    while (bufQ.chainLength() > 0) {
      chunk->appendToChain(bufQ.splitAtMost(FLAGS_maximumChunkSize));
    }
    XLOGF(
        INFO,
        "Serialized {} bytes TakeoverData in {} chunks",
        chunk->computeChainDataLength(),
        chunk->countChainElements());
    return std::move(*chunk);
  } else {
    return std::move(*bufQ.move());
  }
}

folly::IOBuf TakeoverData::serializeErrorThrift(
    uint64_t protocolCapabilities,
    const folly::exception_wrapper& ew) {
  folly::IOBufQueue bufQ;

  serializeHeader(protocolCapabilities, bufQ);

  auto exceptionClassName = ew.class_name();
  folly::StringPiece what = ew ? ew.get_exception()->what() : "";

  if (protocolCapabilities & TakeoverCapabilities::RESULT_TYPE_SERIALIZATION) {
    // depending on if RESULT_TYPE_SERIALIZATION is set we might use either of
    // these types to serialize.

    SerializedTakeoverResult serialized;
    serialized.errorReason() =
        folly::to<std::string>(exceptionClassName, ": ", what);

    CompactSerializer::serialize(serialized, &bufQ);
  } else {
    SerializedTakeoverData serialized;
    serialized.errorReason() =
        folly::to<std::string>(exceptionClassName, ": ", what);

    CompactSerializer::serialize(serialized, &bufQ);
  }

  return std::move(*bufQ.move());
}

TakeoverData TakeoverData::deserializeThrift(
    uint32_t protocolCapabilities,
    IOBuf* buf) {
  if (protocolCapabilities & TakeoverCapabilities::RESULT_TYPE_SERIALIZATION) {
    auto serialized =
        CompactSerializer::deserialize<SerializedTakeoverResult>(buf);
    switch (serialized.getType()) {
      case SerializedTakeoverResult::Type::errorReason:
        throw std::runtime_error(serialized.get_errorReason());

      case SerializedTakeoverResult::Type::takeoverData: {
        auto takeoverData = deserializeThriftMounts(
            protocolCapabilities, *(serialized.takeoverData()->mounts()));
        if (protocolCapabilities & TakeoverCapabilities::ORDERED_FDS) {
          takeoverData.generalFDOrder =
              *(serialized.takeoverData()->fileDescriptors());
        }
        return takeoverData;
      }
      case SerializedTakeoverResult::Type::__EMPTY__:
        // This case triggers when there are no mounts to pass between
        // the processes; we allow for it here and return an empty
        // TakeoverData instance.
        return TakeoverData{};
    }
  } else {
    auto serialized =
        CompactSerializer::deserialize<SerializedTakeoverData>(buf);
    switch (serialized.getType()) {
      case SerializedTakeoverData::Type::errorReason:
        throw std::runtime_error(serialized.get_errorReason());

      case SerializedTakeoverData::Type::mounts:
        return deserializeThriftMounts(
            protocolCapabilities, serialized.mutable_mounts());
      case SerializedTakeoverData::Type::__EMPTY__:
        // This case triggers when there are no mounts to pass between
        // the processes; we allow for it here and return an empty
        // TakeoverData instance.
        return TakeoverData{};
    }
  }
  throw std::runtime_error(
      "impossible enum variant for SerializedTakeoverData");
}

TakeoverData TakeoverData::deserializeThriftMounts(
    uint32_t protocolCapabilities,
    std::vector<SerializedMountInfo>& serializedMounts) {
  TakeoverData data;
  for (auto& serializedMount : serializedMounts) {
    switch (*serializedMount.mountProtocol()) {
      case TakeoverMountProtocol::UNKNOWN:
        if (protocolCapabilities & TakeoverCapabilities::MOUNT_TYPES) {
          throw std::runtime_error("Unknown Mount Protocol");
        }
        // versions <5 all assumed FUSE mounts, but we don't want to make
        // the default mount protocol fuse. We can fall through to parsing a
        // fuse mount in this case.
        [[fallthrough]];
      case TakeoverMountProtocol::FUSE:
        checkCanSerDeMountType(
            protocolCapabilities,
            TakeoverMountProtocol::FUSE,
            *serializedMount.mountPath());
        data.mountPoints.emplace_back(
            canonicalPath(*serializedMount.mountPath()),
            canonicalPath(*serializedMount.stateDirectory()),
            FuseChannelData{
                folly::File{},
                *reinterpret_cast<const fuse_init_out*>(
                    serializedMount.connInfo()->data())},
            std::move(*serializedMount.inodeMap()));
        break;
      case TakeoverMountProtocol::NFS:
        checkCanSerDeMountType(
            protocolCapabilities,
            TakeoverMountProtocol::NFS,
            *serializedMount.mountPath());
        data.mountPoints.emplace_back(
            canonicalPath(*serializedMount.mountPath()),
            canonicalPath(*serializedMount.stateDirectory()),
            NfsChannelData{folly::File{}},
            std::move(*serializedMount.inodeMap()));
        break;
      default:
        throw std::runtime_error(
            "impossible enum variant for TakeoverMountProtocol");
    }
  }
  return data;
}
#endif

} // namespace facebook::eden
