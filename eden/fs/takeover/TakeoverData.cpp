/*
 *  Copyright (c) 2004-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/takeover/TakeoverData.h"

#include <folly/Format.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

using folly::IOBuf;
using std::string;
using apache::thrift::CompactSerializer;

namespace facebook {
namespace eden {

IOBuf TakeoverData::serialize() {
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

    // TODO: remove this redundant serialization.  We're doing this here
    // solely to compute the size of the data.  We plan to refactor this
    // whole thing to use a single thrift struct; T25009883
    auto serializedFileHandleMap =
        CompactSerializer::serialize<std::string>(mount.fileHandleMap);
    bodyLength += sizeof(uint32_t) + serializedFileHandleMap.size();
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

    auto serializedFileHandleMap =
        CompactSerializer::serialize<std::string>(mount.fileHandleMap);
    app.writeBE<uint32_t>(serializedFileHandleMap.size());
    app.push(folly::StringPiece{serializedFileHandleMap});
  }

  return buf;
}

folly::IOBuf TakeoverData::serializeError(const folly::exception_wrapper& ew) {
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

TakeoverData TakeoverData::deserialize(const IOBuf* buf) {
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
    auto fileHandleMapBuffer = cursor.readFixedString(fileHandleMapLength);
    auto fileHandleMap =
        CompactSerializer::deserialize<SerializedFileHandleMap>(
            fileHandleMapBuffer);

    data.mountPoints.emplace_back(
        AbsolutePath{mountPath},
        AbsolutePath{stateDirectory},
        std::move(bindMounts),
        folly::File{},
        connInfo,
        std::move(fileHandleMap));
  }

  return data;
}

} // namespace eden
} // namespace facebook
