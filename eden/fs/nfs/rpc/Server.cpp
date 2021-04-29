/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#ifndef _WIN32

#include "eden/fs/nfs/rpc/Server.h"

#include <folly/Exception.h>
#include <folly/String.h>
#include <folly/io/IOBufQueue.h>
#include <folly/io/async/AsyncSocket.h>
#include <tuple>

using folly::AsyncServerSocket;
using folly::AsyncSocket;
using folly::Future;
using folly::IOBuf;
using folly::SocketAddress;

namespace facebook {
namespace eden {

namespace {
class RpcTcpHandler : public folly::DelayedDestruction {
 public:
  using UniquePtr =
      std::unique_ptr<RpcTcpHandler, folly::DelayedDestruction::Destructor>;

  /**
   * Build a RpcTcpHandler.
   *
   * When the returned UniquePtr is dropped, this class will stay alive until
   * the client drops the connection, at which time the memory will be released
   * and the socket will be closed.
   */
  template <class... Args>
  static UniquePtr create(Args&&... args) {
    return UniquePtr(
        new RpcTcpHandler(std::forward<Args>(args)...),
        folly::DelayedDestruction::Destructor());
  }

 private:
  RpcTcpHandler(
      std::shared_ptr<RpcServerProcessor> proc,
      AsyncSocket::UniquePtr&& socket,
      std::shared_ptr<folly::Executor> threadPool)
      : proc_(proc),
        sock_(std::move(socket)),
        threadPool_(std::move(threadPool)),
        reader_(std::make_unique<Reader>(this)) {
    sock_->setReadCB(reader_.get());
  }

  class Reader : public folly::AsyncReader::ReadCallback {
   public:
    explicit Reader(RpcTcpHandler* handler)
        : handler_(handler), guard_(handler_) {}

   private:
    void getReadBuffer(void** bufP, size_t* lenP) override {
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

    void readDataAvailable(size_t len) noexcept override {
      handler_->readBuf_.postallocate(len);
      handler_->tryConsumeReadBuffer();
    }

    bool isBufferMovable() noexcept override {
      // prefer to have getReadBuffer / readDataAvailable called
      // rather than readBufferAvailable.
      return true;
    }

    void readBufferAvailable(std::unique_ptr<IOBuf> readBuf) noexcept override {
      handler_->readBuf_.append(std::move(readBuf));
      handler_->tryConsumeReadBuffer();
    }

    void deleteMe() {
      handler_->resetReader();
    }

    void readEOF() noexcept override {
      deleteMe();
    }

    void readErr(const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while reading: " << folly::exceptionStr(ex);
      deleteMe();
    }

    RpcTcpHandler* handler_;
    DestructorGuard guard_;
  };

  class Writer : public folly::AsyncWriter::WriteCallback {
   public:
    Writer() = default;

   private:
    void writeSuccess() noexcept override {}

    void writeErr(
        size_t /*bytesWritten*/,
        const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while writing: " << folly::exceptionStr(ex);
    }
  };

  /**
   * Parse the buffer that was just read from the socket. Complete RPC buffers
   * will be dispatched to the RpcServerProcessor.
   */
  void tryConsumeReadBuffer() noexcept;

  /**
   * Delete the reader, called when the socket is closed.
   */
  void resetReader() {
    reader_.reset();
  }

  /**
   * Try to read one request from the buffer.
   *
   * Return a nullptr if no complete RPC request can be read.
   */
  std::unique_ptr<folly::IOBuf> readOneRequest() noexcept;

  /**
   * Dispatch the RPC request contained in the input buffer to the
   * RpcServerProcessor.
   */
  void dispatchAndReply(
      std::unique_ptr<folly::IOBuf> input,
      DestructorGuard guard);

  std::shared_ptr<RpcServerProcessor> proc_;
  AsyncSocket::UniquePtr sock_;
  std::shared_ptr<folly::Executor> threadPool_;
  std::unique_ptr<Reader> reader_;
  Writer writer_{};
  folly::IOBufQueue readBuf_{folly::IOBufQueue::cacheChainLength()};
};

void RpcTcpHandler::tryConsumeReadBuffer() noexcept {
  // Iterate over all the complete fragments and dispatch these to the
  // threadPool_.
  while (true) {
    auto buf = readOneRequest();
    if (!buf) {
      break;
    }

    // Send the work to a thread pool to increase the number of inflight
    // requests that can be handled concurrently.
    threadPool_->add(
        [this, buf = std::move(buf), guard = DestructorGuard(this)]() mutable {
          XLOG(DBG8) << "Received:\n"
                     << folly::hexDump(buf->data(), buf->length());
          auto data = buf->data();
          auto fragmentHeader = folly::Endian::big(*(uint32_t*)data);
          bool isLast = (fragmentHeader & 0x80000000) != 0;

          // Supporting multiple fragments is expensive and requires playing
          // with IOBuf to avoid copying data. Since neither macOS nor Linux
          // are sending requests spanning multiple segments, let's not support
          // these.
          XCHECK(isLast);
          buf->trimStart(sizeof(uint32_t));

          dispatchAndReply(std::move(buf), std::move(guard));
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

    auto fut = proc_->dispatchRpc(
        std::move(deser),
        std::move(ser),
        call.xid,
        call.cbody.prog,
        call.cbody.vers,
        call.cbody.proc);

    return std::move(fut).then(
        [keepInputAlive = std::move(input),
         iobufQueue = std::move(iobufQueue),
         call = std::move(call)](folly::Try<folly::Unit> result) mutable {
          if (result.hasException()) {
            XLOGF(
                WARN,
                "Server failed to dispatch proc {} to {}:{}: {}",
                call.cbody.proc,
                call.cbody.prog,
                call.cbody.vers,
                folly::exceptionStr(*result.exception().get_exception()));

            // We don't know how much dispatchRpc wrote to the iobufQueue, thus
            // let's clear it and write an error onto it.
            iobufQueue->clear();
            folly::io::QueueAppender errSer(iobufQueue.get(), 1024);
            XdrTrait<uint32_t>::serialize(
                errSer, 0); // reserve space for fragment header

            serializeReply(errSer, accept_stat::SYSTEM_ERR, call.xid);
          }

          return finalizeFragment(std::move(iobufQueue));
        });
  })
      .via(this->sock_->getEventBase())
      .then([this, guard = std::move(guard)](
                folly::Try<std::unique_ptr<folly::IOBuf>> result) {
        // This code runs in the EventBase and thus must be as fast as possible
        // to avoid unnecessary overhead in the EventBase. Always prefer
        // duplicating work in the future above to adding code here.

        if (result.hasException()) {
          // XXX: This should never happen.
        } else {
          auto resultBuffer = std::move(result).value();
          sock_->writeChain(&writer_, std::move(resultBuffer));
        }
      });
}

} // namespace

void RpcServer::RpcAcceptCallback::connectionAccepted(
    folly::NetworkSocket fd,
    const folly::SocketAddress& clientAddr) noexcept {
  XLOG(DBG7) << "Accepted connection from: " << clientAddr;
  auto socket = AsyncSocket::newSocket(evb_, fd);
  auto handler = RpcTcpHandler::create(proc_, std::move(socket), threadPool_);
}

void RpcServer::RpcAcceptCallback::acceptError(
    const std::exception& ex) noexcept {
  XLOG(ERR) << "acceptError: " << folly::exceptionStr(ex);
}

void RpcServer::RpcAcceptCallback::acceptStopped() noexcept {
  // We won't ever be accepting any connection, it is now safe to delete
  // ourself, release the guard.
  guard_.reset();
}

auth_stat RpcServerProcessor::checkAuthentication(
    const call_body& /*call_body*/) {
  // Completely ignore authentication.
  // TODO: something reasonable here
  return auth_stat::AUTH_OK;
}

Future<folly::Unit> RpcServerProcessor::dispatchRpc(
    folly::io::Cursor /*deser*/,
    folly::io::QueueAppender /*ser*/,
    uint32_t /*xid*/,
    uint32_t /*progNumber*/,
    uint32_t /*progVersion*/,
    uint32_t /*procNumber*/) {
  return folly::unit;
}

RpcServer::RpcServer(
    std::shared_ptr<RpcServerProcessor> proc,
    folly::EventBase* evb,
    std::shared_ptr<folly::Executor> threadPool)
    : evb_(evb),
      acceptCb_(
          new RpcServer::RpcAcceptCallback(proc, evb_, std::move(threadPool))),
      serverSocket_(new AsyncServerSocket(evb_)) {
  // Ask kernel to assign us a port on the loopback interface
  serverSocket_->bind(SocketAddress("127.0.0.1", 0));
  serverSocket_->listen(1024);

  serverSocket_->addAcceptCallback(acceptCb_.get(), evb_);
  serverSocket_->startAccepting();
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

uint16_t RpcServer::getPort() const {
  return serverSocket_->getAddress().getPort();
}

} // namespace eden
} // namespace facebook

#endif
