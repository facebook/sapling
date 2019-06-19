/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/takeover/TakeoverClient.h"

#include <folly/io/Cursor.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/gen-cpp2/takeover_types.h"
#include "eden/fs/utils/FutureUnixSocket.h"

using apache::thrift::CompactSerializer;
using folly::IOBuf;
using std::string;

/**
 * Five minutes is a high default value.  This could be lowered back to one
 * minute after takeover no longer does O(loaded) expensive save operations.
 */
DEFINE_int32(
    takeoverReceiveTimeout,
    300,
    "Timeout for receiving takeover data from old process in seconds");

namespace facebook {
namespace eden {

TakeoverData takeoverMounts(
    AbsolutePathPiece socketPath,
    const std::set<int32_t>& supportedVersions) {
  folly::EventBase evb;
  folly::Expected<UnixSocket::Message, folly::exception_wrapper>
      expectedMessage;

  auto connectTimeout = std::chrono::seconds(1);
  FutureUnixSocket socket;
  socket.connect(&evb, socketPath.stringPiece(), connectTimeout)
      .thenValue([&socket, supportedVersions](auto&&) {
        // Send our protocol version so that the server knows
        // whether we're capable of handshaking successfully

        TakeoverVersionQuery query;
        query.versions = supportedVersions;

        return socket.send(
            CompactSerializer::serialize<folly::IOBufQueue>(query).move());
      })
      .thenValue([&socket](auto&&) {
        // Wait for the takeover data response
        auto timeout = std::chrono::seconds(FLAGS_takeoverReceiveTimeout);
        return socket.receive(timeout);
      })
      .thenValue([&expectedMessage](UnixSocket::Message&& msg) {
        expectedMessage = std::move(msg);
      })
      .thenError([&expectedMessage](folly::exception_wrapper&& ew) {
        expectedMessage = folly::makeUnexpected(std::move(ew));
      })
      .ensure([&evb] { evb.terminateLoopSoon(); });

  evb.loop();

  if (!expectedMessage) {
    XLOG(ERR) << "error receiving takeover data: " << expectedMessage.error();
    expectedMessage.error().throw_exception();
  }
  auto& message = expectedMessage.value();

  auto data = TakeoverData::deserialize(&message.data);
  // Add 2 here for the lock file and the thrift socket
  if (data.mountPoints.size() + 2 != message.files.size()) {
    throw std::runtime_error(folly::to<string>(
        "received ",
        data.mountPoints.size(),
        " mount paths, but ",
        message.files.size(),
        " FDs (including the lock file FD)"));
  }
  data.lockFile = std::move(message.files[0]);
  data.thriftSocket = std::move(message.files[1]);
  for (size_t n = 0; n < data.mountPoints.size(); ++n) {
    auto& mountInfo = data.mountPoints[n];
    mountInfo.fuseFD = std::move(message.files[n + 2]);
  }

  return data;
}
} // namespace eden
} // namespace facebook
