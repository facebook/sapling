/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/socket.h>
#include <sys/types.h>
#include <memory>
#include <vector>

#include <folly/File.h>
#include <folly/io/IOBuf.h>
#include <folly/io/async/AsyncTimeout.h>
#include <folly/io/async/DelayedDestruction.h>
#include <folly/io/async/EventHandler.h>

namespace folly {
class EventBase;
class exception_wrapper;
class SocketAddress;
} // namespace folly

namespace facebook {
namespace eden {

/**
 * A helper class for performing asynchronous I/O on a unix domain socket.
 *
 * This class is somewhat similar to folly::AsyncSocket, but is targeted at
 * supporting additional sending types of data only supported of unix domain
 * sockets.  In particular this class can also transfer file descriptors and
 * return credential information about the remote peer.
 *
 * This class is not thread safe.  It should only be accessed from the
 * EventBase thread that it is attached to.
 */
class UnixSocket : public folly::DelayedDestruction,
                   private folly::EventHandler,
                   private folly::AsyncTimeout {
 public:
  /**
   * A message that can be transferred over a UnixSocket.
   *
   * This may include normal data and file descriptors.
   */
  class Message {
   public:
    Message() {}
    explicit Message(folly::IOBuf&& data) : data(std::move(data)) {}
    explicit Message(std::vector<folly::File> files)
        : files(std::move(files)) {}
    Message(folly::IOBuf data, std::vector<folly::File> files)
        : data(std::move(data)), files(std::move(files)) {}

    folly::IOBuf data;
    std::vector<folly::File> files;
  };

  using UniquePtr = std::unique_ptr<UnixSocket, Destructor>;

  /**
   * A callback interface for receiving completion information about a
   * sendMessage() call.
   */
  class SendCallback {
   public:
    virtual ~SendCallback() {}

    /**
     * Called when the send completes successfully.
     *
     * Note that this does not mean that the message has been delivered to the
     * remote endpoint, merely that we have successfully finished giving the
     * data to the kernel to send.
     */
    virtual void sendSuccess() noexcept = 0;

    /**
     * Called when a send fails.
     *
     * After a send failure the socket will be in an error state and no further
     * sends or receives will be possible on the socket.
     */
    virtual void sendError(const folly::exception_wrapper& ew) noexcept = 0;
  };

  /**
   * A callback interface for receiving notifications when messages are
   * received on a UnixSocket.
   */
  class ReceiveCallback {
   public:
    virtual ~ReceiveCallback() {}

    /**
     * messageReceived() will be invoked when a new message is received.
     *
     * The ReceiveCallback will remain installed after messageReceived() and
     * will continue to get new messageReceived() calls in the future until
     * the ReceiveCallback is uninstalled or the socket is closed.
     */
    virtual void messageReceived(Message&& message) noexcept = 0;

    /**
     * eofReceived() will be called when the remote endpoint closes the
     * connection.
     */
    virtual void eofReceived() noexcept = 0;

    /**
     * socketClosed() will be called if the socket is closed locally while a
     * ReceiveCallback is installed.
     */
    virtual void socketClosed() noexcept = 0;

    /**
     * receiveError() wil be invoked when an error occurs on the socket.
     *
     * The socket will be in an error state once receiveError() is invoked, and
     * no further sends or receives will be possible on the socket.
     */
    virtual void receiveError(const folly::exception_wrapper& ew) noexcept = 0;
  };

  /**
   * A callback interface for waiting on connect() events.
   */
  class ConnectCallback {
   public:
    virtual ~ConnectCallback() {}

    /**
     * connectSuccess() will be called with the connected UnixSocket when the
     * connect operation succeeds.
     */
    virtual void connectSuccess(UnixSocket::UniquePtr socket) noexcept = 0;

    /**
     * connectError() will be invoked if the connect operation fails.
     */
    virtual void connectError(folly::exception_wrapper&& ew) noexcept = 0;
  };

  UnixSocket(folly::EventBase* eventBase, folly::File socket);

  template <typename... Args>
  static UniquePtr makeUnique(Args&&... args) {
    return UniquePtr{new UnixSocket(std::forward<Args>(args)...), Destructor()};
  }

  /**
   * Create a new UnixSocket by connecting to the specified address.
   */
  static void connect(
      ConnectCallback* callback,
      folly::EventBase* eventBase,
      folly::SocketAddress address,
      std::chrono::milliseconds timeout);

  /**
   * Create a new UnixSocket by connecting to the specified path.
   */
  static void connect(
      ConnectCallback* callback,
      folly::EventBase* eventBase,
      folly::StringPiece path,
      std::chrono::milliseconds timeout);

  folly::EventBase* getEventBase() const {
    return eventBase_;
  }

  /**
   * Attach this socket to an EventBase.
   *
   * This should only be called to set the EventBase if the UnixSocket
   * constructor was called with a null EventBase.  If the EventBase was not
   * set in the constructor then attachEventBase() must be called before any
   * calls to send() or setReceiveCallback().
   *
   * This method may only be called from the EventBase's thread.
   */
  void attachEventBase(folly::EventBase* eventBase);

  /**
   * Detach from the EventBase that is being used to drive this socket.
   *
   * This may only be called from the EventBase thread.
   */
  void detachEventBase();

  /**
   * Destroy the UnixSocket object.
   *
   * The UnixSocket destructor is private, so users must use destroy() instead
   * of manually deleting the object.
   */
  void destroy() override;

  /**
   * Close the socket.
   *
   * If there are messages still in the process of being sent this waits until
   * we have finished send those messages before fully closing the socket.
   *
   * The receive side of the socket is always closed immediately, and
   * socketClosed() is invoked on the ReceiveCallback if one is installed.
   *
   * New calls to setReceiveCallback() or sendMessage() will fail after close()
   * has been called.
   */
  void close();

  /**
   * Close the socket immediately.
   *
   * This is similar to close(), but immediately fails all pending sends rather
   * than waiting for them to complete.
   */
  void closeNow();

  /**
   * Get the user ID of the remote peer.
   */
  uid_t getRemoteUID();

  /**
   * Send a message over the socket.
   *
   * The callback may be null, in which case no notification will be provided
   * when the send succeeds or fails.
   */
  void send(Message&& message, SendCallback* callback = nullptr) noexcept;

  /**
   * Send data over the socket.
   */
  void send(folly::IOBuf&& data, SendCallback* callback = nullptr) noexcept;
  void send(
      std::unique_ptr<folly::IOBuf> data,
      SendCallback* callback = nullptr) noexcept;

  /**
   * Set the ReceiveCallback to be notified when data is received on this
   * socket.
   *
   * Throws an exception if a ReceiveCallback is alraedy installed on this
   * socket.
   */
  void setReceiveCallback(ReceiveCallback* callback);

  /**
   * Remove the ReceiveCallback currently installed on this socket.
   *
   * Throws an exception if no ReceiveCallback is currently installed.
   */
  void clearReceiveCallback();

  /**
   * Set the maximum data length allowed for incoming messages.
   *
   * Messages longer than this will be treated as an error.  This prevents us
   * from attempting to allocate very large data buffers based on remote
   * messages.
   */
  void setMaxRecvDataLength(uint32_t bytes);

  /**
   * Set the maximum number of files allowed on incoming messages.
   */
  void setMaxRecvFiles(uint32_t max);

  /**
   * Set the send timeout.
   *
   * The socket will be closed with an error if we have pending messages to
   * send and no progress is made within this period of time.  (The overall
   * message may take longer than this to send without triggering a timeout as
   * long as we can periodically make progress sending some data.)
   */
  void setSendTimeout(std::chrono::milliseconds timeout);

 private:
  struct Header {
    Header(uint64_t id, uint32_t data, uint32_t files)
        : protocolID{id}, dataSize{data}, numFiles{files} {}

    uint64_t protocolID;
    uint32_t dataSize;
    uint32_t numFiles;
  };
  enum : size_t { kHeaderLength = sizeof(uint64_t) + sizeof(uint32_t) * 2 };
  using HeaderBuffer = std::array<uint8_t, kHeaderLength>;
  enum : uint64_t { kProtocolID = 0xfaceb00c12345678ULL };

  class Connector;

  class SendQueueEntry;
  class SendQueueDestructor {
   public:
    void operator()(SendQueueEntry* entry) const;
  };
  using SendQueuePtr = std::unique_ptr<SendQueueEntry, SendQueueDestructor>;

  /**
   * SendQueueEntry is a node on our send queue.
   *
   * This contains the message to be sent, the callback to invoke when we
   * finish sending the message, as well as information about how much
   * information has been sent so far.
   */
  class SendQueueEntry {
   public:
    SendQueueEntry(
        Message&& message,
        SendCallback* callback,
        size_t iovecCount);

    Message message;
    SendCallback* callback{nullptr};
    SendQueuePtr next;
    size_t iovIndex{0};
    const size_t iovCount{0};
    size_t filesSent{0};
    HeaderBuffer header;

    /**
     * An array of iovec entries.
     * This is dynamically sized based on how many entries are needed.
     * SendQueueEntry objects are manually allocated to ensure that each
     * one has enough room for all of the iovec entries it needs.
     */
    struct iovec iov[0];
  };

  ~UnixSocket();

  static void
  serializeHeader(HeaderBuffer& buffer, uint32_t dataSize, uint32_t numFiles);
  static Header deserializeHeader(const HeaderBuffer& buffer);

  static SendQueuePtr createSendQueueEntry(
      Message&& message,
      SendCallback* callback);

  void trySend();
  bool trySendMessage(SendQueueEntry* entry);
  size_t initializeFirstControlMsg(
      std::vector<uint8_t>& controlBuf,
      struct msghdr* msg,
      SendQueueEntry* entry);
  size_t initializeAdditionalControlMsg(
      std::vector<uint8_t>& controlBuf,
      struct msghdr* msg,
      SendQueueEntry* entry);

  void tryReceive();
  bool tryReceiveOne();
  bool tryReceiveHeader();
  bool tryReceiveData();
  bool tryReceiveFiles();

  /**
   * Call recvmsg(), reading data into the supplied ByteRange.
   *
   * This also processes received control message data before returning.
   *
   * Returns the number of normal data bytes read on success,
   * 0 if the remote endpoint closed the connection, or -1 if EAGAIN was
   * returned.
   *
   * Throws an exception if an error other than EAGAIN occurred.
   */
  ssize_t callRecvMsg(folly::MutableByteRange buf);

  void processReceivedControlData(struct msghdr* msg);
  void processReceivedFiles(struct cmsghdr* cmsg);

  void registerForReads();
  void unregisterForReads();
  void registerForWrites();
  void unregisterForWrites();
  void unregisterIO();
  void updateIORegistration(uint16_t events);

  void handlerReady(uint16_t events) noexcept override;
  void timeoutExpired() noexcept override;

  void socketError(const folly::exception_wrapper& ew);
  void failAllSends(const folly::exception_wrapper& ew);

  folly::EventBase* eventBase_{nullptr};
  folly::File socket_;
  uint16_t registeredIOEvents_{0};
  bool closeStarted_{false};

  // The takeover data for a single monorepo can exceed 20 MB.  Allow
  // sufficiently large transfers while limiting the risk of making too large
  // of an allocation given bogus data.
  uint32_t maxDataLength_ = 512 * 1024 * 1024;
  uint32_t maxFiles_ = 100000;
  std::chrono::milliseconds sendTimeout_{250};

  ReceiveCallback* receiveCallback_{nullptr};
  HeaderBuffer recvHeaderBuffer_;
  std::vector<uint8_t> recvControlBuffer_;
  size_t headerBytesReceived_{0};
  Header recvHeader_{0, 0, 0};
  Message recvMessage_;

  SendQueuePtr sendQueue_;
  SendQueueEntry* sendQueueTail_{nullptr};
};

} // namespace eden
} // namespace facebook
