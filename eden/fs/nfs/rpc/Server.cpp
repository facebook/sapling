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
using ReleasableDestructor = folly::AsyncSocket::ReleasableDestructor;
using folly::AsyncSocket;
using folly::Future;
using folly::IOBuf;
using folly::SocketAddress;

namespace facebook {
namespace eden {

namespace {
class RpcTcpHandler : public folly::DelayedDestruction {
  struct Reader : public folly::AsyncReader::ReadCallback {
    RpcTcpHandler* handler_;
    DestructorGuard guard_;

    void deleteMe() {
      handler_->resetReader();
    }

    explicit Reader(RpcTcpHandler* handler)
        : handler_(handler), guard_(handler_) {}

    void getReadBuffer(void** bufP, size_t* lenP) override {
      auto [buf, len] = handler_->readBuf_.preallocate(64, 64 * 1024);
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

    void readEOF() noexcept override {
      deleteMe();
    }

    void readErr(const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while reading: " << folly::exceptionStr(ex);
      deleteMe();
    }
  };

  struct Writer : public folly::AsyncWriter::WriteCallback {
    Writer() = default;

    void writeSuccess() noexcept override {}

    void writeErr(
        size_t /*bytesWritten*/,
        const folly::AsyncSocketException& ex) noexcept override {
      XLOG(ERR) << "Error while writing: " << folly::exceptionStr(ex);
    }
  };

  void dispatchAndReply(std::unique_ptr<folly::IOBuf> input) {
    DestructorGuard guard(this);

    folly::makeFutureWith([this, input = std::move(input)]() mutable {
      folly::io::Cursor deser(input.get());
      rpc_msg_call call = XdrTrait<rpc_msg_call>::deserialize(deser);

      auto resultBuf = IOBuf::create(1024);
      folly::io::Appender ser(resultBuf.get(), 1024);
      XdrTrait<uint32_t>::serialize(
          ser, 0); // reserve space for fragment header

      if (call.cbody.rpcvers != kRPCVersion) {
        rpc_msg_reply reply{
            call.xid,
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

        return folly::makeFuture(std::move(resultBuf));
      }

      if (auto auth = proc_->checkAuthentication(call.cbody);
          auth != auth_stat::AUTH_OK) {
        rpc_msg_reply reply{
            call.xid,
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
        .then([this, guard = std::move(guard)](
                  folly::Try<std::unique_ptr<folly::IOBuf>> result) {
          if (result.hasException()) {
            // XXX: This should never happen.
          } else {
            auto resultBuffer = std::move(result).value();

            // Fill out the fragment header.
            auto len = uint32_t(
                resultBuffer->computeChainDataLength() - sizeof(uint32_t));
            auto fragment = (uint32_t*)resultBuffer->writableData();
            *fragment = folly::Endian::big(len | 0x80000000);

            XLOG(DBG8) << "Sending:\n"
                       << folly::hexDump(
                              resultBuffer->data(), resultBuffer->length());

            sock_->writeChain(&writer_, std::move(resultBuffer));
          }
        });
  }

  std::shared_ptr<RpcServerProcessor> proc_;
  std::unique_ptr<AsyncSocket, ReleasableDestructor> sock_;
  std::unique_ptr<Reader> reader_;
  Writer writer_;
  folly::IOBufQueue readBuf_;

 public:
  RpcTcpHandler(
      std::shared_ptr<RpcServerProcessor> proc,
      std::unique_ptr<AsyncSocket, ReleasableDestructor>&& socket_)
      : proc_(proc),
        sock_(std::move(socket_)),
        reader_(std::make_unique<Reader>(this)),
        writer_() {}

  void resetReader() {
    reader_.reset();
  }

  void setup() {
    sock_->setReadCB(reader_.get());
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
  auto socket = AsyncSocket::newSocket(evb_, fd);
  using UniquePtr =
      std::unique_ptr<RpcTcpHandler, folly::DelayedDestruction::Destructor>;
  auto handler = UniquePtr(
      new RpcTcpHandler(proc_, std::move(socket)),
      folly::DelayedDestruction::Destructor());
  handler->setup();
}

auth_stat RpcServerProcessor::checkAuthentication(
    const call_body& /*call_body*/) {
  // Completely ignore authentication.
  // TODO: something reasonable here
  return auth_stat::AUTH_OK;
}

Future<folly::Unit> RpcServerProcessor::dispatchRpc(
    folly::io::Cursor /*deser*/,
    folly::io::Appender /*ser*/,
    uint32_t /*xid*/,
    uint32_t /*progNumber*/,
    uint32_t /*progVersion*/,
    uint32_t /*procNumber*/) {
  return folly::unit;
}

RpcServer::RpcServer(
    std::shared_ptr<RpcServerProcessor> proc,
    folly::EventBase* evb)
    : evb_(evb),
      acceptCb_(proc, evb_),
      serverSocket_(AsyncServerSocket::newSocket(evb_)) {
  // Ask kernel to assign us a port on the loopback interface
  serverSocket_->bind(SocketAddress("127.0.0.1", 0));
  serverSocket_->listen(1024);

  serverSocket_->addAcceptCallback(&acceptCb_, evb_);
  serverSocket_->startAccepting();
}

RpcServer::~RpcServer() {
  for (const auto& mapping : mappedPorts_) {
    portMap_.unsetMapping(mapping);
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
  // Enumerate the addresses (in practice, just the loopback) and use the
  // port number we got from the kernel to register the mapping for
  // this program/version pair with rpcbind/portmap.
  auto addrs = serverSocket_->getAddresses();
  for (auto& addr : addrs) {
    auto [netid, addrStr] = getNetIdAndAddr(addr);
    PortmapMapping mapping{progNumber, progVersion, netid, addrStr, "edenfs"};
    portMap_.setMapping(mapping);
    mappedPorts_.push_back(mapping);
  }
}

uint16_t RpcServer::getPort() const {
  return serverSocket_->getAddress().getPort();
}

} // namespace eden
} // namespace facebook

#endif
