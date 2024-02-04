/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/RpcServer.h"

#include <tuple>

#include <folly/Exception.h>
#include <folly/String.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBufQueue.h>
#include <folly/io/async/AsyncSocket.h>

#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"
#include "eden/fs/utils/Throw.h"

using folly::AsyncServerSocket;
using folly::AsyncSocket;
using folly::Future;
using folly::IOBuf;
using folly::SocketAddress;

namespace facebook::eden {

bool RpcStopData::isUnmounted() {
  return !socketToKernel;
}

FsChannelInfo RpcStopData::extractTakeoverInfo() {
  return NfsChannelData{std::move(socketToKernel)};
}

void RpcConnectionHandler::getReadBuffer(void** bufP, size_t* lenP) {
  // TODO(xavierd): Should maxSize be configured to be at least the
  // configured NFS iosize?
  constexpr size_t maxSize = 64 * 1024;
  constexpr size_t minReadSize = 4 * 1024;

  // We want to issue a recv(2) of at least minReadSize, and bound it to
  // the available writable size of the readBuf_ to minimize allocation
  // cost. This guarantees reading large buffers, and minimize the number
  // of calls to tryConsumeReadBuffer.
  auto minSize = std::max(readBuf_.tailroom(), minReadSize);

  auto [buf, len] = readBuf_.preallocate(minSize, maxSize);
  *lenP = len;
  *bufP = buf;
}

void RpcConnectionHandler::readDataAvailable(size_t len) noexcept {
  readBuf_.postallocate(len);
  tryConsumeReadBuffer();
}

bool RpcConnectionHandler::isBufferMovable() noexcept {
  // prefer to have getReadBuffer / readDataAvailable called
  // rather than readBufferAvailable.
  return true;
}

void RpcConnectionHandler::readBufferAvailable(
    std::unique_ptr<IOBuf> readBuf) noexcept {
  readBuf_.append(std::move(readBuf));
  tryConsumeReadBuffer();
}

void RpcConnectionHandler::readEOF() noexcept {
  // The socket was closed on us.
  //
  // For mountd, this is just a connection closing which is normal after every
  // request. We don't care about the stop date.
  //
  // For nfsd, this means the mountpoint was unmounted, so record the stop
  // reason as UNMOUNT.
  //
  // We intentionally "drop" this future. This is fine: there is no need to
  // block the caller on completing the shutdown. We need to update the
  // handler's state inline with this so that we could not have multiple
  // versions of shutdown running in parallel, but we can wait for all requests
  // to finish asynchronously from this call.
  folly::futures::detachOn(
      sock_->getEventBase(), resetReader(RpcStopReason::UNMOUNT));
}

void RpcConnectionHandler::readErr(
    const folly::AsyncSocketException& ex) noexcept {
  XLOG(ERR) << "Error while reading: " << folly::exceptionStr(ex);
  // Reading from the socket failed. There's nothing else to do, so
  // close the connection.  See the comment in readEOF() for more
  // context.
  folly::futures::detachOn(
      sock_->getEventBase(), resetReader(RpcStopReason::ERROR));
}

void RpcConnectionHandler::writeErr(
    size_t /*bytesWritten*/,
    const folly::AsyncSocketException& ex) noexcept {
  // TODO: Should we assume the connection is broken, and we should close the
  // socket, aborting existing requests?
  XLOG(ERR) << "Error while writing: " << folly::exceptionStr(ex);
}

RpcConnectionHandler::RpcConnectionHandler(
    std::shared_ptr<RpcServerProcessor> proc,
    AsyncSocket::UniquePtr&& socket,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger,
    std::weak_ptr<RpcServer> owningServer)
    : proc_(proc),
      sock_(std::move(socket)),
      threadPool_(std::move(threadPool)),
      errorLogger_(structuredLogger),
      state_(sock_->getEventBase()),
      owningServer_(std::move(owningServer)) {
  sock_->setReadCB(this);
  proc_->clientConnected();
}

folly::SemiFuture<folly::Unit> RpcConnectionHandler::takeoverStop() {
  XLOG(DBG7) << "Takeover requested: locking state to change the status";
  // note its essential that this runs inline with the pending requests
  // check in resetReader. This ensures that we don't double set the pending
  // requests promise.
  {
    auto& state = state_.get();
    if (state.stopReason.has_value()) {
      // TODO: Ensure takeoverStop call sites handle exceptions
      // appropriately, and remove this makeSemiFutureWith.
      return folly::makeSemiFutureWith([&] {
        throwf<std::runtime_error>(
            "Takeover attempt failed: RpcServer already shutting down because {}",
            fmt::underlying(*state.stopReason));
      });
    }
  }
  XLOG(DBG7) << "Stop reading from the socket";
  // as far as I can tell this will deliver all reads to the reader before this
  // completes. So we should not see any new requests after this point.
  // Note: it is important this is done inline with the caller. i.e. if we start
  // a graceful restart then move off the main event base thread and do some
  // work then comeback to the main eventbase and do this. In the time, we were
  // off the main eventbase a readErr or readEOF could have been sent that
  // triggered a forced shutdown. Then we could have a graceful restart and a
  // shutdown running in parallel! Bad! Technically maybe these could co-exist,
  // but as the code is currently written that will result in too many
  // getSemiFuture from a promise calls and trying to set a promise more than
  // once.
  sock_->setReadCB(nullptr);

  // Trigger the reader to shutdown now, this will shutdown the handler as well.
  return resetReader(RpcStopReason::TAKEOVER);
}

folly::SemiFuture<folly::Unit> RpcConnectionHandler::resetReader(
    RpcStopReason stopReason) {
  // The lifetimes here are tricky. resetReader() is called by AsyncSocket
  // callbacks under EOF or error conditions, and `this` must stay alive for the
  // duration of this callback.
  DestructorGuard dg{this};

  {
    auto& state = state_.get();

    // it is important that we do this inline with our caller, so that we
    // could not start a graceful restart and then start a forced shutdown.
    // i.e. during a forced shutdown if we move off the main event base do some
    // work, then come back to the main eventbase and update the stopReason.
    // Then a graceful restart could start in the time we are off the
    // mainEventBase then we come back here and start a forced restart. that
    // could lead to too many getSemiFuture calls and too many promise
    // fulfillments in the next bit of code.
    state.stopReason = stopReason;

    // If we have already finished processing all requests, we set the promise.
    // If we don't set the promise in this case we would get stuck forever
    // waiting on the pending requests future.
    // Note this must run on the main eventbase for the socket, and inline with
    // setting the stop reason This ensures that we don't accidentally set this
    // promise twice.
    XLOG(DBG7) << "Pending requests: " << state.pendingRequests;
    if (state.pendingRequests == 0) {
      pendingRequestsComplete_.setValue();
    }
  }

  XLOG(DBG7) << "waiting for pending requests to complete";
  return pendingRequestsComplete_.getFuture().ensure(
      [this, proc = proc_, dg = std::move(dg), stopReason]() {
        XLOG(DBG7) << "Pending requests complete; "
                   << "finishing destroying this RPC handler";
        this->sock_->getEventBase()->checkIsInEventBaseThread();
        if (auto owningServer = this->owningServer_.lock()) {
          owningServer->unregisterRpcHandler(this);
        }

        RpcStopData data{};
        data.reason = stopReason;
        if (stopReason == RpcStopReason::TAKEOVER) {
          // We've already set readCB to nullptr, so detach the
          // network socket and transfer it to the process taking over
          // the connection.
          data.socketToKernel =
              folly::File{this->sock_->detachNetworkSocket().toFd(), true};
          this->sock_.reset();
        }

        // We could move the onSocketClosed call earlier, but it
        // triggers a lot of destruction, so first we finish cleaning up our
        // socket reading and then trigger the socket closed callback.
        proc->onShutdown(std::move(data));
      });
}

namespace {
std::string displayBuffer(folly::IOBuf* buf) {
  auto bytes = buf->coalesce();
  return folly::hexDump(bytes.data(), bytes.size());
}
} // namespace

void RpcConnectionHandler::tryConsumeReadBuffer() noexcept {
  // Iterate over all the complete fragments and dispatch these to the
  // threadPool_.
  while (true) {
    auto buf = readOneRequest();
    if (!buf) {
      break;
    }
    XLOG(DBG7) << "received a request";
    state_.get().pendingRequests += 1;
    // Send the work to a thread pool to increase the number of inflight
    // requests that can be handled concurrently.
    threadPool_->add(
        [this, buf = std::move(buf), guard = DestructorGuard(this)]() mutable {
          XLOG(DBG8) << "Received:\n" << displayBuffer(buf.get());
          // We use a scope so that the cursor is not still around after we
          // delete part of the IOBuf later. Attempting to use this cursor
          // after mutating the buffer could result in bad memory accesses.
          {
            folly::io::Cursor c(buf.get());
            uint32_t fragmentHeader = c.readBE<uint32_t>();
            bool isLast = (fragmentHeader & 0x80000000) != 0;

            // Supporting multiple fragments is expensive and requires playing
            // with IOBuf to avoid copying data. Since neither macOS nor Linux
            // are sending requests spanning multiple segments, let's not
            // support these.
            XCHECK(isLast);
          }

          // Trim off the fragment header.
          // We need to upgrade to an IOBufQueue because the IOBuf here is
          // actually part of a chain. The first buffer in the chain may not
          // have the full fragment header. Thus we need to be trimming off the
          // whole chain and not just from the first buffer.
          //
          // For example, this IOBuf might be the head of a chain of two IOBufs,
          // and the first IOBuf only contains 2 bytes. Trimming the IOBuf
          // would fail in this case.
          folly::IOBufQueue bufQueue{};
          bufQueue.append(std::move(buf));
          bufQueue.trimStart(sizeof(uint32_t));

          dispatchAndReply(bufQueue.move(), std::move(guard));
        });
  }
}

std::unique_ptr<folly::IOBuf> RpcConnectionHandler::readOneRequest() noexcept {
  if (!readBuf_.front()) {
    return nullptr;
  }
  folly::io::Cursor c(readBuf_.front());
  while (true) {
    uint32_t fragmentHeader;
    if (!c.tryReadBE<uint32_t>(fragmentHeader)) {
      // We can't even read the fragment header, bail out.
      return nullptr;
    }
    auto len = fragmentHeader & 0x7fffffff;
    bool isLast = (fragmentHeader & 0x80000000) != 0;
    if (!c.canAdvance(len)) {
      // we don't have a complete request, so try again later
      return nullptr;
    }
    c.skip(len);
    if (isLast) {
      break;
    }
  }
  return readBuf_.split(c.getCurrentPosition());
}

namespace {
void serializeRpcMismatch(folly::io::QueueAppender& ser, uint32_t xid) {
  rpc_msg_reply reply{
      xid,
      msg_type::REPLY,
      reply_body{{
          reply_stat::MSG_DENIED,
          rejected_reply{{
              reject_stat::RPC_MISMATCH,
              mismatch_info{kRPCVersion, kRPCVersion},
          }},
      }},
  };

  XdrTrait<rpc_msg_reply>::serialize(ser, reply);
}

void serializeAuthError(
    folly::io::QueueAppender& ser,
    auth_stat auth,
    uint32_t xid) {
  rpc_msg_reply reply{
      xid,
      msg_type::REPLY,
      reply_body{{
          reply_stat::MSG_DENIED,
          rejected_reply{{
              reject_stat::AUTH_ERROR,
              auth,
          }},
      }},
  };

  XdrTrait<rpc_msg_reply>::serialize(ser, reply);
}

/**
 * Make an RPC fragment by computing the size of the iobufQueue.
 *
 * Return an IOBuf chain that can be directly written to a socket.
 */
std::unique_ptr<folly::IOBuf> finalizeFragment(
    std::unique_ptr<folly::IOBufQueue> iobufQueue) {
  auto chainLength = iobufQueue->chainLength();
  auto resultBuffer = iobufQueue->move();

  // Fill out the fragment header.
  auto len = uint32_t(chainLength - sizeof(uint32_t));
  auto fragment = (uint32_t*)resultBuffer->writableData();
  *fragment = folly::Endian::big(len | 0x80000000);

  XLOG(DBG8) << "Sending:\n"
             << folly::hexDump(resultBuffer->data(), resultBuffer->length());
  return resultBuffer;
}
} // namespace

void RpcConnectionHandler::recordParsingError(
    RpcParsingError& err,
    std::unique_ptr<folly::IOBuf> input) {
  std::string message = fmt::format(
      "{} during {}. Full request {}.",
      err.what(),
      err.getProcedureContext(),
      folly::hexlify(input->coalesce()));

  XLOG(ERR) << message;

  errorLogger_->logEvent(NfsParsingError{
      folly::to<std::string>("FS", " - ", err.getProcedureContext()), message});
}

void RpcConnectionHandler::replyServerError(
    accept_stat err,
    uint32_t xid,
    std::unique_ptr<folly::IOBufQueue>& outputBuffer) {
  // We don't know how much was already written to  the outputBuffer,
  // thus let's clear it and write an error onto it.
  outputBuffer->reset();
  folly::io::QueueAppender errSer(outputBuffer.get(), 1024);
  XdrTrait<uint32_t>::serialize(errSer, 0); // reserve space for fragment header
  serializeReply(errSer, err, xid);
}

void RpcConnectionHandler::dispatchAndReply(
    std::unique_ptr<folly::IOBuf> input,
    DestructorGuard guard) {
  makeImmediateFutureWith(
      [&]() mutable -> ImmediateFuture<std::unique_ptr<folly::IOBuf>> {
        folly::io::Cursor deser(input.get());
        rpc_msg_call call = XdrTrait<rpc_msg_call>::deserialize(deser);

        auto iobufQueue = std::make_unique<folly::IOBufQueue>(
            folly::IOBufQueue::cacheChainLength());
        folly::io::QueueAppender ser(iobufQueue.get(), 1024);
        XdrTrait<uint32_t>::serialize(
            ser, 0); // reserve space for fragment header

        if (call.cbody.rpcvers != kRPCVersion) {
          serializeRpcMismatch(ser, call.xid);
          return finalizeFragment(std::move(iobufQueue));
        }

        if (auto auth = proc_->checkAuthentication(call.cbody);
            auth != auth_stat::AUTH_OK) {
          serializeAuthError(ser, auth, call.xid);
          return finalizeFragment(std::move(iobufQueue));
        }

        XLOG(DBG7) << "dispatching a request";
        auto fut = makeImmediateFutureWith([&]() mutable {
          return proc_->dispatchRpc(
              std::move(deser),
              std::move(ser),
              call.xid,
              call.cbody.prog,
              call.cbody.vers,
              call.cbody.proc);
        });

        return std::move(fut).thenTry(
            [this,
             input = std::move(input),
             iobufQueue = std::move(iobufQueue),
             call = std::move(call)](folly::Try<folly::Unit> result) mutable {
              XLOG(DBG7) << "Request done, sending response.";
              if (result.hasException()) {
                if (auto* err =
                        result.exception().get_exception<RpcParsingError>()) {
                  recordParsingError(*err, std::move(input));
                  replyServerError(
                      accept_stat::GARBAGE_ARGS, call.xid, iobufQueue);
                } else {
                  XLOGF(
                      WARN,
                      "Server failed to dispatch proc {} to {}:{}: {}",
                      call.cbody.proc,
                      call.cbody.prog,
                      call.cbody.vers,
                      folly::exceptionStr(*result.exception().get_exception()));

                  replyServerError(
                      accept_stat::SYSTEM_ERR, call.xid, iobufQueue);
                }
              }
              return finalizeFragment(std::move(iobufQueue));
            });
      })
      .semi()
      // Make sure that all the computation occurs on the threadPool.
      // TODO(xavierd): In the case where the ImmediateFuture is ready adding
      // it to the thread pool is inefficient. In the case where this shows up
      // in profiling, this can be slightly optimized by simply pushing the
      // value to the EventBase directly.
      .via(threadPool_.get())
      // Then move it back to the EventBase to write the result to the socket.
      .via(this->sock_->getEventBase())
      .then([this](folly::Try<std::unique_ptr<folly::IOBuf>> result) {
        // This code runs in the EventBase and thus must be as fast as
        // possible to avoid unnecessary overhead in the EventBase. Always
        // prefer duplicating work in the future above to adding code here.

        if (result.hasException()) {
          // XXX: This should never happen.
        } else {
          auto resultBuffer = std::move(result).value();
          XLOG(DBG7) << "About to write to the socket.";
          // TODO: Wait until the write completes before considering
          // the request finished.
          sock_->writeChain(this, std::move(resultBuffer));
        }
      })
      .ensure([this, guard = std::move(guard)]() {
        XLOG(DBG7) << "Request complete";
        auto& state = this->state_.get();
        state.pendingRequests -= 1;
        XLOG(DBG7) << state.pendingRequests << " more requests to process";
        if (state.pendingRequests == 0 && state.stopReason.has_value()) {
          // We are shutting down and the last request has been
          // handled, so signal that all pending requests have
          // completed.
          pendingRequestsComplete_.setValue();
        }
      });
}

void RpcServer::connectionAccepted(
    folly::NetworkSocket fd,
    const folly::SocketAddress& clientAddr,
    AcceptInfo /* info */) noexcept {
  XLOG(DBG7) << "Accepted connection from: " << clientAddr;
  auto socket = AsyncSocket::newSocket(evb_, fd);
  auto& state = state_.get();
  state.connectionHandlers.push_back(RpcConnectionHandler::create(
      proc_,
      std::move(socket),
      threadPool_,
      structuredLogger_,
      weak_from_this()));

  // At this point we could stop accepting connections with this callback for
  // nfsd3 because we only support one connected client, and we do not support
  // reconnects. BUT its tricky to unregister the accept callback.
  // to unregister and is fine to keep it around for now and just clean it up on
  // shutdown.
  //
  // TODO: Is it really tricky to unregister the accept callback? We could call
  // stopAccepting() here and removeAcceptCallback.
}

void RpcServer::acceptError(const std::exception& ex) noexcept {
  XLOG(ERR) << "acceptError: " << folly::exceptionStr(ex);
}

void RpcServer::acceptStopped() noexcept {
  state_.get().acceptStopped = true;
}

auth_stat RpcServerProcessor::checkAuthentication(
    const call_body& /*call_body*/) {
  // Completely ignore authentication.
  // TODO: something reasonable here
  return auth_stat::AUTH_OK;
}

ImmediateFuture<folly::Unit> RpcServerProcessor::dispatchRpc(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender /*ser*/,
    uint32_t /*xid*/,
    uint32_t /*progNumber*/,
    uint32_t /*progVersion*/,
    uint32_t /*procNumber*/) {
  return folly::unit;
}

void RpcServerProcessor::onShutdown(RpcStopData) {}
void RpcServerProcessor::clientConnected() {}

std::shared_ptr<RpcServer> RpcServer::create(
    std::shared_ptr<RpcServerProcessor> proc,
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger) {
  return std::shared_ptr<RpcServer>{
      new RpcServer{
          std::move(proc), evb, std::move(threadPool), structuredLogger},
      [](RpcServer* p) { p->destroy(); }};
}

RpcServer::RpcServer(
    std::shared_ptr<RpcServerProcessor> proc,
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger)
    : evb_(evb),
      threadPool_(threadPool),
      structuredLogger_(structuredLogger),
      serverSocket_(new AsyncServerSocket(evb_)),
      proc_(std::move(proc)),
      state_{evb} {}

void RpcServer::destroy() {
  evb_->runInEventBaseThread([this] { delete this; });
}

void RpcServer::initialize(folly::SocketAddress addr) {
  evb_->checkIsInEventBaseThread();

  // Ask kernel to assign us a port on the loopback interface
  serverSocket_->bind(addr);
  serverSocket_->listen(1024);

  serverSocket_->addAcceptCallback(this, nullptr);
  serverSocket_->startAccepting();
}

void RpcServer::initializeConnectedSocket(folly::File socket) {
  XLOG(DBG7) << "Initializing server from connected socket: " << socket.fd();
  // Note we don't initialize the accepting socket in this case. This is
  // meant for server that only ever has one connected socket (nfsd3). Since
  // we already have the one connected socket, we will not need the
  // accepting socket to make any more connections.
  auto& state = state_.get();
  state.connectionHandlers.push_back(RpcConnectionHandler::create(
      proc_,
      AsyncSocket::newSocket(
          evb_, folly::NetworkSocket::fromFd(socket.release())),
      threadPool_,
      structuredLogger_,
      weak_from_this()));
}

void RpcServer::initializeServerSocket(folly::File socket) {
  evb_->checkIsInEventBaseThread();

  XLOG(DBG7) << "Initializing server from server socket: " << socket.fd();

  serverSocket_->useExistingSocket(
      folly::NetworkSocket::fromFd(socket.release()));
  serverSocket_->addAcceptCallback(this, nullptr);
  serverSocket_->startAccepting();
}

void RpcServer::unregisterRpcHandler(RpcConnectionHandler* handlerToErase) {
  auto& state = state_.get();
  auto& handlers = state.connectionHandlers;
  handlers.erase(
      std::remove_if(
          handlers.begin(),
          handlers.end(),
          [handlerToErase](RpcConnectionHandler::UniquePtr& handler) {
            return handler.get() == handlerToErase;
          }),
      handlers.end());
}

folly::SemiFuture<folly::File> RpcServer::takeoverStop() {
  auto& state = state_.get();

  XLOG(DBG7) << "Removing accept callback";

  if (serverSocket_->getAccepting()) {
    serverSocket_->removeAcceptCallback(this, nullptr);
    XCHECK(state.acceptStopped)
        << "We always accept on the same primary socket EventBase, so it "
           "should be guaranteed that acceptStopped() ran synchronously.";

    // Removing the last accept callback implicitly paused accepting.
  }

  // No more connections will be made after this point.

  XLOG(DBG7) << "calling takeover stop on handlers";
  // todo should this return the file descriptor for the socket?
  std::vector<RpcConnectionHandler::UniquePtr> handlers;
  handlers.swap(state.connectionHandlers);

  std::vector<folly::SemiFuture<folly::Unit>> futures;
  futures.reserve(handlers.size());
  for (auto& handler : handlers) {
    futures.emplace_back(handler->takeoverStop());
  }

  auto fd = serverSocket_->getNetworkSocket().toFd();
  return collectAll(futures)
      .via(evb_) // make sure we are running on the eventbase to do some more
                 // socket operations
      .thenValue([fd](auto&&) {
        if (fd == -1) {
          return folly::File{};
        }
        // TODO: This needs Windows-specific handling. folly::File and
        // NetworkSocket are not compatible on Windows.

        // The AsyncServerSocket owns the socket handle, so we can't
        // steal ownership here. Duplicate the existing fd and send
        // the duplicated fd to the taking-over process.
        return folly::File(fd, false).dupCloseOnExec();
      });
}

RpcServer::~RpcServer() {
  auto& ebstate = state_.get();
  if (ebstate.portmapState) {
    auto& state = ebstate.portmapState.value();
    for (const auto& mapping : state.mappedPorts) {
      state.portMap.unsetMapping(mapping);
    }
  }
}

namespace {

std::pair<std::string, std::string> getNetIdAndAddr(const SocketAddress& addr) {
  if (addr.isFamilyInet()) {
    auto netid = addr.getFamily() == AF_INET6 ? PortmapMapping4::kTcp6NetId
                                              : PortmapMapping4::kTcpNetId;
    auto port = addr.getPort();
    // The port format is a bit odd, reversed from looking at rpcinfo output.
    return {
        netid,
        fmt::format(
            "{}.{}.{}", addr.getAddressStr(), (port >> 8) & 0xff, port & 0xff)};
  } else {
    return {PortmapMapping4::kLocalNetId, addr.getPath()};
  }
}

} // namespace

void RpcServer::registerService(uint32_t progNumber, uint32_t progVersion) {
  auto& ebstate = state_.get();
  auto& pmstate = ebstate.portmapState;
  if (!pmstate.has_value()) {
    // The rpcbind client was never initialized, do it now.
    pmstate.emplace();
  }
  auto& state = pmstate.value();

  // Enumerate the addresses (in practice, just the loopback) and use the
  // port number we got from the kernel to register the mapping for
  // this program/version pair with rpcbind/portmap.
  auto addrs = serverSocket_->getAddresses();
  for (auto& addr : addrs) {
    auto [netid, addrStr] = getNetIdAndAddr(addr);
    PortmapMapping4 mapping{progNumber, progVersion, netid, addrStr, "edenfs"};
    state.portMap.setMapping(mapping);
    state.mappedPorts.push_back(std::move(mapping));
  }
}

folly::SocketAddress RpcServer::getAddr() const {
  return serverSocket_->getAddress();
}

} // namespace facebook::eden
