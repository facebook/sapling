/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

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
#include "eden/fs/utils/EventBaseState.h"
#include "eden/fs/utils/FutureUnixSocket.h"

using apache::thrift::CompactSerializer;
using folly::AsyncServerSocket;
using folly::exceptionStr;
using folly::Future;
using folly::makeFuture;
using folly::SocketAddress;
using folly::Unit;

DEFINE_int32(
    pingReceiveTimeout,
    5,
    "Timeout for receiving ready ping from new process in seconds");

namespace facebook::eden {

/**
 * ConnHandler handles a single connection received on the TakeoverServer
 * socket.
 */
class TakeoverServer::ConnHandler {
 public:
  ConnHandler(
      TakeoverServer* server,
      folly::File socket,
      const std::set<int32_t>& supportedVersions,
      const uint64_t supportedCapabilities)
      : server_{server},
        supportedCapabilities_{supportedCapabilities},
        supportedVersions_{supportedVersions},
        state_{
            server->getEventBase(),
            server->getEventBase(),
            std::move(socket)} {}

  /**
   * start() begins processing data on this connection.
   *
   * Returns a Future that will complete successfully when this connection
   * finishes gracefully taking over the EdenServer's mount points.
   */
  FOLLY_NODISCARD folly::Future<folly::Unit> start() noexcept;

 private:
  FOLLY_NODISCARD folly::Future<folly::Unit> sendError(
      const folly::exception_wrapper& error);

  FOLLY_NODISCARD folly::Future<folly::Unit> pingThenSendTakeoverData(
      TakeoverData&& data);

  FOLLY_NODISCARD folly::Future<folly::Unit> sendTakeoverData(
      TakeoverData&& data);

  template <typename... Args>
  [[noreturn]] void fail(Args&&... args) {
    auto msg = folly::to<std::string>(std::forward<Args>(args)...);
    XLOG(ERR) << "takeover socket error: " << msg;
    throw std::runtime_error(msg);
  }

  struct State {
    State(folly::EventBase* evb, folly::File socket)
        : socket{evb, std::move(socket)} {}

    bool shouldPing = false;
    // FutureUnixSocket must always be accessed on the EventBase.
    FutureUnixSocket socket;
    int32_t protocolVersion =
        TakeoverData::kTakeoverProtocolVersionNeverSupported;
    uint64_t protocolCapabilities = 0;
  };

  TakeoverServer* const server_;
  const uint64_t supportedCapabilities_;
  const std::set<int32_t>& supportedVersions_;
  EventBaseState<State> state_;
};

Future<Unit> TakeoverServer::ConnHandler::start() noexcept {
  // Check the remote endpoint's credentials.
  // We only allow transferring our mount points to another process
  // owned by the same user.
  auto& state = state_.get();
  auto uid = state.socket.getRemoteUID();
  if (uid != getuid()) {
    throwf<std::runtime_error>(
        "invalid takeover request from incorrect user: current UID={}, got request from UID {}",
        getuid(),
        uid);
  }

  // Check to see if we are speaking a compatible takeover protocol
  // version.  If not, error out so that we don't change any state.
  // The client should send us the version information, but clients
  // prior to the revision where this check was added will never send
  // us the version data.  We use a short timeout for receiving the
  // version data; in practice it will appear immediately or will
  // never be received.
  auto timeout = std::chrono::seconds(5);
  return state.socket.receive(timeout)
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

        auto supported = TakeoverData::computeCompatibleVersion(
            *query.versions_ref(), this->supportedVersions_);

        if (!supported.has_value()) {
          auto clientVersionList = folly::join(", ", *query.versions_ref());
          auto serverVersionList = folly::join(", ", this->supportedVersions_);

          throwf<std::runtime_error>(
              "The client and the server do not share a common "
              "takeover protocol implementation.  Use "
              "`eden shutdown ; eden daemon` to migrate.  "
              "clientVersions=[{}], serverVersions=[{}]",
              clientVersionList,
              serverVersionList);
        }

        auto& state = state_.get();

        // Initiate the takeover shutdown.
        state.protocolVersion = supported.value();
        auto protocolCapabilities =
            TakeoverData::versionToCapabilities(state.protocolVersion);
        if (protocolCapabilities & TakeoverCapabilities::CAPABILITY_MATCHING) {
          state.protocolCapabilities =
              TakeoverData::computeCompatibleCapabilities(
                  *query.capabilities_ref(), supportedCapabilities_);
        } else {
          state.protocolCapabilities = protocolCapabilities;
        }

        XLOG(DBG7) << "Protocol version: " << state.protocolVersion
                   << "; Protocol Capabilities: " << state.protocolCapabilities;

        state.shouldPing =
            (state.protocolCapabilities & TakeoverCapabilities::PING);
        return server_->getTakeoverHandler()->startTakeoverShutdown();
      })
      .via(server_->eventBase_)
      .thenTry([this](folly::Try<TakeoverData>&& data) {
        if (!data.hasValue()) {
          return sendError(data.exception());
        }
        if (state_.get().shouldPing) {
          XLOG(DBG7) << "sending ready ping to takeover client";
          return pingThenSendTakeoverData(std::move(data.value()));
        } else {
          XLOG(DBG7) << "not sending ready ping to takeover client";
          return sendTakeoverData(std::move(data.value()));
        }
      });
}

Future<Unit> TakeoverServer::ConnHandler::sendError(
    const folly::exception_wrapper& error) {
  XLOG(ERR) << "error while performing takeover shutdown: " << error;
  auto& state = state_.get();
  if (state.socket) {
    // Send the error to the client.
    return state.socket.send(
        TakeoverData::serializeError(state.protocolCapabilities, error));
  }
  // Socket was closed (likely by a receive timeout above), so don't
  // try to send again in here lest we break; instead just pass up
  // the error.
  return makeFuture<Unit>(error);
}

Future<Unit> TakeoverServer::ConnHandler::pingThenSendTakeoverData(
    TakeoverData&& data) {
  // Send a message to ping the takeover client process.
  // This ensures that the client is still connected and ready to receive data.
  // If the client disconnected while we were pausing our checkout mounts and
  // preparing the takeover, we want to resume our mounts rather than trying to
  // transfer them to to the now-disconnected process.
  UnixSocket::Message msg;
  msg.data = TakeoverData::serializePing();

  auto& state = state_.get();

  return state.socket.send(std::move(msg))
      .thenValue([this](auto&&) {
        // Wait for the ping reply. Here we just give it a few seconds to
        // respond.
        return server_->faultInjector_.checkAsync("takeover", "ping_receive")
            .semi();
      })
      .via(server_->eventBase_)
      .thenValue([this](auto&&) {
        auto timeout = std::chrono::seconds(FLAGS_pingReceiveTimeout);
        auto& state = state_.get();
        return state.socket.receive(timeout);
      })
      .thenTry([this, data = std::move(data)](
                   folly::Try<UnixSocket::Message>&& msg) mutable {
        if (msg.hasException()) {
          // If we got an exception on sending or receiving here, we should
          // bubble up an exception and recover.

          // We must save the original takeoverComplete promise
          // since we will move the TakeoverData into the takeoverComplete
          // promise and the EdenServer waits on this to be fulfilled to
          // determine to recover or not
          auto takeoverPromise = std::move(data.takeoverComplete);
          takeoverPromise.setValue(std::move(data));

          return makeFuture<Unit>(msg.exception());
        }
        return sendTakeoverData(std::move(data));
      });
}

Future<Unit> TakeoverServer::ConnHandler::sendTakeoverData(
    TakeoverData&& data) {
  // Before sending the takeover data, we must close the server's
  // local and backing store. This is important for ensuring the RocksDB
  // lock is released so the client can take over.
  server_->getTakeoverHandler()->closeStorage();

  auto& state = state_.get();

  UnixSocket::Message msg;
  try {
    data.serialize(state.protocolCapabilities, msg);
    for (auto& file : msg.files) {
      XLOG(DBG7) << "sending fd for takeover: " << file.fd();
    }
  } catch (...) {
    auto ew = folly::exception_wrapper{std::current_exception()};
    data.takeoverComplete.setException(ew);
    return state.socket.send(
        TakeoverData::serializeError(state.protocolCapabilities, ew));
  }

  XLOG(INFO) << "Sending takeover data to new process: "
             << msg.data.computeChainDataLength() << " bytes";

  return state.socket.send(std::move(msg))
      .thenTry([promise = std::move(data.takeoverComplete)](
                   folly::Try<Unit>&& sendResult) mutable {
        if (sendResult.hasException()) {
          promise.setException(sendResult.exception());
        } else {
          // Set an uninitalized optional here to avoid an attempted recovery
          promise.setValue(std::nullopt);
        }
      });
}

TakeoverServer::TakeoverServer(
    folly::EventBase* eventBase,
    AbsolutePathPiece socketPath,
    TakeoverHandler* handler,
    FaultInjector* faultInjector,
    const std::set<int32_t>& supportedVersions,
    const uint64_t supportedCapabilities)
    : eventBase_{eventBase},
      handler_{handler},
      socketPath_{socketPath},
      faultInjector_(*faultInjector),
      supportedCapabilities_{supportedCapabilities},
      supportedVersions_{supportedVersions} {
  start();
}

TakeoverServer::~TakeoverServer() = default;

void TakeoverServer::start() {
  // Build the address for the takeover socket.
  SocketAddress address;
  address.setFromPath(socketPath_.view());

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
    const folly::SocketAddress& /* clientAddr */,
    AcceptInfo /* info */) noexcept {
  int fd = fdNetworkSocket.toFd();

  folly::File socket(fd, /* ownsFd */ true);
  std::unique_ptr<ConnHandler> handler;
  try {
    handler.reset(new ConnHandler{
        this, std::move(socket), supportedVersions_, supportedCapabilities_});
  } catch (const std::exception& ex) {
    XLOG(ERR) << "error allocating connection handler for new takeover "
                 "connection: "
              << exceptionStr(ex);
    return;
  }

  XLOG(INFO) << "takeover socket connection received";
  auto* handlerRawPtr = handler.get();
  folly::makeFutureWith([&] { return handlerRawPtr->start(); })
      .thenError([](const folly::exception_wrapper& ew) {
        XLOG(ERR) << "error processing takeover connection request: "
                  << folly::exceptionStr(ew);
      })
      .ensure([h = std::move(handler)] {});
}

void TakeoverServer::acceptError(folly::exception_wrapper ex) noexcept {
  XLOG(ERR) << "accept() error on takeover socket: " << exceptionStr(ex);
}
} // namespace facebook::eden

#endif
