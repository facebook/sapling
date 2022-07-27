/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/takeover/TakeoverClient.h"

#include <folly/io/Cursor.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/gen-cpp2/takeover_types.h"
#include "eden/fs/utils/FutureUnixSocket.h"

using apache::thrift::CompactSerializer;
using std::string;

/**
 * Five minutes is a high default value.  This could be lowered back to one
 * minute after takeover no longer does O(loaded) expensive save operations.
 */
DEFINE_int32(
    takeoverReceiveTimeout,
    300,
    "Timeout for receiving takeover data from old process in seconds");

namespace facebook::eden {

TakeoverData takeoverMounts(
    AbsolutePathPiece socketPath,
    bool shouldPing,
    const std::set<int32_t>& supportedVersions,
    const uint64_t supportedTakeoverCapabilities) {
  folly::EventBase evb;
  folly::Expected<UnixSocket::Message, folly::exception_wrapper>
      expectedMessage;

  auto connectTimeout = std::chrono::seconds(1);
  FutureUnixSocket socket;
  socket.connect(&evb, socketPath.stringPiece(), connectTimeout)
      .thenValue(
          [&socket, supportedVersions, supportedTakeoverCapabilities](auto&&) {
            // Send our protocol version so that the server knows
            // whether we're capable of handshaking successfully

            TakeoverVersionQuery query;
            query.versions_ref() = supportedVersions;
            query.capabilities_ref() = supportedTakeoverCapabilities;

            return socket.send(
                CompactSerializer::serialize<folly::IOBufQueue>(query).move());
          })
      .thenValue([&socket](auto&&) {
        // Wait for a response. this will either be a "ready" ping or the
        // takeover data depending on the server protocol
        auto timeout = std::chrono::seconds(FLAGS_takeoverReceiveTimeout);
        return socket.receive(timeout);
      })
      .thenValue([&socket, shouldPing](UnixSocket::Message&& msg) {
        if (TakeoverData::isPing(&msg.data)) {
          if (shouldPing) {
            // Just send an empty message back here, the server knows it sent a
            // ping so it does not need to parse the message.
            UnixSocket::Message ping;
            return socket.send(std::move(ping)).thenValue([&socket](auto&&) {
              // Wait for the takeover data response
              auto timeout = std::chrono::seconds(FLAGS_takeoverReceiveTimeout);
              return socket.receive(timeout);
            });
          } else {
            // This should only be hit during integration tests.
            return folly::makeFuture<UnixSocket::Message>(
                folly::exception_wrapper(std::runtime_error(
                    "ping received but should not respond")));
          }
        } else {
          // Older versions of EdenFS will not send a "ready" ping and
          // could simply send the takeover data.
          return folly::makeFuture<UnixSocket::Message>(std::move(msg));
        }
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
  for (auto& file : message.files) {
    XLOG(DBG7) << "received fd for takeover: " << file.fd();
  }

  return TakeoverData::deserialize(message);
}
} // namespace facebook::eden

#endif
