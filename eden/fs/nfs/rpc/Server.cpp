/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/rpc/Server.h"

#include <tuple>

#include <folly/Exception.h>
#include <folly/String.h>
#include <folly/executors/QueuedImmediateExecutor.h>
#include <folly/futures/Future.h>
#include <folly/io/IOBufQueue.h>
#include <folly/io/async/AsyncSocket.h>

#include "eden/fs/nfs/rpc/Rpc.h"
#include "eden/fs/telemetry/LogEvent.h"
#include "eden/fs/telemetry/StructuredLogger.h"

using folly::AsyncServerSocket;
using folly::AsyncSocket;
using folly::Future;
using folly::IOBuf;
using folly::SocketAddress;

namespace facebook::eden {

RpcTcpHandler::Reader::Reader(RpcTcpHandler* handler)
    : handler_(handler), guard_(handler_) {}

void RpcTcpHandler::Reader::getReadBuffer(void** bufP, size_t* lenP) {
  // TODO(xavierd): Should maxSize be configured to be at least the
  // configured NFS iosize?
  constexpr size_t maxSize = 64 * 1024;
  constexpr size_t minReadSize = 4 * 1024;

  // We want to issue a recv(2) of at least minReadSize, and bound it to
  // the available writable size of the readBuf_ to minimize allocation
  // cost. This guarantees reading large buffers, and minimize the number
  // of calls to tryConsumeReadBuffer.
  auto minSize = std::max(handler_->readBuf_.tailroom(), minReadSize);

  auto [buf, len] = handler_->readBuf_.preallocate(minSize, maxSize);
  *lenP = len;
  *bufP = buf;
}

void RpcTcpHandler::Reader::readDataAvailable(size_t len) noexcept {
  handler_->readBuf_.postallocate(len);
  handler_->tryConsumeReadBuffer();
}

bool RpcTcpHandler::Reader::isBufferMovable() noexcept {
  // prefer to have getReadBuffer / readDataAvailable called
  // rather than readBufferAvailable.
  return true;
}

void RpcTcpHandler::Reader::readBufferAvailable(
    std::unique_ptr<IOBuf> readBuf) noexcept {
  handler_->readBuf_.append(std::move(readBuf));
  handler_->tryConsumeReadBuffer();
}

folly::SemiFuture<folly::Unit> RpcTcpHandler::Reader::deleteMe(
    RpcStopReason stopReason) {
  return handler_->resetReader(stopReason);
}

void RpcTcpHandler::Reader::readEOF() noexcept {
  // note1: The socket was closed on us. For the mountd, this is just a
  // connection closing which is normal after every request. we don't care too
  // much about the stop data for mountd any way because we throw it away. For
  // the nfsd this means the mountpoint was unmounted. Thus the socket closed
  // state is called unmounted, since this is when it is meaningful.
  // note2: we are "dropping" this future. This is fine, there is no need to
  // block the caller on completing the shutdown. We need to update the handers
  // state inline with this so that we could not have multiple versions of
  // shutdown running in parallel, but we can wait for all requests to finish
  // asynchronously from this call. (in fact it would lead to deadlock to
  // block this thread waiting to complete shutdown because shutdown might need
  // to run thing on our thread.)
  auto evb = handler_->sock_->getEventBase();
  deleteMe(RpcStopReason::UNMOUNT).via(evb);
}

void RpcTcpHandler::Reader::readErr(
    const folly::AsyncSocketException& ex) noexcept {
  XLOG(ERR) << "Error while reading: " << folly::exceptionStr(ex);
  // see comment in readEOF about "dropping" this future.
  auto evb = handler_->sock_->getEventBase();
  deleteMe(RpcStopReason::ERROR).via(evb);
}

void RpcTcpHandler::Writer::writeErr(
    size_t /*bytesWritten*/,
    const folly::AsyncSocketException& ex) noexcept {
  XLOG(ERR) << "Error while writing: " << folly::exceptionStr(ex);
}

RpcTcpHandler::RpcTcpHandler(
    std::shared_ptr<RpcServerProcessor> proc,
    AsyncSocket::UniquePtr&& socket,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger,
    std::weak_ptr<RpcServer> owningServer)
    : proc_(proc),
      sock_(std::move(socket)),
      threadPool_(std::move(threadPool)),
      errorLogger_(structuredLogger),
      reader_(std::make_unique<Reader>(this)),
      state_(sock_->getEventBase()),
      owningServer_(std::move(owningServer)) {
  sock_->setReadCB(reader_.get());
  proc_->clientConnected();
}

folly::SemiFuture<folly::Unit> RpcTcpHandler::takeoverStop() {
  XLOG(DBG7) << "Takeover requested: locking state to change the status";
  // note its essential that this runs inline with the pending requests
  // check in resetReader. This ensures that we don't double set the pending
  // requests promise.
  {
    auto& state = state_.get();
    if (state.stopReason != RpcStopReason::RUNNING) {
      return folly::makeSemiFuture<folly::Unit>(std::runtime_error(
          "Rpc Server already shutting down during a takeover."));
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
  return reader_->deleteMe(RpcStopReason::TAKEOVER);
}

folly::SemiFuture<folly::Unit> RpcTcpHandler::resetReader(
    RpcStopReason stopReason) {
  // The lifetimes here are tricky. The reader holds the last reference to the
  // RpcTcpHandler, so when we reset the reader, this class will be destroyed.
  // Thus, we need to keep any member variables around our selves to use after
  // the reset call.
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
    XLOG(DBG7) << "Pending Requests: " << state.pendingRequests;
    if (state.pendingRequests == 0) {
      pendingRequestsComplete_.setValue();
    }

    stopReason = state.stopReason;
  }

  auto future = pendingRequestsComplete_.getSemiFuture();

  XLOG(DBG7) << "waiting for pending requests to complete";
  return std::move(future)
      .via(
          this->sock_->getEventBase()) // make sure we go back to the main event
                                       // base to do our socket manipulations
      .ensure([this, proc = proc_, stopReason]() {
        XLOG(DBG7) << "Pending Requests complete;"
                   << "finishing destroying this rpc tcp handler";
        this->sock_->getEventBase()->dcheckIsInEventBaseThread();
        if (auto owningServer = this->owningServer_.lock()) {
          owningServer->unregisterRpcHandler(this);
        }

        RpcStopData data{};
        data.reason = stopReason;
        if (stopReason == RpcStopReason::TAKEOVER) {
          data.socketToKernel =
              folly::File{this->sock_->detachNetworkSocket().toFd(), true};
        }

        this->reader_.reset(); // DO NOT USE "this" after this point!

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

void RpcTcpHandler::tryConsumeReadBuffer() noexcept {
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

std::unique_ptr<folly::IOBuf> RpcTcpHandler::readOneRequest() noexcept {
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

void RpcTcpHandler::recordParsingError(
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

void RpcTcpHandler::replyServerError(
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

void RpcTcpHandler::dispatchAndReply(
    std::unique_ptr<folly::IOBuf> input,
    DestructorGuard guard) {
  folly::makeFutureWith([this, input = std::move(input)]() mutable {
    folly::io::Cursor deser(input.get());
    rpc_msg_call call = XdrTrait<rpc_msg_call>::deserialize(deser);

    auto iobufQueue = std::make_unique<folly::IOBufQueue>(
        folly::IOBufQueue::cacheChainLength());
    folly::io::QueueAppender ser(iobufQueue.get(), 1024);
    XdrTrait<uint32_t>::serialize(ser, 0); // reserve space for fragment header

    if (call.cbody.rpcvers != kRPCVersion) {
      serializeRpcMismatch(ser, call.xid);
      return folly::makeFuture(finalizeFragment(std::move(iobufQueue)));
    }

    if (auto auth = proc_->checkAuthentication(call.cbody);
        auth != auth_stat::AUTH_OK) {
      serializeAuthError(ser, auth, call.xid);
      return folly::makeFuture(finalizeFragment(std::move(iobufQueue)));
    }

    XLOG(DBG7) << "dispatching a request";
    auto fut = makeImmediateFutureWith([this,
                                        deser = std::move(deser),
                                        ser = std::move(ser),
                                        xid = call.xid,
                                        prog = call.cbody.prog,
                                        vers = call.cbody.vers,
                                        proc = call.cbody.proc]() mutable {
      return proc_->dispatchRpc(
          std::move(deser), std::move(ser), xid, prog, vers, proc);
    });

    return std::move(fut)
        .thenTry([this,
                  input = std::move(input),
                  iobufQueue = std::move(iobufQueue),
                  call =
                      std::move(call)](folly::Try<folly::Unit> result) mutable {
          XLOG(DBG7) << "Request done, sending response.";
          if (result.hasException()) {
            if (auto* err =
                    result.exception().get_exception<RpcParsingError>()) {
              recordParsingError(*err, std::move(input));
              replyServerError(accept_stat::GARBAGE_ARGS, call.xid, iobufQueue);
            } else {
              XLOGF(
                  WARN,
                  "Server failed to dispatch proc {} to {}:{}: {}",
                  call.cbody.proc,
                  call.cbody.prog,
                  call.cbody.vers,
                  folly::exceptionStr(*result.exception().get_exception()));

              replyServerError(accept_stat::SYSTEM_ERR, call.xid, iobufQueue);
            }
          }
          return finalizeFragment(std::move(iobufQueue));
        })
        .semi()
        .via(&folly::QueuedImmediateExecutor::instance());
  })
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
          sock_->writeChain(&writer_, std::move(resultBuffer));
        }
      })
      .ensure([this, guard = std::move(guard)]() {
        XLOG(DBG7) << "Request complete";
        {
          // needs to be called on sock_->getEventBase
          auto& state = this->state_.get();
          state.pendingRequests -= 1;
          // this is actually a bug two threads might try to set the promise
          XLOG(DBG7) << state.pendingRequests << " more requests to process";
          // We are shutting down either due to an unmount or a takeover,
          // and the last request has just been handled, we signal all the
          // pendingRequests have completed
          if (UNLIKELY(state.stopReason != RpcStopReason::RUNNING)) {
            if (state.pendingRequests == 0) {
              this->pendingRequestsComplete_.setValue();
            }
          }
        }
      });
}

void RpcServer::RpcAcceptCallback::connectionAccepted(
    folly::NetworkSocket fd,
    const folly::SocketAddress& clientAddr,
    AcceptInfo /* info */) noexcept {
  XLOG(DBG7) << "Accepted connection from: " << clientAddr;
  auto socket = AsyncSocket::newSocket(evb_, fd);
  auto handler = RpcTcpHandler::create(
      proc_, std::move(socket), threadPool_, structuredLogger_, owningServer_);

  if (auto server = owningServer_.lock()) {
    server->registerRpcHandler(std::move(handler));
  }

  // At this point we could stop accepting connections with this callback for
  // nfsd3 because we only support one connected client, and we do not support
  // reconnects. BUT its tricky to unregister the accept callback.
  // to unregister and is fine to keep it around for now and just clean it up on
  // shutdown.
}

void RpcServer::RpcAcceptCallback::acceptError(
    const std::exception& ex) noexcept {
  XLOG(ERR) << "acceptError: " << folly::exceptionStr(ex);
}

void RpcServer::RpcAcceptCallback::acceptStopped() noexcept {
  // We won't ever be accepting any connection, it is now safe to delete
  // ourself, release the guard.
  { auto guard = std::move(guard_); }
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
  return std::shared_ptr<RpcServer>{new RpcServer{
      std::move(proc), evb, std::move(threadPool), structuredLogger}};
}

RpcServer::RpcServer(
    std::shared_ptr<RpcServerProcessor> proc,
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool,
    const std::shared_ptr<StructuredLogger>& structuredLogger)
    : evb_(evb),
      threadPool_(threadPool),
      structuredLogger_(structuredLogger),
      acceptCb_(nullptr),
      serverSocket_(new AsyncServerSocket(evb_)),
      proc_(std::move(proc)),
      rpcTcpHandlers_{} {}

void RpcServer::initialize(folly::SocketAddress addr) {
  acceptCb_.reset(new RpcServer::RpcAcceptCallback{
      proc_,
      evb_,
      threadPool_,
      structuredLogger_,
      std::weak_ptr<RpcServer>{shared_from_this()}});

  // Ask kernel to assign us a port on the loopback interface
  serverSocket_->bind(addr);
  serverSocket_->listen(1024);

  serverSocket_->addAcceptCallback(acceptCb_.get(), evb_);
  serverSocket_->startAccepting();
}

void RpcServer::initialize(folly::File&& socket, InitialSocketType type) {
  switch (type) {
    case InitialSocketType::CONNECTED_SOCKET:
      XLOG(DBG7) << "Initializing server from connected socket: "
                 << socket.fd();
      // Note we don't initialize the accepting socket in this case. This is
      // meant for server that only ever has one connected socket (nfsd3). Since
      // we already have the one connected socket, we will not need the
      // accepting socket to make any more connections.
      rpcTcpHandlers_.wlock()->emplace_back(RpcTcpHandler::create(
          proc_,
          AsyncSocket::newSocket(
              evb_, folly::NetworkSocket::fromFd(socket.release())),
          threadPool_,
          structuredLogger_,
          shared_from_this()));
      return;
    case InitialSocketType::SERVER_SOCKET:
      XLOG(DBG7) << "Initializing server from server socket: " << socket.fd();
      acceptCb_.reset(new RpcServer::RpcAcceptCallback{
          proc_,
          evb_,
          threadPool_,
          structuredLogger_,
          std::weak_ptr<RpcServer>{shared_from_this()}});
      serverSocket_->useExistingSocket(
          folly::NetworkSocket::fromFd(socket.release()));

      serverSocket_->addAcceptCallback(acceptCb_.get(), evb_);
      serverSocket_->startAccepting();
      return;
  }
  throw std::runtime_error("Impossible socket type.");
}

void RpcServer::registerRpcHandler(RpcTcpHandler::UniquePtr handler) {
  rpcTcpHandlers_.wlock()->emplace_back(std::move(handler));
}

void RpcServer::unregisterRpcHandler(RpcTcpHandler* handlerToErase) {
  auto rpcTcpHandlers = rpcTcpHandlers_.wlock();
  rpcTcpHandlers->erase(
      std::remove_if(
          rpcTcpHandlers->begin(),
          rpcTcpHandlers->end(),
          [&handlerToErase](RpcTcpHandler::UniquePtr& handler) {
            return handler.get() == handlerToErase;
          }),
      rpcTcpHandlers->end());
}

folly::SemiFuture<folly::File> RpcServer::takeoverStop() {
  evb_->dcheckIsInEventBaseThread();

  XLOG(DBG7) << "Removing accept callback";
  if (acceptCb_) {
    serverSocket_->removeAcceptCallback(acceptCb_.get(), evb_);
  }
  // implicitly pauses accepting on the socket.
  // not more connections will be made after this point.

  XLOG(DBG7) << "calling takeover stop on handlers";
  // TODO this needs to check if the unique_ptr is valid
  // todo should this return the file descriptor for the socket?
  std::vector<RpcTcpHandler::UniquePtr> handlers{};
  {
    auto lockedHandlers = rpcTcpHandlers_.wlock();
    lockedHandlers->swap(handlers);
  }

  std::vector<folly::SemiFuture<folly::Unit>> futures{};
  futures.reserve(handlers.size());
  for (auto& handler : handlers) {
    futures.emplace_back(handler->takeoverStop());
  }
  return collectAll(futures)
      .via(evb_) // make sure we are running on the eventbase to do some more
                 // socket operations
      .thenValue([this](auto&&) {
        auto fd = this->serverSocket_->getNetworkSocket().toFd();
        if (fd == -1) {
          return folly::File{};
        }
        return folly::File(fd, true);
      });
}

RpcServer::~RpcServer() {
  auto lock = portMapState_.wlock();
  if (lock->has_value()) {
    auto& state = lock->value();
    for (const auto& mapping : state.mappedPorts) {
      state.portMap.unsetMapping(mapping);
    }
  }
}

namespace {

std::pair<std::string, std::string> getNetIdAndAddr(const SocketAddress& addr) {
  if (addr.isFamilyInet()) {
    auto netid = addr.getFamily() == AF_INET6 ? PortmapMapping::kTcp6NetId
                                              : PortmapMapping::kTcpNetId;
    auto port = addr.getPort();
    // The port format is a bit odd, reversed from looking at rpcinfo output.
    return {
        netid,
        fmt::format(
            "{}.{}.{}", addr.getAddressStr(), (port >> 8) & 0xff, port & 0xff)};
  } else {
    return {PortmapMapping::kLocalNetId, addr.getPath()};
  }
}

} // namespace

void RpcServer::registerService(uint32_t progNumber, uint32_t progVersion) {
  auto lock = portMapState_.wlock();
  if (!lock->has_value()) {
    // The rpcbind client was never initialized, do it now.
    lock->emplace();
  }
  auto& state = lock->value();

  // Enumerate the addresses (in practice, just the loopback) and use the
  // port number we got from the kernel to register the mapping for
  // this program/version pair with rpcbind/portmap.
  auto addrs = serverSocket_->getAddresses();
  for (auto& addr : addrs) {
    auto [netid, addrStr] = getNetIdAndAddr(addr);
    PortmapMapping mapping{progNumber, progVersion, netid, addrStr, "edenfs"};
    state.portMap.setMapping(mapping);
    state.mappedPorts.push_back(std::move(mapping));
  }
}

folly::SocketAddress RpcServer::getAddr() const {
  return serverSocket_->getAddress();
}

} // namespace facebook::eden

#endif
