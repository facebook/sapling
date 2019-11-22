/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/takeover/TakeoverData.h"

#include <folly/Format.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/utils/Bug.h"

using apache::thrift::CompactSerializer;
using folly::IOBuf;
using std::string;

namespace facebook {
namespace eden {

const std::set<int32_t> kSupportedTakeoverVersions{
    TakeoverData::kTakeoverProtocolVersionOne,
    TakeoverData::kTakeoverProtocolVersionThree};

std::optional<int32_t> TakeoverData::computeCompatibleVersion(
    const std::set<int32_t>& versions,
    const std::set<int32_t>& supported) {
  std::optional<int32_t> best;

  for (auto& version : versions) {
    if (best.has_value() && best.value() > version) {
      // No better than the current best
      continue;
    }
    if (supported.find(version) == supported.end()) {
      // Not supported
      continue;
    }
    best = version;
  }
  return best;
}

IOBuf TakeoverData::serialize(int32_t protocolVersion) {
  switch (protocolVersion) {
    case kTakeoverProtocolVersionOne:
      return serializeVersion1();
    case kTakeoverProtocolVersionThree:
      return serializeVersion3();
    default: {
      EDEN_BUG()
          << "only kTakeoverProtocolVersionOne is supported, but somehow "
          << "we were asked to handle version " << protocolVersion;
    }
  }
}

folly::IOBuf TakeoverData::serializeError(
    int32_t protocolVersion,
    const folly::exception_wrapper& ew) {
  switch (protocolVersion) {
    // We allow NeverSupported in the error case so that we don't
    // end up EDEN_BUG'ing out in the version mismatch error
    // reporting case.
    case kTakeoverProtocolVersionNeverSupported:
    case kTakeoverProtocolVersionOne:
      return serializeErrorVersion1(ew);
    case kTakeoverProtocolVersionThree:
      return serializeErrorVersion3(ew);
    default: {
      EDEN_BUG()
          << "only kTakeoverProtocolVersionOne is supported, but somehow "
          << "we were asked to handle version " << protocolVersion;
    }
  }
}

TakeoverData TakeoverData::deserialize(IOBuf* buf) {
  // We need to probe the data to see which version we have
  folly::io::Cursor cursor(buf);

  auto messageType = cursor.readBE<uint32_t>();
  switch (messageType) {
    case MessageType::ERROR:
    case MessageType::MOUNTS:
      // A version 1 response.  We don't advance the buffer that we pass down
      // because it the messageType is needed to decode the response.
      return deserializeVersion1(buf);
    case kTakeoverProtocolVersionThree:
      // Version 3 (there was no 2 because of how Version 1 used word values
      // 1 and 2) doesn't care about this version byte, so we skip past it
      // and let the underlying code decode the data
      buf->trimStart(sizeof(uint32_t));
      return deserializeVersion3(buf);
    default:
      throw std::runtime_error(folly::sformat(
          "Unrecognized TakeoverData response starting with {:x}",
          messageType));
  }
}

IOBuf TakeoverData::serializeVersion1() {
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
    app.push(folly::StringPiece{reinterpret_cast<const char*>(&mount.connInfo),
                                sizeof(mount.connInfo)});
    // SerializedFileHandleMap has been removed so its size is always 0.
    app.writeBE<uint32_t>(0);

    auto serializedInodeMap =
        CompactSerializer::serialize<std::string>(mount.inodeMap);
    app.writeBE<uint32_t>(serializedInodeMap.size());
    app.push(folly::StringPiece{serializedInodeMap});
  }

  return buf;
}

folly::IOBuf TakeoverData::serializeErrorVersion1(
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

TakeoverData TakeoverData::deserializeVersion1(IOBuf* buf) {
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
        folly::File{},
        connInfo,
        std::move(inodeMap));
  }

  return data;
}

IOBuf TakeoverData::serializeVersion3() {
  SerializedTakeoverData serialized;

  folly::IOBufQueue bufQ;
  folly::io::QueueAppender app(&bufQ, 0);

  // First word is the protocol version
  app.writeBE<uint32_t>(kTakeoverProtocolVersionThree);

  std::vector<SerializedMountInfo> serializedMounts;
  for (const auto& mount : mountPoints) {
    SerializedMountInfo serializedMount;

    serializedMount.mountPath = mount.mountPath.stringPiece().str();
    serializedMount.stateDirectory = mount.stateDirectory.stringPiece().str();

    for (const auto& bindMount : mount.bindMounts) {
      serializedMount.bindMountPaths.push_back(bindMount.stringPiece().str());
    }

    // Stuffing the fuse connection information in as a binary
    // blob because we know that the endianness of the target
    // machine must match the current system for a graceful
    // takeover, and it saves us from re-encoding an operating
    // system specific struct into a thrift file.
    serializedMount.connInfo = std::string{
        reinterpret_cast<const char*>(&mount.connInfo), sizeof(mount.connInfo)};

    serializedMount.inodeMap = mount.inodeMap;

    serializedMounts.emplace_back(std::move(serializedMount));
  }

  serialized.set_mounts(std::move(serializedMounts));

  CompactSerializer::serialize(serialized, &bufQ);
  return std::move(*bufQ.move());
}

folly::IOBuf TakeoverData::serializeErrorVersion3(
    const folly::exception_wrapper& ew) {
  SerializedTakeoverData serialized;
  auto exceptionClassName = ew.class_name();
  folly::StringPiece what = ew ? ew.get_exception()->what() : "";
  serialized.set_errorReason(
      folly::to<std::string>(exceptionClassName, ": ", what));

  folly::IOBufQueue bufQ;
  folly::io::QueueAppender app(&bufQ, 0);

  // First word is the protocol version
  app.writeBE<uint32_t>(kTakeoverProtocolVersionThree);

  CompactSerializer::serialize(serialized, &bufQ);
  return std::move(*bufQ.move());
}

TakeoverData TakeoverData::deserializeVersion3(IOBuf* buf) {
  auto serialized = CompactSerializer::deserialize<SerializedTakeoverData>(buf);

  switch (serialized.getType()) {
    case SerializedTakeoverData::Type::errorReason:
      throw std::runtime_error(serialized.get_errorReason());

    case SerializedTakeoverData::Type::mounts: {
      TakeoverData data;
      for (auto& serializedMount : serialized.mutable_mounts()) {
        const auto* connInfo = reinterpret_cast<const fuse_init_out*>(
            serializedMount.connInfo.data());

        std::vector<AbsolutePath> bindMounts;
        for (const auto& path : serializedMount.bindMountPaths) {
          bindMounts.emplace_back(AbsolutePathPiece{path});
        }

        data.mountPoints.emplace_back(
            AbsolutePath{serializedMount.mountPath},
            AbsolutePath{serializedMount.stateDirectory},
            std::move(bindMounts),
            folly::File{},
            *connInfo,
            std::move(serializedMount.inodeMap));
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
