/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
#include "eden/fs/utils/UnixSocket.h"

#include <folly/Exception.h>
#include <folly/SocketAddress.h>
#include <folly/futures/Future.h>
#include <folly/io/Cursor.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/EventBase.h>
#include <folly/logging/xlog.h>
#include <folly/portability/Fcntl.h>
#include <folly/portability/Sockets.h>
#include <folly/portability/SysUio.h>
#include <algorithm>
#include <new>
#ifdef __APPLE__
#include <sys/ucred.h> // @manual
#endif
#include "eden/fs/utils/Bug.h"

using folly::ByteRange;
using folly::EventBase;
using folly::exception_wrapper;
using folly::File;
using folly::Future;
using folly::IOBuf;
using folly::make_exception_wrapper;
using folly::makeSystemError;
using folly::MutableByteRange;
using folly::throwSystemError;
using folly::throwSystemErrorExplicit;
using folly::Unit;
using folly::io::Cursor;
using folly::io::RWPrivateCursor;
using std::unique_ptr;
using std::vector;

#ifdef MSG_CMSG_CLOEXEC
#define HAVE_MSG_CMSG_CLOEXEC
#else
#define MSG_CMSG_CLOEXEC 0
#endif

namespace facebook {
namespace eden {

namespace {
/**
 * The maximum number of file descriptors that can be sent in a SCM_RIGHTS
 * control message.
 *
 * Linux internally defines this to 253 using the SCM_MAX_FD constant in
 * linux/include/net/scm.h
 */
constexpr size_t kMaxFDs = 253;
} // namespace

class UnixSocket::Connector : private folly::EventHandler, folly::AsyncTimeout {
 public:
  Connector(ConnectCallback* callback, EventBase* eventBase, File socket)
      : EventHandler{eventBase, folly::NetworkSocket::fromFd(socket.fd())},
        AsyncTimeout{eventBase},
        callback_{callback},
        eventBase_{eventBase},
        socket_{std::move(socket)} {}

  void start(std::chrono::milliseconds timeout) {
    scheduleTimeout(timeout);
    registerHandler(EventHandler::WRITE);
  }

 private:
  void handlerReady(uint16_t /* events */) noexcept override {
    cancelTimeout();

    // Call getsockopt() to check if the connect succeeded
    int error;
    socklen_t len = sizeof(error);
    int rv = getsockopt(socket_.fd(), SOL_SOCKET, SO_ERROR, &error, &len);
    if (rv == 0) {
      callback_->connectSuccess(
          UnixSocket::makeUnique(eventBase_, std::move(socket_)));
    } else {
      callback_->connectError(makeSystemError("connect failed on unix socket"));
    }
    delete this;
  }

  void timeoutExpired() noexcept override {
    unregisterHandler();
    callback_->connectError(folly::makeSystemErrorExplicit(
        ETIMEDOUT, "connect timeout on unix socket"));
    delete this;
  }

  ConnectCallback* const callback_{nullptr};
  EventBase* const eventBase_{nullptr};
  folly::File socket_;
};

UnixSocket::UnixSocket(EventBase* eventBase, File socket)
    : EventHandler{eventBase, folly::NetworkSocket::fromFd(socket.fd())},
      AsyncTimeout{eventBase},
      eventBase_{eventBase},
      socket_{std::move(socket)},
      // Create recvControlBuffer_ with enough capacity to receive
      // the maximum number of file descriptors that can be sent at once.
      recvControlBuffer_(CMSG_SPACE(kMaxFDs * sizeof(int))) {
  // on macOS, sendmsg() doesn't respect MSG_DONTWAIT at all.
  // Instead, the socket must be placed on non-blocking mode for
  // the sendmsg call to have non-blocking semantics.
  // Ensure that that is true here for all UnixSocket instances.
  auto rv = fcntl(socket_.fd(), F_SETFL, O_NONBLOCK);
  if (rv != 0) {
    throw std::runtime_error("failed to set O_NONBLOCK on unix socket");
  }
}

UnixSocket::~UnixSocket() {
  // The destructor should generally remain empty.
  // Most cleanup should be done in destroy() rather than in the destructor.
}

void UnixSocket::destroy() {
  // Close the socket to ensure we are unregistered from I/O and timeout
  // callbacks before our destructor starts.
  closeNow();

  // Call our parent's destroy() implementation
  DelayedDestruction::destroy();
}

void UnixSocket::attachEventBase(folly::EventBase* eventBase) {
  DCHECK(!eventBase_);
  eventBase_ = eventBase;
  EventHandler::attachEventBase(eventBase);
  AsyncTimeout::attachEventBase(eventBase);
}

void UnixSocket::detachEventBase() {
  DCHECK(eventBase_);
  eventBase_ = nullptr;
  EventHandler::detachEventBase();
  AsyncTimeout::detachEventBase();
}

void UnixSocket::connect(
    ConnectCallback* callback,
    EventBase* eventBase,
    folly::StringPiece path,
    std::chrono::milliseconds timeout) {
  folly::SocketAddress address;
  address.setFromPath(path);
  return connect(callback, eventBase, address, timeout);
}

void UnixSocket::connect(
    ConnectCallback* callback,
    EventBase* eventBase,
    folly::SocketAddress address,
    std::chrono::milliseconds timeout) {
  try {
    // Create the socket
    namespace fsp = folly::portability::sockets;
    int fd = fsp::socket(address.getFamily(), SOCK_STREAM, 0);
    if (fd < 0) {
      return callback->connectError(
          makeSystemError("failed to create unix socket"));
    }
    folly::File socketFile(fd, /* ownsFd */ true);
    int rv = fcntl(socketFile.fd(), F_SETFD, FD_CLOEXEC);
    if (rv != 0) {
      return callback->connectError(
          makeSystemError("failed to set FD_CLOEXEC on unix socket"));
    }
    rv = fcntl(socketFile.fd(), F_SETFL, O_NONBLOCK);
    if (rv != 0) {
      return callback->connectError(
          makeSystemError("failed to set O_NONBLOCK on unix socket"));
    }

    // Connect
    sockaddr_storage addrStorage;
    auto addrLen = address.getAddress(&addrStorage);
    rv = fsp::connect(
        socketFile.fd(), reinterpret_cast<sockaddr*>(&addrStorage), addrLen);
    if (rv == 0) {
      return callback->connectSuccess(
          makeUnique(eventBase, std::move(socketFile)));
    }
    if (errno != EAGAIN) {
      return callback->connectError(makeSystemError(
          "unable to connect to unix socket at ", address.describe()));
    }

    // If we are still here the connect blocked.
    // Create a Connector object to wait for it to complete.  The Connector
    // will destroy itself when the connect operation finishes.
    auto connector = new Connector(callback, eventBase, std::move(socketFile));
    connector->start(timeout);
    return;
  } catch (const std::exception& ex) {
    auto ew = exception_wrapper{std::current_exception(), ex};
    return callback->connectError(std::move(ew));
  }
}

void UnixSocket::close() {
  if (closeStarted_) {
    return;
  }

  // If we don't have a send queue we can close immediately.
  if (!sendQueue_) {
    closeNow();
    return;
  }

  // We have pending sends.  Just close the receive side for now.
  closeStarted_ = true;

  if (receiveCallback_) {
    unregisterForReads();
    auto callback = receiveCallback_;
    receiveCallback_ = nullptr;
    callback->socketClosed();
  }

  if (shutdown(socket_.fd(), SHUT_RD) != 0) {
    throwSystemError("error performing receive shutdown on UnixSocket");
  }
}

void UnixSocket::closeNow() {
  if (!socket_) {
    DCHECK(closeStarted_);
    DCHECK_EQ(registeredIOEvents_, 0);
    DCHECK(!isScheduled());
    DCHECK(!receiveCallback_);
    DCHECK(!sendQueue_);
    DCHECK(!sendQueueTail_);
    return;
  }

  DestructorGuard guard(this);
  closeStarted_ = true;

  // Make sure we unregister for IO events before closing our socket
  unregisterIO();
  // Go ahead and cancel our timeout too.
  cancelTimeout();

  if (receiveCallback_) {
    auto callback = receiveCallback_;
    receiveCallback_ = nullptr;
    callback->socketClosed();
  }

  if (sendQueue_) {
    auto error = make_exception_wrapper<std::system_error>(
        ENOTCONN, std::generic_category(), "unix socket closed");
    failAllSends(error);
  }

  socket_.close();
}

uid_t UnixSocket::getRemoteUID() {
  if (!socket_) {
    throw std::runtime_error(
        "cannot get the remote UID of a closed unix socket");
  }

  // We intentionally return only the user ID here, and not other values:
  //
  // - Linux's SO_PEERCRED option also returns the process ID, but BSD/Darwin's
  //   LOCAL_PEERCRED option does not.  Even on Linux, the remote process ID
  //   should only be used for debugging/logging purposes.  It generally
  //   shouldn't be used for other purposes since the remote process may have
  //   exited and the process ID could have been re-used by the time we process
  //   it here.
  //
  // - We don't return group information.  Linux's SO_PEERCRED only returns the
  //   remote process's primary group.  This generally isn't all that useful
  //   without supplemental group information as well.
  //
  // The user ID is the only useful value that we can retrieve on all the
  // platforms we currently care about.

#ifdef SO_PEERCRED
  struct ucred cred = {};
  constexpr int optname = SO_PEERCRED;
  constexpr int level = SOL_SOCKET;
#elif defined(LOCAL_PEERCRED)
  struct xucred cred = {};
  constexpr int optname = LOCAL_PEERCRED;
  constexpr int level = SOL_LOCAL;
#else
  static_assert("getting credentials not supported on this platform");
#endif

  socklen_t len = sizeof(cred);
  int result = getsockopt(socket_.fd(), level, optname, &cred, &len);
  folly::checkUnixError(result, "error getting unix socket peer credentials");

#ifdef SO_PEERCRED
  return cred.uid;
#elif defined(LOCAL_PEERCRED)
  return cred.cr_uid;
#endif
}

void UnixSocket::setMaxRecvDataLength(uint32_t bytes) {
  eventBase_->dcheckIsInEventBaseThread();
  maxDataLength_ = bytes;
}

void UnixSocket::setMaxRecvFiles(uint32_t max) {
  eventBase_->dcheckIsInEventBaseThread();
  maxFiles_ = max;
}

void UnixSocket::setSendTimeout(std::chrono::milliseconds timeout) {
  eventBase_->dcheckIsInEventBaseThread();
  sendTimeout_ = timeout;
}

void UnixSocket::send(unique_ptr<IOBuf> data, SendCallback* callback) noexcept {
  return send(Message(std::move(*data)), callback);
}

void UnixSocket::send(IOBuf&& data, SendCallback* callback) noexcept {
  return send(Message(std::move(data)), callback);
}

void UnixSocket::send(Message&& message, SendCallback* callback) noexcept {
  if (closeStarted_) {
    callback->sendError(make_exception_wrapper<std::runtime_error>(
        "cannot send a message on a closed UnixSocket"));
    return;
  }
  eventBase_->dcheckIsInEventBaseThread();

  // We can try sending immediately if there is nothing else already in the
  // queue.
  bool trySendNow = false;

  // Allocate a SendQueueEntry
  SendQueuePtr queueEntry;
  try {
    queueEntry = createSendQueueEntry(std::move(message), callback);
  } catch (const std::exception& ex) {
    auto ew = exception_wrapper{std::current_exception(), ex};
    XLOG(ERR) << "error allocating a send queue entry: " << ew.what();
    callback->sendError(make_exception_wrapper<std::runtime_error>(
        "cannot send a message on a closed UnixSocket"));
    return;
  }

  // Append the new SendQueueEntry to sendQueue_
  if (!sendQueueTail_) {
    DCHECK(!sendQueue_);
    trySendNow = true;
    sendQueue_ = std::move(queueEntry);
    sendQueueTail_ = sendQueue_.get();
  } else {
    DCHECK(sendQueue_);
    sendQueueTail_->next = std::move(queueEntry);
    sendQueueTail_ = sendQueueTail_->next.get();
  }

  if (trySendNow) {
    // If trySend() succeeds and invokes the send callback, make sure it
    // cannot destroy us until trySend() finishes.
    DestructorGuard guard(this);

    try {
      trySend();
    } catch (const std::exception& ex) {
      auto ew = exception_wrapper{std::current_exception(), ex};
      XLOG(ERR) << "unix socket error during send(): " << ew.what();
      socketError(ew);
    }
  }
}

// Iterates over an IOBuf and calls fn with a series of non-empty iovecs.
template <typename Fn>
static void enumerateIovecs(const IOBuf& buffer, Fn&& fn) {
  const IOBuf* buf = &buffer;
  do {
    if (buf->length() > 0) {
      fn(iovec{const_cast<uint8_t*>(buf->data()), buf->length()});
    }
    buf = buf->next();
  } while (buf != &buffer);
}

UnixSocket::SendQueueEntry::SendQueueEntry(
    Message&& msg,
    SendCallback* cb,
    size_t iovecCount)
    : message(std::move(msg)), callback(cb), iovCount(iovecCount) {
  iov[0].iov_base = header.data();
  iov[0].iov_len = header.size();

  size_t bodySize = 0;
  size_t idx = 1;
  enumerateIovecs(message.data, [&](const auto& iovec) {
    iov[idx++] = iovec;
    bodySize += iovec.iov_len;
  });

  DCHECK_EQ(iovCount, idx);

  serializeHeader(header, bodySize, message.files.size());
}

void UnixSocket::SendQueueDestructor::operator()(SendQueueEntry* entry) const {
#if __cpp_sized_deallocation
  size_t allocationSize =
      sizeof(SendQueueEntry) + sizeof(struct iovec[entry->iovCount]);
  entry->~SendQueueEntry();
  operator delete(entry, allocationSize);
#else
  entry->~SendQueueEntry();
  operator delete(entry);
#endif
}

UnixSocket::SendQueuePtr UnixSocket::createSendQueueEntry(
    Message&& message,
    SendCallback* callback) {
  // Compute how many iovec entries we will have.  We have 1 for the message
  // header plus one for each non-empty element in the IOBuf chain.
  size_t iovecElements = 1;
  enumerateIovecs(message.data, [&](const auto&) { ++iovecElements; });

  size_t allocationSize =
      sizeof(SendQueueEntry) + sizeof(struct iovec[iovecElements]);
  SendQueuePtr entry;
  void* data = operator new(allocationSize);
  try {
    entry.reset(
        new (data) SendQueueEntry(std::move(message), callback, iovecElements));
  } catch (const std::exception& ex) {
#if __cpp_sized_deallocation
    operator delete(data, allocationSize);
#else
    operator delete(data);
#endif
    throw;
  }

  return entry;
}

void UnixSocket::serializeHeader(
    HeaderBuffer& buffer,
    uint32_t dataSize,
    uint32_t numFiles) {
  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{buffer});
  RWPrivateCursor cursor(&buf);
  cursor.writeBE(static_cast<uint64_t>(kProtocolID));
  cursor.writeBE(static_cast<uint32_t>(dataSize));
  cursor.writeBE(static_cast<uint32_t>(numFiles));
  CHECK(cursor.isAtEnd());
}

UnixSocket::Header UnixSocket::deserializeHeader(const HeaderBuffer& buffer) {
  IOBuf buf(IOBuf::WRAP_BUFFER, ByteRange{buffer});
  Cursor cursor(&buf);
  auto id = cursor.readBE<uint64_t>();
  auto dataSize = cursor.readBE<uint32_t>();
  auto numFiles = cursor.readBE<uint32_t>();
  return Header{id, dataSize, numFiles};
}

void UnixSocket::trySend() {
  // If we have multiple message to send and write doesn't block,
  // break out after sending MAX_MSGS_AT_ONCE, just to yield the event loop
  // so that we don't starve other events that need to be handled.
  constexpr unsigned int MAX_MSGS_AT_ONCE = 10;
  for (unsigned int n = 0; n < MAX_MSGS_AT_ONCE; ++n) {
    if (!sendQueue_) {
      break;
    }

    if (!trySendMessage(sendQueue_.get())) {
      // The write blocked, and we need to retry this message again
      // after waiting for the socket to become writable.
      break;
    }
    auto* callback = sendQueue_->callback;
    sendQueue_ = std::move(sendQueue_->next);
    if (!sendQueue_) {
      sendQueueTail_ = nullptr;
    }
    if (callback) {
      callback->sendSuccess();
    }
  }

  // Update our I/O event and timeout registration
  if (!sendQueue_) {
    cancelTimeout();
    unregisterForWrites();

    // If we have started closing, finish closing now that we have
    // emptied our send queue.
    if (closeStarted_) {
      closeNow();
    }
  } else {
    scheduleTimeout(sendTimeout_);
    registerForWrites();
  }
}

bool UnixSocket::trySendMessage(SendQueueEntry* entry) {
  uint8_t dataByte = 0;
  struct msghdr msg = {};

  vector<uint8_t> controlBuf;
  size_t filesToSend = 0;
  if (entry->iovIndex < entry->iovCount) {
    msg.msg_iov = entry->iov + entry->iovIndex;
    // Send at most IOV_MAX chunks at once; the send may fail with EMSGSIZE
    // if we send too many iovecs at once.
    msg.msg_iovlen =
        std::min(entry->iovCount - entry->iovIndex, folly::kIovMax);

    // Include FDs if we have them
    bool isFirstSend = entry->iovIndex == 0 &&
        (entry->iov[0].iov_base == entry->header.data());
    if (isFirstSend) {
      filesToSend = initializeFirstControlMsg(controlBuf, &msg, entry);
    }
    XLOG(DBG9) << "trySendMessage(): iovIndex=" << entry->iovIndex
               << " iovCount=" << entry->iovCount
               << ", controlLength=" << msg.msg_controllen;
  } else {
    // We finished sending the normal message data, but still have more
    // file descriptors to send.  (We had more FDs than could fit in a single
    // sendmsg() call.)
    //
    // We send more than kMaxFDs in additional send calls after the main
    // message body.  We have to include at least 1 byte of normal data in each
    // sendmsg() call, so we send a single 0 byte with each remainging chunk of
    // FDs.
    CHECK_LT(entry->filesSent, entry->message.files.size());
    // We re-use the header iovec to point at our 1 byte of data,
    // since we are don sending the header and don't need it to point at the
    // header any more.
    entry->iov[0].iov_base = &dataByte;
    entry->iov[0].iov_len = sizeof(dataByte);
    msg.msg_iov = entry->iov;
    msg.msg_iovlen = 1;
    filesToSend = initializeAdditionalControlMsg(controlBuf, &msg, entry);
    XLOG(DBG9) << "trySendMessage(): controlLength=" << msg.msg_controllen;
  }

  // Now call sendmsg.
  // Portability concern: MSG_DONTWAIT is not documented at all in the
  // macOS sendmsg() man page, and the obvserved behavior is that it
  // has no effect at all on sendmsg().  Instead, the socket must be
  // in non-blocking mode if we want non-blocking behavior!
  auto bytesSent = sendmsg(socket_.fd(), &msg, MSG_DONTWAIT);
  XLOG(DBG9) << "sendmsg() returned " << bytesSent
             << ", files sent: " << filesToSend;
  if (bytesSent < 0) {
    if (errno == EAGAIN) {
      return false;
    }
    throwSystemError("sendmsg() failed on UnixSocket");
  }

  if (entry->iovIndex < entry->iovCount) {
    // Update entry->iov and entry->iovIndex to account for the data that was
    // successfully sent.
    while (bytesSent > 0) {
      if (static_cast<size_t>(bytesSent) >=
          entry->iov[entry->iovIndex].iov_len) {
        bytesSent -= entry->iov[entry->iovIndex].iov_len;
        ++entry->iovIndex;
      } else {
        auto* iov = entry->iov + entry->iovIndex;
        iov->iov_len -= bytesSent;
        iov->iov_base = static_cast<char*>(iov->iov_base) + bytesSent;
        break;
      }
    }
  }

  // Update entry->filesSent to account for the file descriptors we sent.
  entry->filesSent += filesToSend;

  // Return true if we sent everything.
  return (
      entry->iovIndex == entry->iovCount &&
      entry->filesSent == entry->message.files.size());
}

size_t UnixSocket::initializeFirstControlMsg(
    vector<uint8_t>& controlBuf,
    struct msghdr* msg,
    SendQueueEntry* entry) {
  const auto& message = entry->message;
  if (message.files.empty()) {
    return 0;
  }

  // Compute how much space we need for the control data
  size_t fdsToSend = std::min(kMaxFDs, message.files.size());
  size_t cmsgSpace = CMSG_SPACE(fdsToSend * sizeof(int));

  // Allocate the buffer
  controlBuf.resize(cmsgSpace);
  msg->msg_control = controlBuf.data();
  msg->msg_controllen = controlBuf.size();

  // Initialize the data
  struct cmsghdr* hdr = CMSG_FIRSTHDR(msg);
  DCHECK(hdr);
  DCHECK_GT(fdsToSend, 0);
  hdr->cmsg_len = CMSG_LEN(fdsToSend * sizeof(int));
  hdr->cmsg_level = SOL_SOCKET;
  hdr->cmsg_type = SCM_RIGHTS;

  auto* data = reinterpret_cast<int*>(CMSG_DATA(hdr));
  for (size_t n = 0; n < fdsToSend; ++n) {
    data[n] = message.files[n].fd();
  }

  return fdsToSend;
}

size_t UnixSocket::initializeAdditionalControlMsg(
    vector<uint8_t>& controlBuf,
    struct msghdr* msg,
    SendQueueEntry* entry) {
  const auto& message = entry->message;

  DCHECK(!message.files.empty());
  DCHECK_GT(entry->filesSent, 0);

  size_t fdsToSend = std::min(kMaxFDs, message.files.size() - entry->filesSent);
  auto cmsgSpace = CMSG_SPACE(fdsToSend * sizeof(int));

  controlBuf.resize(cmsgSpace);
  msg->msg_control = controlBuf.data();
  msg->msg_controllen = controlBuf.size();

  struct cmsghdr* hdr = reinterpret_cast<struct cmsghdr*>(controlBuf.data());
  hdr->cmsg_len = CMSG_LEN(fdsToSend * sizeof(int));
  hdr->cmsg_level = SOL_SOCKET;
  hdr->cmsg_type = SCM_RIGHTS;
  auto* data = reinterpret_cast<int*>(CMSG_DATA(hdr));
  for (size_t n = 0; n < fdsToSend; ++n) {
    data[n] = message.files[entry->filesSent + n].fd();
  }
  return fdsToSend;
}

void UnixSocket::setReceiveCallback(ReceiveCallback* callback) {
  if (receiveCallback_) {
    throw std::runtime_error(
        "a receive callback is already installed on this UnixSocket");
  }
  if (closeStarted_) {
    throw std::runtime_error(
        "cannot set a receive callback on a closed UnixSocket");
  }
  eventBase_->dcheckIsInEventBaseThread();
  receiveCallback_ = callback;
  registerForReads();
}

void UnixSocket::clearReceiveCallback() {
  if (!receiveCallback_) {
    throw std::runtime_error(
        "no receive callback currently installed on this UnixSocket");
  }
  eventBase_->dcheckIsInEventBaseThread();
  receiveCallback_ = nullptr;
  unregisterForReads();
}

void UnixSocket::tryReceive() {
  DCHECK(receiveCallback_);

  // Set a limit on the number of messages we process at once in one EventBase
  // loop iteration, to avoid starving other EventBase callbacks.
  size_t maxMessagesAtOnce = 10;
  for (size_t n = 0; n < maxMessagesAtOnce; ++n) {
    // Stop if the receiveCallback_ gets uninstalled
    if (!receiveCallback_) {
      break;
    }

    // Try receiving message data.
    // Break if we didn't receive the full message yet.
    if (!tryReceiveOne()) {
      break;
    }

    // We finished receiveing a full message.
    // Reset headerBytesReceived_ and invoke the receive callback.
    headerBytesReceived_ = 0;
    receiveCallback_->messageReceived(Message{std::move(recvMessage_)});
  }
}

bool UnixSocket::tryReceiveOne() {
  if (headerBytesReceived_ < recvHeaderBuffer_.size()) {
    DCHECK_EQ(recvMessage_.data.length(), 0);
    DCHECK_EQ(recvMessage_.files.size(), 0);

    if (!tryReceiveHeader()) {
      return false;
    }

    // Deserialize and check the header
    recvHeader_ = deserializeHeader(recvHeaderBuffer_);
    if (recvHeader_.protocolID != kProtocolID) {
      throwSystemErrorExplicit(
          ECONNABORTED,
          "unknown protocol ID received from remote unix socket endpoint: ",
          recvHeader_.protocolID,
          " != ",
          kProtocolID);
    }
    if (recvHeader_.dataSize > maxDataLength_) {
      throwSystemErrorExplicit(
          ECONNABORTED,
          "remote endpoint sent unreasonably large message: length=",
          recvHeader_.dataSize);
    }
    if (recvHeader_.numFiles > maxFiles_) {
      throwSystemErrorExplicit(
          ECONNABORTED,
          "remote endpoint sent unreasonably large number of files: numFDs=",
          recvHeader_.numFiles);
    }

    if (recvHeader_.dataSize > 0) {
      recvMessage_.data = IOBuf(IOBuf::CREATE, recvHeader_.dataSize);
    }
  }

  if (recvMessage_.data.computeChainDataLength() < recvHeader_.dataSize) {
    if (!tryReceiveData()) {
      return false;
    }
  }

  if (recvMessage_.files.size() < recvHeader_.numFiles) {
    if (!tryReceiveFiles()) {
      return false;
    }
  }

  return true;
}

void UnixSocket::processReceivedControlData(struct msghdr* msg) {
  struct cmsghdr* cmsg = CMSG_FIRSTHDR(msg);
  while (cmsg) {
    XLOG(DBG9) << "received control msg: level=" << cmsg->cmsg_level
               << ", type=" << cmsg->cmsg_type;
    if (cmsg->cmsg_level != SOL_SOCKET) {
      XLOG(WARN) << "unexpected control message level on unix socket: ("
                 << cmsg->cmsg_level << ", " << cmsg->cmsg_type << ")";
    } else if (cmsg->cmsg_type == SCM_RIGHTS) {
      processReceivedFiles(cmsg);
    } else {
      XLOG(WARN) << "unexpected control message type on unix socket: ("
                 << cmsg->cmsg_level << ", " << cmsg->cmsg_type << ")";
    }

    cmsg = CMSG_NXTHDR(msg, cmsg);
  }
}

void UnixSocket::processReceivedFiles(struct cmsghdr* cmsg) {
  if (cmsg->cmsg_len < CMSG_LEN(sizeof(int))) {
    throwSystemErrorExplicit(
        ECONNABORTED,
        "received truncated SCM_RIGHTS message data: length=",
        cmsg->cmsg_len);
  }
  size_t dataLength = cmsg->cmsg_len - CMSG_LEN(0);

  size_t numFDs = dataLength / sizeof(int);
  DCHECK_EQ(dataLength % sizeof(int), 0)
      << "expected an even number of file descriptors: size=" << dataLength;

  auto* data = reinterpret_cast<const int*>(CMSG_DATA(cmsg));
  for (size_t n = 0; n < numFDs; ++n) {
    auto fd = data[n];
#ifndef HAVE_MSG_CMSG_CLOEXEC
    // We don't have atomic FD_CLOEXEC setting ability, so make a best
    // effort attempt at setting it here, and hope that it doesn't escape
    // into a newly spawned import helper
    auto flags = fcntl(fd, F_GETFD);
    folly::checkPosixError(flags);
    folly::checkPosixError(fcntl(fd, F_SETFD, flags | FD_CLOEXEC));
#endif
    recvMessage_.files.push_back(File{fd, /* ownsFd */ true});
  }
}

ssize_t UnixSocket::callRecvMsg(MutableByteRange buf) {
  struct iovec iov;
  iov.iov_base = buf.data();
  iov.iov_len = buf.size();

  struct msghdr msg;
  msg.msg_name = nullptr;
  msg.msg_namelen = 0;
  msg.msg_iov = &iov;
  msg.msg_iovlen = 1;
  msg.msg_control = recvControlBuffer_.data();
  msg.msg_controllen = recvControlBuffer_.size();
  msg.msg_flags = 0;

  auto bytesReceived =
      recvmsg(socket_.fd(), &msg, MSG_CMSG_CLOEXEC | MSG_DONTWAIT);
  XLOG(DBG9) << "recvmsg(): got " << bytesReceived << " data bytes, "
             << msg.msg_controllen << " control bytes";
  if (bytesReceived < 0) {
    if (errno == EAGAIN) {
      return -1;
    }
    throwSystemError("recvmsg() failed on unix socket");
  }

  if (msg.msg_flags == MSG_CTRUNC) {
    throwSystemError(
        "truncated control message data when receiving on unix socket");
  }

  processReceivedControlData(&msg);
  return bytesReceived;
}

bool UnixSocket::tryReceiveHeader() {
  MutableByteRange buf{recvHeaderBuffer_.data(), recvHeaderBuffer_.size()};
  buf.advance(headerBytesReceived_);

  auto bytesReceived = callRecvMsg(buf);
  if (bytesReceived < 0) {
    if (errno == EAGAIN) {
      return false;
    }
    throwSystemError("error receiving message header on unix socket");
  }
  if (bytesReceived == 0) {
    if (headerBytesReceived_ == 0) {
      receiveCallback_->eofReceived();
      return false;
    }
    throwSystemErrorExplicit(
        ECONNABORTED,
        "remote endpoint closed connection partway "
        "through a unix socket message header");
  }

  headerBytesReceived_ += bytesReceived;
  return headerBytesReceived_ == recvHeaderBuffer_.size();
}

bool UnixSocket::tryReceiveData() {
  auto dataToRead =
      recvHeader_.dataSize - recvMessage_.data.computeChainDataLength();
  auto bytesReceived = callRecvMsg(
      MutableByteRange{recvMessage_.data.writableTail(), dataToRead});
  if (bytesReceived < 0) {
    return false;
  }
  if (bytesReceived == 0) {
    throwSystemErrorExplicit(
        ECONNABORTED,
        "remote endpoint closed connection partway "
        "through a unix socket message");
  }

  recvMessage_.data.append(bytesReceived);
  if (static_cast<size_t>(bytesReceived) == dataToRead) {
    return true;
  }
  return false;
}

bool UnixSocket::tryReceiveFiles() {
  uint8_t dataByte = 0;
  MutableByteRange buf(&dataByte, sizeof(dataByte));
  auto bytesReceived = callRecvMsg(buf);
  if (bytesReceived < 0) {
    return false;
  }
  if (bytesReceived == 0) {
    throwSystemErrorExplicit(
        ECONNABORTED,
        "remote endpoint closed connection partway "
        "through a unix socket FD message");
  }

  if (recvMessage_.files.size() > recvHeader_.numFiles) {
    throwSystemErrorExplicit(
        ECONNABORTED,
        "remote endpoint sent more file descriptors than indicated "
        "in the unix socket message header: ",
        recvMessage_.files.size(),
        " > ",
        recvHeader_.numFiles);
  }
  return (recvMessage_.files.size() == recvHeader_.numFiles);
}

void UnixSocket::registerForReads() {
  updateIORegistration(registeredIOEvents_ | EventHandler::READ);
}

void UnixSocket::unregisterForReads() {
  updateIORegistration(registeredIOEvents_ & ~EventHandler::READ);
}

void UnixSocket::registerForWrites() {
  updateIORegistration(registeredIOEvents_ | EventHandler::WRITE);
}

void UnixSocket::unregisterForWrites() {
  updateIORegistration(registeredIOEvents_ & ~EventHandler::WRITE);
}

void UnixSocket::unregisterIO() {
  registeredIOEvents_ = 0;
  unregisterHandler();
}

void UnixSocket::updateIORegistration(uint16_t newEvents) {
  if (registeredIOEvents_ == newEvents) {
    return;
  }

  if (newEvents) {
    // We always use the PERSIST flag when we are registered
    registerHandler(newEvents | EventHandler::PERSIST);
  } else {
    unregisterHandler();
  }
  registeredIOEvents_ = newEvents;
}

void UnixSocket::socketError(const exception_wrapper& ew) {
  // In case socketError() gets called when we are already closed,
  // just return immediately.
  if (!socket_) {
    DCHECK_EQ(registeredIOEvents_, 0);
    DCHECK(!isScheduled());
    DCHECK(!receiveCallback_);
    DCHECK(!sendQueue_);
    DCHECK(!sendQueueTail_);
    return;
  }

  // If any of the error callbacks try to destroy us, delay actually invoking
  // our destructor until we have finished invoking all callbacks.
  DestructorGuard guard(this);

  // Close the socket so that future send/receive attempts will fail
  closeStarted_ = true;
  unregisterIO();
  cancelTimeout();
  socket_.close();

  if (receiveCallback_) {
    auto callback = receiveCallback_;
    receiveCallback_ = nullptr;
    callback->receiveError(ew);
  }

  failAllSends(ew);
}

void UnixSocket::failAllSends(const exception_wrapper& ew) {
  while (sendQueue_) {
    auto* callback = sendQueue_->callback;
    sendQueue_ = std::move(sendQueue_->next);
    if (!sendQueue_) {
      sendQueueTail_ = nullptr;
    }
    if (callback) {
      callback->sendError(ew);
    }
  }
}

void UnixSocket::handlerReady(uint16_t events) noexcept {
  // In case a send or receive callback calls destroy(), make sure we aren't
  // destroyed immediately while handlerReady() is still running.
  DestructorGuard guard(this);

  try {
    if (events & EventHandler::READ) {
      tryReceive();
    }
    if (events & EventHandler::WRITE) {
      trySend();
    }
  } catch (const std::exception& ex) {
    auto ew = exception_wrapper{std::current_exception(), ex};
    XLOG(ERR) << "unix socket I/O handler error: " << ew.what();
    socketError(ew);
  }
}

void UnixSocket::timeoutExpired() noexcept {
  XLOG(WARN) << "send timeout on unix socket";
  socketError(
      folly::makeSystemErrorExplicit(ETIMEDOUT, "send timeout on unix socket"));
}

} // namespace eden
} // namespace facebook
