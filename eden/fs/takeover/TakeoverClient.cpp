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
#include "eden/common/utils/FaultInjector.h"
#include "eden/common/utils/FutureUnixSocket.h"
#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/gen-cpp2/takeover_types.h"

using apache::thrift::CompactSerializer;
using std::string;

namespace facebook::eden {

folly::Future<UnixSocket::Message> receiveTakeoverDataMessage(
    FutureUnixSocket& socket,
    UnixSocket::Message&& msg,
    const std::chrono::seconds& takeoverReceiveTimeout) {
  // Read the rest of the data from socket.
  auto timeout = std::chrono::seconds(takeoverReceiveTimeout);
  return socket.receive(timeout).thenValue(
      [msg = std::move(msg), &socket, &takeoverReceiveTimeout](
          UnixSocket::Message&& nextMsg) mutable {
        if (TakeoverData::isLastChunk(&nextMsg.data)) {
          XLOGF(DBG7, "Client received the last chunk msg");
          // We have all the msg chunks. Now we can connect all the msg data
          // chains together
          msg.data.coalesce();
          return folly::makeFuture<UnixSocket::Message>(std::move(msg));
        } else {
          XLOGF(DBG7, "Client received a new chunk msg");
          // Append the next data chunk to the end of the current msg chain.
          // clone() creates a new heap allocated IOBuf that points to the same
          // underlying buffer. This underlying buffer is reference-counted and
          // will remain alive as long as there is at least one IOBuf
          // referencing it.
          msg.data.appendToChain(nextMsg.data.clone());
          // these chunks do not have file descriptors, so we can ignore them
          return receiveTakeoverDataMessage(
              socket, std::move(msg), takeoverReceiveTimeout);
        }
      });
}

TakeoverData takeoverMounts(
    AbsolutePathPiece socketPath,
    const std::chrono::seconds& takeoverReceiveTimeout,
    bool shouldThrowDuringTakeover,
    bool shouldPing,
    const std::set<int32_t>& supportedVersions,
    const uint64_t supportedTakeoverCapabilities) {
  folly::EventBase evb;
  folly::exception_wrapper expectedException;
  TakeoverData takeoverData;

  auto connectTimeout = std::chrono::seconds(1);
  FutureUnixSocket socket;
  socket.connect(&evb, socketPath.view(), connectTimeout)
      .thenValue(
          [&socket, supportedVersions, supportedTakeoverCapabilities](auto&&) {
            // Send our protocol version so that the server knows
            // whether we're capable of handshaking successfully

            TakeoverVersionQuery query;
            query.versions() = supportedVersions;
            query.capabilities() = supportedTakeoverCapabilities;

            return socket.send(
                CompactSerializer::serialize<folly::IOBufQueue>(query).move());
          })
      .thenValue([&socket, &takeoverReceiveTimeout](auto&&) {
        // Wait for a response. this will either be a "ready" ping or the
        // takeover data depending on the server protocol
        return socket.receive(takeoverReceiveTimeout);
      })
      .thenValue([&socket,
                  shouldPing,
                  shouldThrowDuringTakeover,
                  &takeoverReceiveTimeout](UnixSocket::Message&& msg) mutable {
        if (TakeoverData::isPing(&msg.data)) {
          if (shouldPing) {
            // Just send an empty message back here, the server knows it sent
            // a ping so it does not need to parse the message.
            UnixSocket::Message ping;
            return socket.send(std::move(ping))
                .thenValue([&socket,
                            shouldThrowDuringTakeover,
                            &takeoverReceiveTimeout](auto&&) mutable {
                  // Possibly simulate a takeover error during data transfer
                  // for testing purposes. While we would prefer to use
                  // fault injection here, it's not possible to inject an
                  // error into the TakeoverClient because the thrift server
                  // is not yet running.
                  if (shouldThrowDuringTakeover) {
                    // throw std::runtime_error("simulated takeover error");
                    return folly::makeFuture<UnixSocket::Message>(
                        folly::exception_wrapper(
                            std::runtime_error("simulated takeover error")));
                  }
                  // Wait for the takeover data response
                  return socket.receive(takeoverReceiveTimeout);
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
      .thenValue([&socket, &takeoverReceiveTimeout](UnixSocket::Message&& msg) {
        if (TakeoverData::isFirstChunk(&msg.data)) {
          // TakeoverData is sent in chunks. Receive the first chunk and
          // call a recursive function to receive the rest of the data.
          auto timeout = std::chrono::seconds(takeoverReceiveTimeout);
          return socket.receive(timeout).thenValue(
              [&socket,
               &takeoverReceiveTimeout](UnixSocket::Message&& msg) mutable {
                // Only the first chunk has the file descriptors. The rest of
                // the chunks will have empty msg.files
                return receiveTakeoverDataMessage(
                    socket, std::move(msg), takeoverReceiveTimeout);
              });
        } else {
          // Older versions of EdenFS will not send data in chunks
          return folly::makeFuture<UnixSocket::Message>(std::move(msg));
        }
      })
      .thenValue([&takeoverData](UnixSocket::Message&& msg) {
        for (auto& file : msg.files) {
          XLOGF(DBG7, "received fd for takeover: {}", file.fd());
        }
        takeoverData = TakeoverData::deserialize(msg);
      })
      .thenError([&expectedException](folly::exception_wrapper&& ew) {
        expectedException = std::move(ew);
      })
      .ensure([&evb] { evb.terminateLoopSoon(); });

  evb.loop();

  if (expectedException) {
    XLOGF(ERR, "error receiving takeover data: {}", expectedException.what());
    expectedException.throw_exception();
  }

  return takeoverData;
}
} // namespace facebook::eden

#endif
