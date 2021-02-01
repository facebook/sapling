/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "eden/fs/nfs/rpc/Server.h"

#include <common/network/NetworkUtil.h>
#include <folly/Exception.h>
#include <folly/String.h>
#include <folly/io/IOBufQueue.h>
#include <folly/io/async/AsyncSocket.h>
#include <folly/io/async/EventBaseManager.h>
#include <tuple>

using folly::AsyncServerSocket;
using ReleasableDestructor = folly::AsyncSocket::ReleasableDestructor;
using folly::AsyncSocket;
using folly::EventBaseManager;
using folly::Future;
using folly::IOBuf;
using folly::SocketAddress;

namespace facebook {
namespace eden {

namespace {
class RpcTcpHandler {
  struct Reader : public folly::AsyncReader::ReadCallback {
    RpcTcpHandler* handler;

    void deleteMe() {
      if (handler) {
        delete handler;
        handler = nullptr;
      }
    }

    ~Reader() override {
      deleteMe();
    }

    explicit Reader(RpcTcpHandler* handler) : handler(handler) {}

    void getReadBuffer(void** bufP, size_t* lenP) override {
      auto [buf, len] = handler->readBuf_.preallocate(64, 64 * 1024);
      *lenP = len;
      *bufP = buf;
    }

    void readDataAvailable(size_t len) noexcept override {
      handler->readBuf_.postallocate(len);
      handler->tryConsumeReadBuffer();
    }

    bool isBufferMovable() noexcept override {
      // prefer to have getReadBuffer / readDataAvailable called
      // rather than readBufferAvailable.
      return true;
    }

    void readBufferAvailable(std::unique_ptr<IOBuf> readBuf) noexcept override {
      handler->readBuf_.append(std::move(readBuf));
      handler->tryConsumeReadBuffer();
    }

    void readEOF() noexcept override {
      deleteMe();
    }

    void readErr(const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while reading: " << folly::exceptionStr(ex);
      deleteMe();
    }
  };

  struct Writer : public folly::AsyncWriter::WriteCallback {
    RpcTcpHandler* handler;

    explicit Writer(RpcTcpHandler* handler) : handler(handler) {}

    void writeSuccess() noexcept override {}

    void writeErr(
        size_t /*bytesWritten*/,
        const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while writing: " << folly::exceptionStr(ex);
      delete handler;
    }
  };

  void dispatchAndReply(std::unique_ptr<folly::IOBuf> input) {
    // TODO(xavierd): `this` capture is unsafe due to the Reader and Writer
    // above deleting it on error.
    folly::makeFutureWith([this, input = std::move(input)]() mutable {
      XdrDeSerializer deser(input.get());
      rpc::rpc_msg_call call;
      deSerializeXdrInto(deser, call);

      auto resultBuf = IOBuf::create(1024);
      XdrSerializer ser(resultBuf.get(), 1024);
      ser.writeBE<uint32_t>(0); // reserve space for fragment header

      if (call.cbody.rpcvers != rpc::kRPCVersion) {
        rpc::rpc_msg_reply reply;
        reply.xid = call.xid;
        reply.mtype = rpc::msg_type::REPLY;

        rpc::mismatch_info mismatch = {rpc::kRPCVersion, rpc::kRPCVersion};

        rpc::rejected_reply rejected;
        rejected.set_RPC_MISMATCH(std::move(mismatch));

        rpc::reply_body body;
        body.set_MSG_DENIED(std::move(rejected));

        reply.rbody = std::move(body);
        serializeXdr(ser, reply);

        return folly::makeFuture(std::move(resultBuf));
      }

      if (auto auth = proc_->checkAuthentication(call.cbody);
          auth != rpc::auth_stat::AUTH_OK) {
        rpc::rpc_msg_reply reply;
        reply.xid = call.xid;
        reply.mtype = rpc::msg_type::REPLY;

        rpc::rejected_reply rejected;
        rejected.set_AUTH_ERROR(std::move(auth));

        rpc::reply_body body;
        body.set_MSG_DENIED(std::move(rejected));

        reply.rbody = std::move(body);
        serializeXdr(ser, reply);

        return folly::makeFuture(std::move(resultBuf));
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
           resultBuffer = std::move(resultBuf),
           call = std::move(call)](folly::Try<folly::Unit> result) mutable {
            if (result.hasException()) {
              // XXX: shrink resultBuffer and serialize a SYSTEM_ERR into it
              XLOGF(
                  WARN,
                  "Server failed to dispatch proc {} to {}:{}: {}",
                  call.cbody.proc,
                  call.cbody.prog,
                  call.cbody.vers,
                  folly::exceptionStr(*result.exception().get_exception()));
            }

            return std::move(resultBuffer);
          });
    })
        .via(this->sock_->getEventBase())
        .then([this](folly::Try<std::unique_ptr<folly::IOBuf>> result) {
          if (result.hasException()) {
            // XXX: This should never happen.
          } else {
            auto resultBuffer = std::move(result).value();

            // Fill out the fragment header.
            auto len = uint32_t(
                resultBuffer->computeChainDataLength() - sizeof(uint32_t));
            auto fragment = (uint32_t*)resultBuffer->writableData();
            *fragment = folly::Endian::big(len | 0x80000000);

            sock_->writeChain(&writer_, std::move(resultBuffer));
          }
        });
  }

  std::shared_ptr<RpcServerProcessor> proc_;
  std::unique_ptr<AsyncSocket, ReleasableDestructor> sock_;
  Reader reader_;
  Writer writer_;
  folly::IOBufQueue readBuf_;

 public:
  RpcTcpHandler(
      std::shared_ptr<RpcServerProcessor> proc,
      std::unique_ptr<AsyncSocket, ReleasableDestructor>&& socket_)
      : proc_(proc), sock_(std::move(socket_)), reader_(this), writer_(this) {}

  void setup() {
    sock_->setReadCB(&reader_);
  }

  void tryConsumeReadBuffer() noexcept {
    // Since we are TCP:
    // Then: decode framing information from start of buffer.
    // See if a complete frame is available.
    // If so, decode call_body and dispatch

    folly::io::Cursor c(readBuf_.front());
    while (true) {
      auto fragmentHeader = c.readBE<uint32_t>();
      auto len = fragmentHeader & 0x7fffffff;
      bool isLast = (fragmentHeader & 0x80000000) != 0;
      if (!c.canAdvance(len)) {
        // we don't have a complete request, so try again later
        return;
      }
      c.skip(len);
      if (isLast) {
        break;
      }
    }

    auto buf = readBuf_.split(c.getCurrentPosition());
    buf->coalesce();

    XLOG(DBG8) << "Received:\n" << folly::hexDump(buf->data(), buf->length());

    // Remove the fragment framing from the buffer
    // XXX: This is O(N^2) in the number of fragments.
    auto data = buf->writableData();
    auto remain = buf->length();
    size_t totalLength = 0;
    while (true) {
      auto fragmentHeader = folly::Endian::big(*(uint32_t*)data);
      auto len = fragmentHeader & 0x7fffffff;
      bool isLast = (fragmentHeader & 0x80000000) != 0;
      memmove(data, data + sizeof(uint32_t), remain - sizeof(uint32_t));
      totalLength += len;
      remain -= len + sizeof(uint32_t);
      data += len;
      if (isLast) {
        break;
      }
    }

    buf->trimEnd(buf->length() - totalLength);

    dispatchAndReply(std::move(buf));
  }
};

} // namespace

void RpcServer::RpcAcceptCallback::connectionAccepted(
    folly::NetworkSocket fd,
    const folly::SocketAddress& clientAddr) noexcept {
  XLOG(DBG7) << "Accepted connection from: " << clientAddr;
  auto eb = EventBaseManager::get()->getEventBase();
  auto socket = AsyncSocket::newSocket(eb, fd);
  auto handler = new RpcTcpHandler(proc, std::move(socket));
  handler->setup();
}

rpc::auth_stat RpcServerProcessor::checkAuthentication(
    const rpc::call_body& /*call_body*/) {
  // Completely ignore authentication.
  // TODO: something reasonable here
  return rpc::auth_stat::AUTH_OK;
}

Future<folly::Unit> RpcServerProcessor::dispatchRpc(
    XdrDeSerializer /*deser*/,
    XdrSerializer /*ser*/,
    uint32_t /*xid*/,
    uint32_t /*progNumber*/,
    uint32_t /*progVersion*/,
    uint32_t /*procNumber*/) {
  return folly::unit;
}

RpcServer::RpcServer(std::shared_ptr<RpcServerProcessor> proc)
    : acceptCb_(proc),
      serverSocket_(AsyncServerSocket::newSocket(
          EventBaseManager::get()->getEventBase())) {
  // Ask kernel to assign us a port on the loopback interface
  serverSocket_->bind(SocketAddress(network::NetworkUtil::getLocalIPv4(), 0));
  serverSocket_->listen(1024);

  auto eb = EventBaseManager::get()->getEventBase();
  serverSocket_->addAcceptCallback(&acceptCb_, eb);
  serverSocket_->startAccepting();
}

void RpcServer::registerService(uint32_t progNumber, uint32_t progVersion) {
  // Enumerate the addresses (in practice, just the loopback) and use the
  // port number we got from the kernel to register the mapping for
  // this program/version pair with rpcbind/portmap.
  auto addrs = serverSocket_->getAddresses();
  for (auto& addr : addrs) {
    PortmapMapping mapping{
        progNumber, progVersion, PortmapMapping::kProtoTcp, addr.getPort()};
    portMap_.setMapping(mapping);
    mappedPorts_.push_back(mapping);
  }
}

} // namespace eden
} // namespace facebook
