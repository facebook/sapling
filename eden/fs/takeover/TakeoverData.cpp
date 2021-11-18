/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/takeover/TakeoverData.h"

#include <memory>
#include <stdexcept>
#include <variant>

#include <folly/Format.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/logging/xlog.h>

#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/utils/Bug.h"
#include "eden/fs/utils/UnixSocket.h"

using apache::thrift::CompactSerializer;
using folly::IOBuf;
using std::string;

namespace facebook {
namespace eden {

namespace {

/**
 * Determines the mount protocol for the mount point encoded in the mountInfo.
 */
TakeoverMountProtocol getMountProtocol(
    const TakeoverData::MountInfo& mountInfo) {
  if (std::holds_alternative<FuseChannelData>(mountInfo.channelInfo)) {
    return TakeoverMountProtocol::FUSE;
  } else if (std::holds_alternative<NfsChannelData>(mountInfo.channelInfo)) {
    return TakeoverMountProtocol::NFS;
  }
  throw std::runtime_error(fmt::format(
      "unrecognized mount protocol {} for mount: {}",
      mountInfo.channelInfo.index(),
      mountInfo.mountPath));
}

} // namespace

const std::set<int32_t> kSupportedTakeoverVersions{
    TakeoverData::kTakeoverProtocolVersionOne,
    TakeoverData::kTakeoverProtocolVersionThree,
    TakeoverData::kTakeoverProtocolVersionFour,
    TakeoverData::kTakeoverProtocolVersionFive};

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

uint64_t TakeoverData::versionToCapabilites(int32_t version) {
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
  }
  throw std::runtime_error(fmt::format("Unsupported version: {}", version));
}

int32_t TakeoverData::capabilitesToVersion(uint64_t capabilities) {
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

  throw std::runtime_error(
      fmt::format("Unsupported combination of capabilities: {}", capabilities));
}

bool TakeoverData::shouldSerdeNFSInfo(uint32_t protocolCapabilities) {
  // 4 and below know nothing of NFS mounts. we introduce NFS support in version
  // 5 and expect to continue to support NFS mounts beyond version 5.
  return protocolCapabilities & TakeoverCapabilities::NFS;
}

void TakeoverData::serialize(
    uint64_t protocolCapabilities,
    UnixSocket::Message& msg) {
  msg.data = serialize(protocolCapabilities);
  msg.files.push_back(std::move(lockFile));
  msg.files.push_back(std::move(thriftSocket));

  if (shouldSerdeNFSInfo(protocolCapabilities)) {
    XLOG(DBG7) << "serializing mountd socket: " << mountdServerSocket.fd();
    msg.files.push_back(std::move(mountdServerSocket));
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
  uint64_t serializationMethod = protocolCapabilities &
      (TakeoverCapabilities::CUSTOM_SERIALIZATION |
       TakeoverCapabilities::THRIFT_SERIALIZATION);

  if (serializationMethod == TakeoverCapabilities::CUSTOM_SERIALIZATION) {
    return serializeCustom();
  } else if (
      serializationMethod == TakeoverCapabilities::THRIFT_SERIALIZATION) {
    return serializeThrift(protocolCapabilities);
  } else {
    throw std::runtime_error(fmt::format(
        "Asked to serialize takeover data in unsupported format. "
        "Cababilities: {}",
        protocolCapabilities));
  }
}

folly::IOBuf TakeoverData::serializeError(
    uint64_t protocolCapabilities,
    const folly::exception_wrapper& ew) {
  uint64_t serializationMethod = protocolCapabilities &
      (TakeoverCapabilities::CUSTOM_SERIALIZATION |
       TakeoverCapabilities::THRIFT_SERIALIZATION);

  // We allow NeverSupported in the error case so that we don't
  // end up erroring out in the version mismatch error
  // reporting case.
  if (serializationMethod == TakeoverCapabilities::CUSTOM_SERIALIZATION ||
      protocolCapabilities == 0) {
    return serializeErrorCustom(ew);
  } else if (
      serializationMethod == TakeoverCapabilities::THRIFT_SERIALIZATION) {
    return serializeErrorThrift(ew);
  } else {
    throw std::runtime_error(fmt::format(
        "Asked to serialize takeover error in unsupported format. "
        "Capabilities: {}",
        protocolCapabilities));
  }
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
  return buf;
}

TakeoverData TakeoverData::deserialize(UnixSocket::Message& msg) {
  auto protocolVersion = TakeoverData::getProtocolVersion(&msg.data);
  auto capabilities = TakeoverData::versionToCapabilites(protocolVersion);

  auto data = TakeoverData::deserialize(capabilities, &msg.data);
  // when we serialize the mountd socket we have three general files instead
  // of two
  const auto mountPointFilesOffset = shouldSerdeNFSInfo(capabilities) ? 3 : 2;

  // Add 2 here for the lock file and the thrift socket
  if (data.mountPoints.size() + mountPointFilesOffset != msg.files.size()) {
    throw std::runtime_error(folly::to<string>(
        "received ",
        data.mountPoints.size(),
        " mount paths, but ",
        msg.files.size(),
        " FDs (including the lock file FD)"));
  }
  data.lockFile = std::move(msg.files[0]);
  data.thriftSocket = std::move(msg.files[1]);
  if (shouldSerdeNFSInfo(capabilities)) {
    data.mountdServerSocket = std::move(msg.files[2]);
    XLOG(DBG1) << "Deserialized mountd Socket " << data.mountdServerSocket.fd();
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

int32_t TakeoverData::getProtocolVersion(IOBuf* buf) {
  // We need to probe the data to see which version we have
  folly::io::Cursor cursor(buf);

  auto messageType = cursor.readBE<uint32_t>();
  switch (messageType) {
    case MessageType::ERROR:
    case MessageType::MOUNTS:
      // A version 1 response.  We don't advance the buffer that we pass down
      // because it the messageType is needed to decode the response.
      return kTakeoverProtocolVersionOne;
    case kTakeoverProtocolVersionThree:
    case kTakeoverProtocolVersionFour:
    case kTakeoverProtocolVersionFive:
      // Version 3 (there was no 2 because of how Version 1 used word values
      // 1 and 2) doesn't care about this version byte, so we skip past it
      // and let the underlying code decode the data
      buf->trimStart(sizeof(uint32_t));
      return messageType;
    default:
      throw std::runtime_error(fmt::format(
          "Unrecognized TakeoverData response starting with {:x}",
          messageType));
  }
}

TakeoverData TakeoverData::deserialize(
    uint64_t protocolCapabilities,
    IOBuf* buf) {
  uint64_t serializationMethod = protocolCapabilities &
      (TakeoverCapabilities::CUSTOM_SERIALIZATION |
       TakeoverCapabilities::THRIFT_SERIALIZATION);
  if (serializationMethod == TakeoverCapabilities::CUSTOM_SERIALIZATION) {
    return deserializeCustom(buf);
  }
  if (serializationMethod == TakeoverCapabilities::THRIFT_SERIALIZATION) {
    return deserializeThrift(protocolCapabilities, buf);
  }

  throw std::runtime_error(fmt::format(
      "Unrecognized TakeoverData serialization capability {:x}",
      protocolCapabilities));
}

IOBuf TakeoverData::serializeCustom() {
  // Compute the body data length
  uint64_t bodyLength = sizeof(uint32_t);
  for (const auto& mount : mountPoints) {
    bodyLength += sizeof(uint32_t) + mount.mountPath.stringPiece().size();
    bodyLength += sizeof(uint32_t) + mount.stateDirectory.stringPiece().size();
    bodyLength += sizeof(uint32_t);
    for (const auto& bindMount : mount.bindMounts) {
      bodyLength += sizeof(uint32_t) + bindMount.stringPiece().size();
    }
    bodyLength += sizeof(fuse_init_out);

    // The fileHandleMap has been removed, so its size will always be 0.
    constexpr size_t fileHandleMapSize = 0;
    bodyLength += sizeof(uint32_t) + fileHandleMapSize;

    auto serializedInodeMap =
        CompactSerializer::serialize<std::string>(mount.inodeMap);
    bodyLength += sizeof(uint32_t) + serializedInodeMap.size();
  }

  // Build a buffer with all of the mount paths
  auto fullCapacity = kHeaderLength + bodyLength;
  IOBuf buf(IOBuf::CREATE, fullCapacity);
  folly::io::Appender app(&buf, 0);

  // Serialize the message type
  app.writeBE<uint32_t>(MessageType::MOUNTS);

  // Write the number of mount points
  app.writeBE<uint32_t>(mountPoints.size());

  // Serialize each mount point
  for (const auto& mount : mountPoints) {
    auto mountProtocol = getMountProtocol(mount);
    if (mountProtocol == TakeoverMountProtocol::FUSE) {
      auto& channelData = std::get<FuseChannelData>(mount.channelInfo);

      // The mount path
      const auto& pathStr = mount.mountPath.stringPiece();
      app.writeBE<uint32_t>(pathStr.size());
      app(pathStr);

      // The client configuration dir
      const auto& clientStr = mount.stateDirectory.stringPiece();
      app.writeBE<uint32_t>(clientStr.size());
      app(clientStr);

      // Number of bind mounts, followed by the bind mount paths
      app.writeBE<uint32_t>(mount.bindMounts.size());
      for (const auto& bindMount : mount.bindMounts) {
        app.writeBE<uint32_t>(bindMount.stringPiece().size());
        app(bindMount.stringPiece());
      }

      // Stuffing the fuse connection information in as a binary
      // blob because we know that the endianness of the target
      // machine must match the current system for a graceful
      // takeover.
      app.push(folly::StringPiece{
          reinterpret_cast<const char*>(&channelData.connInfo),
          sizeof(channelData.connInfo)});
      // SerializedFileHandleMap has been removed so its size is always 0.
      app.writeBE<uint32_t>(0);

      auto serializedInodeMap =
          CompactSerializer::serialize<std::string>(mount.inodeMap);
      app.writeBE<uint32_t>(serializedInodeMap.size());
      app.push(folly::StringPiece{serializedInodeMap});
    } else {
      throw std::runtime_error(fmt::format(
          "version 1 of the protocol does not support serializing non-FUSE"
          "mounts. problem mount: {} . protocol: {}",
          mount.mountPath,
          mountProtocol));
    }
  }

  return buf;
}

folly::IOBuf TakeoverData::serializeErrorCustom(
    const folly::exception_wrapper& ew) {
  // Compute the body data length
  auto exceptionClassName = ew.class_name();
  folly::StringPiece what = ew ? ew.get_exception()->what() : "";
  uint64_t bodyLength = sizeof(uint32_t) + exceptionClassName.size() +
      sizeof(uint32_t) + what.size();

  // Allocate the buffer
  auto fullCapacity = kHeaderLength + bodyLength;
  IOBuf buf(IOBuf::CREATE, fullCapacity);
  folly::io::Appender app(&buf, 0);

  // Serialize the message type
  app.writeBE<uint32_t>(MessageType::ERROR);

  // Write the error type and message
  app.writeBE<uint32_t>(exceptionClassName.size());
  app(exceptionClassName);
  app.writeBE<uint32_t>(what.size());
  app(what);

  return buf;
}

TakeoverData TakeoverData::deserializeCustom(IOBuf* buf) {
  folly::io::Cursor cursor(buf);

  auto messageType = cursor.readBE<uint32_t>();
  if (messageType != MessageType::ERROR && messageType != MessageType::MOUNTS) {
    throw std::runtime_error(
        folly::to<string>("unknown takeover data message type ", messageType));
  }

  // Check the message type
  if (messageType == MessageType::ERROR) {
    auto errorTypeLength = cursor.readBE<uint32_t>();
    auto errorType = cursor.readFixedString(errorTypeLength);
    auto errorMessageLength = cursor.readBE<uint32_t>();
    auto errorMessage = cursor.readFixedString(errorMessageLength);

    throw std::runtime_error(errorType + ": " + errorMessage);
  }
  if (messageType != MessageType::MOUNTS) {
    throw std::runtime_error(
        folly::to<string>("unknown takeover data message type ", messageType));
  }

  TakeoverData data;
  auto numMounts = cursor.readBE<uint32_t>();
  for (uint32_t mountIdx = 0; mountIdx < numMounts; ++mountIdx) {
    auto pathLength = cursor.readBE<uint32_t>();
    auto mountPath = cursor.readFixedString(pathLength);

    auto clientPathLength = cursor.readBE<uint32_t>();
    auto stateDirectory = cursor.readFixedString(clientPathLength);

    auto numBindMounts = cursor.readBE<uint32_t>();

    std::vector<AbsolutePath> bindMounts;
    bindMounts.reserve(numBindMounts);
    for (uint32_t bindIdx = 0; bindIdx < numBindMounts; ++bindIdx) {
      auto bindPathLength = cursor.readBE<uint32_t>();
      auto bindPath = cursor.readFixedString(bindPathLength);
      bindMounts.emplace_back(AbsolutePathPiece{bindPath});
    }

    fuse_init_out connInfo;
    cursor.pull(&connInfo, sizeof(connInfo));

    auto fileHandleMapLength = cursor.readBE<uint32_t>();
    cursor.readFixedString(fileHandleMapLength);
    // No need to decode the file handle map.

    auto inodeMapLength = cursor.readBE<uint32_t>();
    auto inodeMapBuffer = cursor.readFixedString(inodeMapLength);
    auto inodeMap =
        CompactSerializer::deserialize<SerializedInodeMap>(inodeMapBuffer);

    data.mountPoints.emplace_back(
        AbsolutePath{mountPath},
        AbsolutePath{stateDirectory},
        std::move(bindMounts),
        FuseChannelData{folly::File{}, connInfo},
        std::move(inodeMap));
  }

  return data;
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
    throw std::runtime_error(fmt::format(
        "protocol does not support serializing/deserializing this type of "
        "mounts. protocol capabilities: {}. problem mount: {}. mount protocol:"
        " {}",
        protocolCapabilities,
        mountPath,
        mountProtocol));
  }
}

IOBuf TakeoverData::serializeThrift(uint64_t protocolCapabilities) {
  SerializedTakeoverData serialized;

  folly::IOBufQueue bufQ;
  folly::io::QueueAppender app(&bufQ, 0);

  { // we scope this to avoid using the version any further in the code.
    // Ideally we would only use capabilities, but we need to send version
    // numbers to be compatible with older version.
    int32_t versionToAdvertize = capabilitesToVersion(protocolCapabilities);
    // first word is the protocol version. previous versions of EdenFS do not
    // know how to deserialize version 4 because they assume that protocol 4
    // uses protocol 3 serialization. We need to do this funkiness for rollback
    // safety.
    if (versionToAdvertize == kTakeoverProtocolVersionFour) {
      versionToAdvertize = kTakeoverProtocolVersionThree;
    }
    app.writeBE<uint32_t>(versionToAdvertize);
  }

  std::vector<SerializedMountInfo> serializedMounts;
  for (const auto& mount : mountPoints) {
    auto mountProtocol = getMountProtocol(mount);

    checkCanSerDeMountType(
        protocolCapabilities, mountProtocol, mount.mountPath.stringPiece());

    SerializedMountInfo serializedMount;

    *serializedMount.mountPath_ref() = mount.mountPath.stringPiece().str();
    *serializedMount.stateDirectory_ref() =
        mount.stateDirectory.stringPiece().str();

    for (const auto& bindMount : mount.bindMounts) {
      serializedMount.bindMountPaths_ref()->push_back(
          bindMount.stringPiece().str());
    }

    if (auto fuseChannelInfo =
            std::get_if<FuseChannelData>(&mount.channelInfo)) {
      // Stuffing the fuse connection information in as a binary
      // blob because we know that the endianness of the target
      // machine must match the current system for a graceful
      // takeover, and it saves us from re-encoding an operating
      // system specific struct into a thrift file.
      serializedMount.connInfo_ref() = std::string{
          reinterpret_cast<const char*>(&fuseChannelInfo->connInfo),
          sizeof(fuseChannelInfo->connInfo)};
    }

    *serializedMount.inodeMap_ref() = mount.inodeMap;

    serializedMount.mountProtocol_ref() = mountProtocol;

    serializedMounts.emplace_back(std::move(serializedMount));
  }

  serialized.mounts_ref() = std::move(serializedMounts);

  CompactSerializer::serialize(serialized, &bufQ);
  return std::move(*bufQ.move());
}

folly::IOBuf TakeoverData::serializeErrorThrift(
    const folly::exception_wrapper& ew) {
  SerializedTakeoverData serialized;
  auto exceptionClassName = ew.class_name();
  folly::StringPiece what = ew ? ew.get_exception()->what() : "";
  serialized.errorReason_ref() =
      folly::to<std::string>(exceptionClassName, ": ", what);

  folly::IOBufQueue bufQ;
  folly::io::QueueAppender app(&bufQ, 0);

  // First word is the protocol version
  app.writeBE<uint32_t>(kTakeoverProtocolVersionThree);

  CompactSerializer::serialize(serialized, &bufQ);
  return std::move(*bufQ.move());
}

TakeoverData TakeoverData::deserializeThrift(
    uint32_t protocolCapabilities,
    IOBuf* buf) {
  auto serialized = CompactSerializer::deserialize<SerializedTakeoverData>(buf);

  switch (serialized.getType()) {
    case SerializedTakeoverData::Type::errorReason:
      throw std::runtime_error(serialized.get_errorReason());

    case SerializedTakeoverData::Type::mounts: {
      TakeoverData data;
      for (auto& serializedMount : serialized.mutable_mounts()) {
        std::vector<AbsolutePath> bindMounts;
        for (const auto& path : *serializedMount.bindMountPaths_ref()) {
          bindMounts.emplace_back(AbsolutePathPiece{path});
        }
        switch (*serializedMount.mountProtocol_ref()) {
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
                *serializedMount.mountPath_ref());
            data.mountPoints.emplace_back(
                AbsolutePath{*serializedMount.mountPath_ref()},
                AbsolutePath{*serializedMount.stateDirectory_ref()},
                std::move(bindMounts),
                FuseChannelData{
                    folly::File{},
                    *reinterpret_cast<const fuse_init_out*>(
                        serializedMount.connInfo_ref()->data())},
                std::move(*serializedMount.inodeMap_ref()));
            break;
          case TakeoverMountProtocol::NFS:
            checkCanSerDeMountType(
                protocolCapabilities,
                TakeoverMountProtocol::NFS,
                *serializedMount.mountPath_ref());
            data.mountPoints.emplace_back(
                AbsolutePath{*serializedMount.mountPath_ref()},
                AbsolutePath{*serializedMount.stateDirectory_ref()},
                std::move(bindMounts),
                NfsChannelData{folly::File{}},
                std::move(*serializedMount.inodeMap_ref()));
            break;
          default:
            throw std::runtime_error(
                "impossible enum variant for TakeoverMountProtocol");
        }
      }
      return data;
    }
    case SerializedTakeoverData::Type::__EMPTY__:
      // This case triggers when there are no mounts to pass between
      // the processes; we allow for it here and return an empty
      // TakeoverData instance.
      return TakeoverData{};
  }
  throw std::runtime_error(
      "impossible enum variant for SerializedTakeoverData");
}

} // namespace eden
} // namespace facebook

#endif
