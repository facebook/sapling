/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include "eden/fs/takeover/TakeoverServer.h"

#include <chrono>

#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventHandler.h>
#include <folly/logging/xlog.h>
#include <thrift/lib/cpp2/protocol/Serializer.h>

#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/utils/FutureUnixSocket.h"

using namespace std::chrono_literals;

using apache::thrift::CompactSerializer;
using folly::AsyncServerSocket;
using folly::checkUnixError;
using folly::exceptionStr;
using folly::Future;
using folly::makeFuture;
using folly::SocketAddress;
using folly::StringPiece;
using folly::Unit;
using std::make_unique;

namespace facebook {
namespace eden {

/**
 * ConnHandler handles a single connection received on the TakeoverServer
 * socket.
 */
class TakeoverServer::ConnHandler {
 public:
  ConnHandler(TakeoverServer* server, folly::File socket)
      : server_{server}, socket_{server_->getEventBase(), std::move(socket)} {}

  /**
   * start() begins processing data on this connection.
   *
   * Returns a Future that will complete successfully when this connection
   * finishes gracefully taking over the EdenServer's mount points.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> start() noexcept;

 private:
  FOLLY_NODISCARD folly::Future<folly::Unit> sendTakeoverData(
      folly::Try<TakeoverData>&& data);

  template <typename... Args>
  [[noreturn]] void fail(Args&&... args) {
    auto msg = folly::to<std::string>(std::forward<Args>(args)...);
    XLOG(ERR) << "takeover socket error: " << msg;
    throw std::runtime_error(msg);
  }

  TakeoverServer* const server_{nullptr};
  FutureUnixSocket socket_;
  int32_t protocolVersion_{
      TakeoverData::kTakeoverProtocolVersionNeverSupported};
};

Future<Unit> TakeoverServer::ConnHandler::start() noexcept {
  try {
    // Check the remote endpoint's credentials.
    // We only allow transferring our mount points to another process
    // owned by the same user.
    auto uid = socket_.getRemoteUID();
    if (uid != getuid()) {
      return makeFuture<Unit>(std::runtime_error(folly::to<std::string>(
          "invalid takeover request from incorrect user: current UID=",
          getuid(),
          ", got request from UID ",
          uid)));
    }

    // Check to see if we are speaking a compatible takeover protocol
    // version.  If not, error out so that we don't change any state.
    // The client should send us the version information, but clients
    // prior to the revision where this check was added will never send
    // us the version data.  We use a short timeout for receiving the
    // version data; in practice it will appear immediately or will
    // never be received.
    auto timeout = std::chrono::seconds(5);
    return socket_.receive(timeout)
        .thenTry([this](folly::Try<UnixSocket::Message>&& msg) {
          if (msg.hasException()) {
            // most likely cause: timed out waiting for the client to
            // send the protocol version.  FutureUnixSocket::receiveTimeout()
            // will close the socket unconditionally, so we can't send
            // an error message back to the peer.  However, for the sake
            // of clarity in the control flow we bubble up the error
            // as if we could do that.
            XLOG(ERR) << "Exception while waiting for takeover version from "
                         "the client.  Most likely reason is a client version "
                         "mismatch, you may need to perform a full "
                         "`eden shutdown ; eden daemon` restart to migrate."
                      << msg.exception();
            return folly::makeFuture<TakeoverData>(msg.exception());
          }

          auto query =
              CompactSerializer::deserialize<TakeoverVersionQuery>(&msg->data);

          auto supported =
              TakeoverData::computeCompatibleVersion(query.versions);

          if (!supported.has_value()) {
            auto clientVersionList = folly::join(", ", query.versions);
            auto serverVersionList =
                folly::join(", ", kSupportedTakeoverVersions);

            return folly::makeFuture<TakeoverData>(
                folly::make_exception_wrapper<std::runtime_error>(
                    folly::to<std::string>(
                        "The client and the server do not share a common "
                        "takeover protocol implementation.  Use "
                        "`eden shutdown ; eden daemon` to migrate.  "
                        "clientVersions=[",
                        clientVersionList,
                        "], "
                        "serverVersions=[",
                        serverVersionList,
                        "]")));
          }
          // Initiate the takeover shutdown.
          protocolVersion_ = supported.value();
          return server_->getTakeoverHandler()->startTakeoverShutdown();
        })
        .thenTryInline(folly::makeAsyncTask(
            server_->eventBase_, [this](folly::Try<TakeoverData>&& data) {
              return sendTakeoverData(std::move(data));
            }));
  } catch (const std::exception& ex) {
    return makeFuture<Unit>(
        folly::exception_wrapper{std::current_exception(), ex});
  }
}

Future<Unit> TakeoverServer::ConnHandler::sendTakeoverData(
    folly::Try<TakeoverData>&& dataTry) {
  if (!dataTry.hasValue()) {
    XLOG(ERR) << "error while performing takeover shutdown: "
              << dataTry.exception();
    if (socket_) {
      // Send the error to the client.
      return socket_.send(
          TakeoverData::serializeError(protocolVersion_, dataTry.exception()));
    }
    // Socket was closed (likely by a receive timeout above), so don't
    // try to send again in here lest we break; instead just pass up
    // the error.
    return makeFuture<Unit>(dataTry.exception());
  }

  UnixSocket::Message msg;
  auto& data = dataTry.value();
  try {
    msg.data = data.serialize(protocolVersion_);
    msg.files.push_back(std::move(data.lockFile));
    msg.files.push_back(std::move(data.thriftSocket));
    for (auto& mount : data.mountPoints) {
      msg.files.push_back(std::move(mount.fuseFD));
    }
  } catch (const std::exception& ex) {
    auto ew = folly::exception_wrapper{std::current_exception(), ex};
    data.takeoverComplete.setException(ew);
    return socket_.send(TakeoverData::serializeError(protocolVersion_, ew));
  }

  XLOG(INFO) << "Sending takeover data to new process: "
             << msg.data.computeChainDataLength() << " bytes";

  return socket_.send(std::move(msg))
      .thenTry([promise = std::move(data.takeoverComplete)](
                   folly::Try<Unit>&& sendResult) mutable {
        promise.setTry(std::move(sendResult));
      });
}

TakeoverServer::TakeoverServer(
    folly::EventBase* eventBase,
    AbsolutePathPiece socketPath,
    TakeoverHandler* handler)
    : eventBase_{eventBase}, handler_{handler}, socketPath_{socketPath} {
  start();
}

TakeoverServer::~TakeoverServer() {}

void TakeoverServer::start() {
  // Build the address for the takeover socket.
  SocketAddress address;
  address.setFromPath(socketPath_.stringPiece());

  // Remove any old file at this path, so we can bind to it.
  auto rc = unlink(socketPath_.value().c_str());
  if (rc != 0 && errno != ENOENT) {
    folly::throwSystemError("error removing old takeover socket");
  }

  socket_.reset(new AsyncServerSocket{eventBase_});
  socket_->bind(address);
  socket_->listen(/* backlog */ 1024);
  socket_->addAcceptCallback(this, nullptr);
  socket_->startAccepting();
}

void TakeoverServer::connectionAccepted(
    folly::NetworkSocket fdNetworkSocket,
    const folly::SocketAddress& /* clientAddr */) noexcept {
  int fd = fdNetworkSocket.toFd();

  folly::File socket(fd, /* ownsFd */ true);
  std::unique_ptr<ConnHandler> handler;
  try {
    handler.reset(new ConnHandler{this, std::move(socket)});
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error allocating connection handler for new takeover "
                 "connection: "
              << exceptionStr(ex);
    return;
  }

  XLOG(INFO) << "takeover socket connection received";
  auto* handlerRawPtr = handler.get();
  handlerRawPtr->start()
      .thenError([](const folly::exception_wrapper& ew) {
        XLOG(ERR) << "error processing takeover connection request: "
                  << folly::exceptionStr(ew);
      })
      .ensure([h = std::move(handler)] {});
}

void TakeoverServer::acceptError(const std::exception& ex) noexcept {
  XLOG(ERR) << "accept() error on takeover socket: " << exceptionStr(ex);
}
} // namespace eden
} // namespace facebook
