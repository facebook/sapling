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
#include <folly/experimental/logging/xlog.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/io/async/EventHandler.h>

#include "eden/fs/takeover/TakeoverData.h"
#include "eden/fs/takeover/TakeoverHandler.h"
#include "eden/fs/utils/FutureUnixSocket.h"

using namespace std::literals::chrono_literals;

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
  folly::Future<folly::Unit> start() noexcept;

 private:
  folly::Future<folly::Unit> sendTakeoverData(folly::Try<TakeoverData>&& data);

  template <typename... Args>
  [[noreturn]] void fail(Args&&... args) {
    auto msg = folly::to<std::string>(std::forward<Args>(args)...);
    XLOG(ERR) << "takeover socket error: " << msg;
    throw std::runtime_error(msg);
  }

  TakeoverServer* const server_{nullptr};
  FutureUnixSocket socket_;
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

    // Initiate the takeover shutdown.
    return server_->getTakeoverHandler()->startTakeoverShutdown().then(
        [this](folly::Try<TakeoverData>&& data) {
          return sendTakeoverData(std::move(data));
        });
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
    // Send the error to the client.
    return socket_.send(TakeoverData::serializeError(dataTry.exception()));
  }

  UnixSocket::Message msg;
  auto& data = dataTry.value();
  try {
    msg.data = data.serialize();
    msg.files.push_back(std::move(data.lockFile));
    msg.files.push_back(std::move(data.thriftSocket));
    for (auto& mount : data.mountPoints) {
      msg.files.push_back(std::move(mount.fuseFD));
    }
  } catch (const std::exception& ex) {
    auto ew = folly::exception_wrapper{std::current_exception(), ex};
    data.takeoverComplete.setException(ew);
    return socket_.send(TakeoverData::serializeError(ew));
  }

  return socket_.send(std::move(msg))
      .then([promise = std::move(data.takeoverComplete)](
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
    int fd,
    const folly::SocketAddress& /* clientAddr */) noexcept {
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
      .onError([](const folly::exception_wrapper& ew) {
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
