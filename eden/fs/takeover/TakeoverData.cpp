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

using folly::IOBuf;
using std::string;

namespace facebook {
namespace eden {

constexpr uint32_t TakeoverData::kMagicNumber;

std::unique_ptr<IOBuf> TakeoverData::serialize() {
  // Compute the body data length
  uint64_t bodyLength = sizeof(uint32_t);
  for (const auto& mount : mountPoints) {
    bodyLength += sizeof(uint32_t) + mount.path.stringPiece().size();
    bodyLength += sizeof(uint32_t);
    for (const auto& bindMount : mount.bindMounts) {
      bodyLength += sizeof(uint32_t) + bindMount.stringPiece().size();
    }
  }

  // Build a buffer with all of the mount paths
  auto fullCapacity = kHeaderLength + bodyLength;
  auto buf = IOBuf::create(fullCapacity);
  folly::io::Appender app(buf.get(), 0);

  // Serialize the header data
  app.writeBE<uint32_t>(kMagicNumber);
  app.writeBE<uint32_t>(MessageType::MOUNTS);
  app.writeBE<uint64_t>(bodyLength);

  // Write the number of mount points
  app.writeBE<uint32_t>(mountPoints.size());

  // Serialize each mount point
  for (const auto& mount : mountPoints) {
    // The mount path
    const auto& pathStr = mount.path.stringPiece();
    app.writeBE<uint32_t>(pathStr.size());
    app(pathStr);
    // Number of bind mounts, followed by the bind mount paths
    app.writeBE<uint32_t>(mount.bindMounts.size());
    for (const auto& bindMount : mount.bindMounts) {
      app.writeBE<uint32_t>(bindMount.stringPiece().size());
      app(bindMount.stringPiece());
    }
  }

  return buf;
}

std::unique_ptr<folly::IOBuf> TakeoverData::serializeError(
    const folly::exception_wrapper& ew) {
  // Compute the body data length
  auto exceptionClassName = ew.class_name();
  folly::StringPiece what = ew ? ew.get_exception()->what() : "";
  uint64_t bodyLength = sizeof(uint32_t) + exceptionClassName.size() +
      sizeof(uint32_t) + what.size();

  // Allocate the buffer
  auto fullCapacity = kHeaderLength + bodyLength;
  auto buf = IOBuf::create(fullCapacity);
  folly::io::Appender app(buf.get(), 0);

  // Serialize the header data
  app.writeBE<uint32_t>(kMagicNumber);
  app.writeBE<uint32_t>(MessageType::ERROR);
  app.writeBE<uint64_t>(bodyLength);

  // Write the error type and message
  app.writeBE<uint32_t>(exceptionClassName.size());
  app(exceptionClassName);
  app.writeBE<uint32_t>(what.size());
  app(what);

  return buf;
}

TakeoverData::HeaderInfo TakeoverData::deserializeHeader(const IOBuf* buf) {
  DCHECK_EQ(buf->computeChainDataLength(), kHeaderLength);
  folly::io::Cursor cursor(buf);

  HeaderInfo header;
  auto magic = cursor.readBE<uint32_t>();
  if (magic != kMagicNumber) {
    throw std::runtime_error(folly::sformat(
        "unexpected takeover data serialization ID {:#x}", magic));
  }
  header.messageType = cursor.readBE<uint32_t>();
  if (header.messageType != MessageType::ERROR &&
      header.messageType != MessageType::MOUNTS) {
    throw std::runtime_error(folly::to<string>(
        "unknown takeover data message type ", header.messageType));
  }
  header.bodyLength = cursor.readBE<uint64_t>();

  return header;
}

TakeoverData TakeoverData::deserializeBody(
    const HeaderInfo& header,
    const IOBuf* buf) {
  folly::io::Cursor cursor(buf);

  // Check the message type
  if (header.messageType == MessageType::ERROR) {
    auto errorTypeLength = cursor.readBE<uint32_t>();
    auto errorType = cursor.readFixedString(errorTypeLength);
    auto errorMessageLength = cursor.readBE<uint32_t>();
    auto errorMessage = cursor.readFixedString(errorMessageLength);

    throw std::runtime_error(errorType + ": " + errorMessage);
  }
  if (header.messageType != MessageType::MOUNTS) {
    throw std::runtime_error(folly::to<string>(
        "unknown takeover data message type ", header.messageType));
  }

  TakeoverData data;

  auto numMounts = cursor.readBE<uint32_t>();
  for (uint32_t mountIdx = 0; mountIdx < numMounts; ++mountIdx) {
    auto pathLength = cursor.readBE<uint32_t>();
    auto path = cursor.readFixedString(pathLength);
    auto numBindMounts = cursor.readBE<uint32_t>();

    std::vector<AbsolutePath> bindMounts;
    bindMounts.reserve(numBindMounts);
    for (uint32_t bindIdx = 0; bindIdx < numBindMounts; ++bindIdx) {
      auto bindPathLength = cursor.readBE<uint32_t>();
      auto bindPath = cursor.readFixedString(bindPathLength);
      bindMounts.emplace_back(AbsolutePathPiece{bindPath});
    }

    data.mountPoints.emplace_back(
        AbsolutePath{path}, std::move(bindMounts), folly::File{});
  }

  return data;
}

} // namespace eden
} // namespace facebook
